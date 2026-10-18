//! Versioned registry model and atomic persistence.
//!
//! Callers must hold a shared registry lock for reads or an exclusive lock around
//! the complete read-modify-write/reconcile operation; see [`lock::RegistryLock`].
//! Missing instances remain loadable for doctor; existing paths must be canonical.
use std::{
    collections::BTreeMap,
    fmt, io,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    error::Error,
    names::{ProjectName, SkillName},
    paths::{HomePath, RelPath},
    services::fs::Fs,
};

pub mod lock;
#[cfg(unix)]
pub mod resolve;

pub const SCHEMA_VERSION: u32 = 1;

fn invalid(message: impl Into<String>) -> Error {
    Error::Registry {
        message: message.into(),
        hint: "Correct registry.yml or restore its backup before retrying.".into(),
    }
}

fn io_error(operation: String, source: io::Error) -> Error {
    Error::Io {
        operation,
        source,
        hint: "Check the registry path and filesystem permissions.".into(),
    }
}

/// A pinned exact semantic version, or the mutable authoring draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Pin {
    Version(Version),
    Draft,
}

impl FromStr for Pin {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        if value == "draft" {
            Ok(Self::Draft)
        } else {
            Version::parse(value).map(Self::Version).map_err(|error| {
                invalid(format!(
                    "Invalid skill pin {value:?}: {error}; expected semver or 'draft'"
                ))
            })
        }
    }
}
impl TryFrom<String> for Pin {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        value.parse()
    }
}
impl From<Pin> for String {
    fn from(value: Pin) -> Self {
        value.to_string()
    }
}
impl fmt::Display for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version(version) => version.fmt(f),
            Self::Draft => f.write_str("draft"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub global: Global,
    #[serde(default, deserialize_with = "unique_map")]
    pub projects: BTreeMap<ProjectName, Project>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            settings: Settings::default(),
            global: Global::default(),
            projects: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub link_targets: Vec<RelPath>,
    pub git_exclude: bool,
    pub global_link_targets: Vec<HomePath>,
    pub global_in_instances: bool,
    /// Retained until a successful relink --all cleans obsolete targets.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub previous_link_targets: Vec<RelPath>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            link_targets: [".agents/skills", ".claude/skills"]
                .into_iter()
                .map(|path| path.parse().expect("valid default target"))
                .collect(),
            git_exclude: true,
            global_link_targets: vec!["~/.claude/skills".parse().expect("valid home target")],
            global_in_instances: false,
            previous_link_targets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Global {
    #[serde(default, deserialize_with = "unique_map")]
    pub skills: BTreeMap<SkillName, Pin>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    #[serde(default, deserialize_with = "unique_map")]
    pub skills: BTreeMap<SkillName, Pin>,
    #[serde(default)]
    pub instances: Vec<Instance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub path: PathBuf,
    pub attached_at: DateTime<Utc>,
    #[serde(default)]
    pub created_dirs: Vec<RelPath>,
}

// Serde's BTreeMap otherwise silently replaces duplicate keys.
fn unique_map<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
{
    struct Visitor<K, V>(std::marker::PhantomData<(K, V)>);
    impl<'de, K: Deserialize<'de> + Ord, V: Deserialize<'de>> serde::de::Visitor<'de>
        for Visitor<K, V>
    {
        type Value = BTreeMap<K, V>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a mapping with unique keys")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut access: A,
        ) -> Result<Self::Value, A::Error> {
            let mut map = BTreeMap::new();
            while let Some((key, value)) = access.next_entry()? {
                if map.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate registry map key"));
                }
            }
            Ok(map)
        }
    }
    deserializer.deserialize_map(Visitor(std::marker::PhantomData))
}

fn check_schema(schema: u32) -> Result<(), Error> {
    if schema != SCHEMA_VERSION {
        return Err(Error::Registry {
            message: format!("Unsupported registry schema {schema}; supported schema is {SCHEMA_VERSION}"),
            hint: "Upgrade skillctl to a version supporting this registry schema; do not edit the schema number.".into(),
        });
    }
    Ok(())
}

