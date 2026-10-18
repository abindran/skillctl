//! Writable authoring and validated publication. Callers hold the exclusive registry lock.
use super::{
    hash::content_hash,
    meta::{Metadata, Source},
    stage::stage_version,
};
use crate::{
    cli::Bump,
    commands::init::DEFAULT_TEMPLATE,
    error::Error,
    names::SkillName,
    paths::Paths,
    services::fs::Fs,
    skill::{Skill, frontmatter},
    validate,
};
use semver::Version;
use serde_yaml_ng::Value;
use std::{
    io,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Scaffold a valid draft. Existing drafts are never overwritten, even if empty.
pub fn new(
    fs: &dyn Fs,
    paths: &Paths,
    name: &SkillName,
    template: Option<&str>,
    dry_run: bool,
) -> Result<PathBuf, Error> {
    let template = template.unwrap_or("default");
    // Template names are single safe components, not arbitrary filesystem paths.
    SkillName::new(template).map_err(|_| {
        invalid("Invalid template name; use lowercase letters, digits and single hyphens")
    })?;
    let template_dir = paths.templates().join(template);
    for dir in [paths.templates(), template_dir.clone()] {
        real_dir(fs, &dir, dry_run)?;
    }
    let path = template_dir.join("SKILL.md");
    let document = match fs.symlink_metadata(&path) {
        Ok(info) if info.is_file() => String::from_utf8(op(fs.read(&path), &path)?)
            .map_err(|_| invalid("Template is not valid UTF-8"))?,
        Err(error)
            if error.kind() == io::ErrorKind::NotFound && dry_run && template == "default" =>
        {
            DEFAULT_TEMPLATE.into()
        }
        Err(error) => return Err(io_error(&path, error)),
        Ok(_) => {
            return Err(invalid(
                "Template SKILL.md must be a regular file, not a symlink",
            ));
        }
    };
    let document = document.replace("{{name}}", name.as_str());
    let document = rewrite(&document, Some(name), Some(&Version::new(0, 1, 0)), true)?;
    let destination = paths.draft_dir(name);
    let parsed = Skill::parse(&destination, &document)?;
    let findings = validate::Rule::check(&validate::spec::SpecRules, &parsed);
    require_valid(&findings)?;
    real_dir(fs, &paths.drafts(), dry_run)?;
    missing(fs, &destination)?;
    let mut created = Vec::new();
    let result = (|| {
        op(fs.create_dir(&destination), &destination)?;
        created.push((destination.clone(), true));
        for child in ["scripts", "references"] {
            let path = destination.join(child);
            op(fs.create_dir(&path), &path)?;
            created.push((path, true));
        }
        let path = destination.join("SKILL.md");
        created.push((path.clone(), false));
        op(fs.write_atomic(&path, document.as_bytes()), &path)?;
        op(fs.set_mode(&path, 0o644), &path)?;
        Ok(destination.clone())
    })();
    if result.is_err() {
        for (path, directory) in created.iter().rev() {
            let cleanup = if *directory {
                fs.remove_dir(path)
            } else {
                fs.remove_file(path)
            };
            if let Err(source) = cleanup {
                if source.kind() != io::ErrorKind::NotFound {
                    return Err(io_error(path, source));
                }
            }
        }
    }
    result
}

/// Seal using a read overlay for a requested bump: failed publication never edits
/// the draft. Only after staging succeeds is the original draft removed.
pub fn seal(
    fs: &dyn Fs,
    paths: &Paths,
    name: &SkillName,
    bump: Option<Bump>,
) -> Result<PathBuf, Error> {
    seal_with_allow(fs, paths, name, bump, &[])
}

/// Seal with the registry's global org/harness suppression list.
pub fn seal_with_allow(
    fs: &dyn Fs,
    paths: &Paths,
    name: &SkillName,
    bump: Option<Bump>,
    allow: &[String],
) -> Result<PathBuf, Error> {
    real_dir(fs, &paths.drafts(), false)?;
    let draft = paths.draft_dir(name);
    real_dir(fs, &draft, false)?;
    let findings = validate::run_all(fs, &draft, allow, false)?;
    require_valid(&findings)?;
    let skill = Skill::load(fs, &draft)?;
    let mut version = skill.version()?.ok_or_else(|| Error::Validation {
        summary: "metadata.version is required to seal a draft".into(),
        hint: "Set metadata.version to a semver string in the draft's SKILL.md.".into(),
    })?;
    let path = draft.join("SKILL.md");
    let document = if let Some(bump) = bump {
        version = bumped(version, bump)?;
        let bytes = op(fs.read(&path), &path)?;
        let text =
            std::str::from_utf8(&bytes).map_err(|_| invalid("SKILL.md is not valid UTF-8"))?;
        Some(rewrite(text, None, Some(&version), false)?.into_bytes())
    } else {
        None
    };
    let overlay = DocumentFs { fs, path, document };
    let metadata = Metadata {
        name: name.clone(),
        version,
        source: Source::Local,
        content_hash: content_hash(&overlay, &draft)?,
        installed_at: SystemTime::now().into(),
        skillctl_version: Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|error| invalid(error.to_string()))?,
    };
    let destination = stage_version(&overlay, paths, &draft, &metadata)?;
    remove_tree(fs, &draft).map_err(|error| Error::Io {
        operation: format!(
            "Version was published at {}, but draft cleanup failed: {error}",
            destination.display()
        ),
        source: io::Error::other(error.to_string()),
        hint: format!(
            "The sealed version is safe. Remove the remaining draft at {} before creating another.",
            draft.display()
        ),
    })?;
    Ok(destination)
}