impl Registry {
    /// Parse YAML and enforce registry invariants, without checking installed versions.
    pub fn from_yaml(bytes: &[u8]) -> Result<Self, Error> {
        // Check the schema before interpreting fields of a potentially newer format.
        #[derive(Deserialize)]
        struct Header {
            schema: u32,
        }
        let header: Header = serde_yaml_ng::from_slice(bytes)
            .map_err(|error| invalid(format!("Cannot parse registry header: {error}")))?;
        check_schema(header.schema)?;
        let registry: Self = serde_yaml_ng::from_slice(bytes)
            .map_err(|error| invalid(format!("Cannot parse registry YAML: {error}")))?;
        registry.validate()?;
        Ok(registry)
    }

    pub fn to_yaml(&self) -> Result<String, Error> {
        self.validate()?;
        serde_yaml_ng::to_string(self)
            .map_err(|error| invalid(format!("Cannot serialize registry: {error}")))
    }

    /// Missing files are errors; initialization is the responsibility of item 10.
    pub fn load(fs: &dyn Fs, path: &Path) -> Result<Self, Error> {
        let bytes = fs
            .read(path)
            .map_err(|error| io_error(format!("Read {}", path.display()), error))?;
        Self::from_yaml(&bytes)
    }

    /// Back up exact previous bytes to `<filename>.bak-<schema>` before atomic replacement.
    /// No migrations exist yet: schema 1 is the only supported format.
    pub fn save(&self, fs: &dyn Fs, path: &Path) -> Result<(), Error> {
        let yaml = self.to_yaml()?;
        match fs.read(path) {
            Ok(bytes) => {
                // Never overwrite a malformed or unsupported registry.
                let previous = Self::from_yaml(&bytes)?;
                let backup = Self::backup_path(path, previous.schema);
                fs.write_atomic(&backup, &bytes)
                    .map_err(|error| io_error(format!("Back up {}", path.display()), error))?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(io_error(
                    format!("Read {} before saving", path.display()),
                    error,
                ));
            }
        }
        fs.write_atomic(path, yaml.as_bytes())
            .map_err(|error| io_error(format!("Write {}", path.display()), error))
    }

    pub fn backup_path(path: &Path, schema: u32) -> PathBuf {
        let mut name = path.as_os_str().to_os_string();
        name.push(format!(".bak-{schema}"));
        PathBuf::from(name)
    }

    /// Missing folders stay registered so doctor can diagnose them later.
    pub fn validate(&self) -> Result<(), Error> {
        check_schema(self.schema)?;
        let mut paths: Vec<(&ProjectName, &Path)> = Vec::new();
        for (name, project) in &self.projects {
            for skill in project.skills.keys() {
                if self.global.skills.contains_key(skill) {
                    return Err(invalid(format!(
                        "Skill '{skill}' is pinned globally and in project '{name}'"
                    )));
                }
            }
            for instance in &project.instances {
                let path = &instance.path;
                if !path.is_absolute()
                    || path
                        .components()
                        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
                {
                    return Err(invalid(format!(
                        "Instance path must be absolute and canonical: {}",
                        path.display()
                    )));
                }
                // Path component iteration normalizes embedded '.' and repeated separators.
                if path.components().collect::<PathBuf>().as_os_str() != path.as_os_str() {
                    return Err(invalid(format!(
                        "Instance path must be canonical: {}",
                        path.display()
                    )));
                }
                match std::fs::canonicalize(path) {
                    Ok(canonical) => {
                        if canonical.as_os_str() != path.as_os_str() {
                            return Err(invalid(format!(
                                "Instance path is not canonical: {}; use {}",
                                path.display(),
                                canonical.display()
                            )));
                        }
                        if !std::fs::metadata(path)
                            .map_err(|error| {
                                io_error(format!("Inspect {}", path.display()), error)
                            })?
                            .is_dir()
                        {
                            return Err(invalid(format!(
                                "Instance is not a directory: {}",
                                path.display()
                            )));
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(io_error(
                            format!("Canonicalize instance {}", path.display()),
                            error,
                        ));
                    }
                }
                for (other_project, other) in &paths {
                    if path.starts_with(other) || other.starts_with(path) {
                        return Err(invalid(format!(
                            "Duplicate or nested instances: {} ({name}) and {} ({other_project})",
                            path.display(),
                            other.display()
                        )));
                    }
                }
                paths.push((name, path));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