fn require_valid(findings: &[validate::Finding]) -> Result<(), Error> {
    if findings
        .iter()
        .any(|finding| finding.severity == validate::Severity::Error)
    {
        return Err(Error::Validation {
            summary: validate::report::text(findings),
            hint: "Fix the reported SKILL.md fields before sealing or scaffolding.".into(),
        });
    }
    Ok(())
}

fn bumped(mut version: Version, bump: Bump) -> Result<Version, Error> {
    let increment = |number: u64| {
        number
            .checked_add(1)
            .ok_or_else(|| invalid("Version component overflow"))
    };
    match bump {
        Bump::Major => {
            version.major = increment(version.major)?;
            version.minor = 0;
            version.patch = 0;
        }
        Bump::Minor => {
            version.minor = increment(version.minor)?;
            version.patch = 0;
        }
        Bump::Patch => version.patch = increment(version.patch)?,
    }
    version.pre = semver::Prerelease::EMPTY;
    version.build = semver::BuildMetadata::EMPTY;
    Ok(version)
}

fn rewrite(
    document: &str,
    name: Option<&SkillName>,
    version: Option<&Version>,
    scaffold: bool,
) -> Result<String, Error> {
    let (yaml, body) = frontmatter::split(document)?;
    let mut value: Value =
        serde_yaml_ng::from_str(yaml).map_err(|error| invalid(error.to_string()))?;
    let map = value
        .as_mapping_mut()
        .ok_or_else(|| invalid("Template frontmatter must be a mapping"))?;
    if let Some(name) = name {
        map.insert(Value::from("name"), Value::from(name.as_str()));
    }
    if scaffold
        && map
            .get(Value::from("description"))
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
    {
        map.insert(
            Value::from("description"),
            Value::from(
                "Describe what this skill does and use when its specialized guidance is needed.",
            ),
        );
    }
    if let Some(version) = version {
        let metadata = map
            .entry(Value::from("metadata"))
            .or_insert_with(|| Value::Mapping(Default::default()));
        let metadata = metadata
            .as_mapping_mut()
            .ok_or_else(|| invalid("metadata must be a mapping"))?;
        metadata.insert(Value::from("version"), Value::from(version.to_string()));
    }
    let yaml = serde_yaml_ng::to_string(&value).map_err(|error| invalid(error.to_string()))?;
    Ok(format!("---\n{yaml}---\n{body}"))
}

fn remove_tree(fs: &dyn Fs, dir: &Path) -> Result<(), Error> {
    for path in op(fs.read_dir(dir), dir)? {
        let info = op(fs.symlink_metadata(&path), &path)?;
        if info.is_dir() {
            remove_tree(fs, &path)?;
        } else {
            op(fs.remove_file(&path), &path)?;
        }
    }
    op(fs.remove_dir(dir), dir)
}

fn real_dir(fs: &dyn Fs, path: &Path, allow_missing: bool) -> Result<(), Error> {
    match fs.symlink_metadata(path) {
        Ok(info) if info.is_dir() => Ok(()),
        Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path, error)),
        Ok(_) => Err(invalid(format!(
            "{} must be a real directory",
            path.display()
        ))),
    }
}
fn missing(fs: &dyn Fs, path: &Path) -> Result<(), Error> {
    match fs.symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path, error)),
        Ok(_) => Err(invalid(format!(
            "{} already exists; existing drafts are never overwritten",
            path.display()
        ))),
    }
}
fn op<T>(result: io::Result<T>, path: &Path) -> Result<T, Error> {
    result.map_err(|source| io_error(path, source))
}
fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Update draft {}", path.display()),
        source,
        hint: "Check the draft path and filesystem permissions.".into(),
    }
}
fn invalid(message: impl Into<String>) -> Error {
    Error::Usage { message: message.into(), hint: "Check the template, draft and version; existing drafts and store versions are never overwritten.".into() }
}

struct DocumentFs<'a> {
    fs: &'a dyn Fs,
    path: PathBuf,
    document: Option<Vec<u8>>,
}
impl Fs for DocumentFs<'_> {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        if path == self.path {
            if let Some(bytes) = &self.document {
                return Ok(bytes.clone());
            }
        }
        self.fs.read(path)
    }
    fn symlink_metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        self.fs.symlink_metadata(path)
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.fs.read_link(path)
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.fs.read_dir(path)
    }
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        self.fs.create_dir(path)
    }
    fn symlink(&self, target: &Path, link: &Path) -> io::Result<()> {
        self.fs.symlink(target, link)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.fs.rename(from, to)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.fs.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        self.fs.remove_dir(path)
    }
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.fs.write_atomic(path, bytes)
    }
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        self.fs.set_mode(path, mode)
    }
}
