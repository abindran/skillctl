//! Validated registry paths and side-effect-free skillctl home/path resolution.
//!
//! Path construction does not create folders or require the store to exist.
//! Canonicalization and identity queries deliberately require existing paths.
use std::{
    env, fmt, fs,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{error::Error, names::SkillName};

fn invalid_path(message: impl Into<String>) -> Error {
    Error::Usage {
        message: message.into(),
        hint: "Use a non-empty relative path without '..'; home targets must start with '~/' or be absolute.".to_owned(),
    }
}

fn path_io(operation: String, source: std::io::Error) -> Error {
    Error::Io {
        operation,
        source,
        hint: "Check that the path exists and that you have permission to access it.".to_owned(),
    }
}

/// A non-empty relative path with no parent traversal or filesystem root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "PathBuf", into = "PathBuf")]
pub struct RelPath(PathBuf);

impl RelPath {
    pub fn new(value: impl Into<PathBuf>) -> Result<Self, Error> {
        let value = value.into();
        let mut has_normal = false;
        for component in value.components() {
            match component {
                Component::Normal(_) => has_normal = true,
                Component::CurDir => {}
                _ => {
                    return Err(invalid_path(format!(
                        "Invalid relative path: {}",
                        value.display()
                    )));
                }
            }
        }
        if !has_normal {
            return Err(invalid_path("A relative path must name a file or folder"));
        }
        Ok(Self(value))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl TryFrom<PathBuf> for RelPath {
    type Error = Error;
    fn try_from(value: PathBuf) -> Result<Self, Error> {
        Self::new(value)
    }
}
impl From<RelPath> for PathBuf {
    fn from(value: RelPath) -> Self {
        value.into_path_buf()
    }
}
impl FromStr for RelPath {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        Self::new(value)
    }
}
impl AsRef<Path> for RelPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}
impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(f)
    }
}

/// An absolute user-level target or a `~/...` path, expanded only when used.
/// Named-user expansion (`~alice`) and parent traversal are not supported.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "PathBuf", into = "PathBuf")]
pub struct HomePath(PathBuf);

impl HomePath {
    pub fn new(value: impl Into<PathBuf>) -> Result<Self, Error> {
        let value = value.into();
        if value.is_absolute() {
            if value
                .components()
                .any(|part| matches!(part, Component::ParentDir))
            {
                return Err(invalid_path("Home targets cannot contain '..'"));
            }
        } else {
            let tail = value
                .strip_prefix("~")
                .map_err(|_| invalid_path("Home targets must be absolute or start with '~/'."))?;
            RelPath::new(tail)?;
        }
        Ok(Self(value))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Expand against an explicit user home, avoiding process environment mutation in tests.
    pub fn expand_with(&self, user_home: &Path) -> Result<PathBuf, Error> {
        if self.0.is_absolute() {
            return Ok(self.0.clone());
        }
        if !user_home.is_absolute() {
            return Err(invalid_path("The user home must be absolute"));
        }
        Ok(user_home.join(self.0.strip_prefix("~").expect("validated tilde prefix")))
    }

    pub fn expand(&self) -> Result<PathBuf, Error> {
        if self.0.is_absolute() {
            return Ok(self.0.clone());
        }
        self.expand_with(&user_home()?)
    }
}
impl TryFrom<PathBuf> for HomePath {
    type Error = Error;
    fn try_from(value: PathBuf) -> Result<Self, Error> {
        Self::new(value)
    }
}
impl From<HomePath> for PathBuf {
    fn from(value: HomePath) -> Self {
        value.0
    }
}
impl FromStr for HomePath {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        Self::new(value)
    }
}
impl AsRef<Path> for HomePath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}
impl fmt::Display for HomePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(f)
    }
}

fn user_home() -> Result<PathBuf, Error> {
    dirs::home_dir()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            invalid_path("Cannot determine the user home; pass --home or set SKILLCTL_HOME")
        })
}

/// Paths under the selected skillctl home, which need not exist yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    home: PathBuf,
}

impl Paths {
    /// Priority: explicit `--home`, then `SKILLCTL_HOME`, then `~/.skills`.
    /// Relative overrides resolve against the current working directory.
    pub fn resolve(explicit: Option<&Path>) -> Result<Self, Error> {
        let environment = env::var_os("SKILLCTL_HOME").map(PathBuf::from);
        let selected = explicit.or(environment.as_deref());
        let needs_home = selected.is_none_or(|path| path.starts_with("~"));
        let home = if needs_home { Some(user_home()?) } else { None };
        let cwd = if selected.is_some_and(|path| !path.is_absolute() && !path.starts_with("~")) {
            env::current_dir()
                .map_err(|source| path_io("Read current directory".to_owned(), source))?
        } else {
            PathBuf::new()
        };
        Self::resolve_from(explicit, environment.as_deref(), home.as_deref(), &cwd)
    }

    fn resolve_from(
        explicit: Option<&Path>,
        environment: Option<&Path>,
        user: Option<&Path>,
        cwd: &Path,
    ) -> Result<Self, Error> {
        let home = match explicit.or(environment) {
            Some(path) if path.as_os_str().is_empty() => {
                return Err(invalid_path("The skillctl home cannot be empty"));
            }
            Some(path) if path.starts_with("~") => {
                let user =
                    user.ok_or_else(|| invalid_path("Cannot expand '~' without a user home"))?;
                user.join(path.strip_prefix("~").expect("checked tilde prefix"))
            }
            Some(path) if path.is_absolute() => path.to_path_buf(),
            Some(path) => cwd.join(path),
            None => user
                .ok_or_else(|| {
                    invalid_path("Cannot determine the user home; pass --home or set SKILLCTL_HOME")
                })?
                .join(".skills"),
        };
        if !home.is_absolute() {
            return Err(invalid_path("The resolved skillctl home must be absolute"));
        }
        Ok(Self { home })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }
    pub fn registry(&self) -> PathBuf {
        self.home.join("registry.yml")
    }
    pub fn registry_lock(&self) -> PathBuf {
        self.home.join("registry.lock")
    }
    pub fn store(&self) -> PathBuf {
        self.home.join("store")
    }
    pub fn drafts(&self) -> PathBuf {
        self.home.join("drafts")
    }
    pub fn templates(&self) -> PathBuf {
        self.home.join("templates")
    }
    pub fn skill_versions(&self, name: &SkillName) -> PathBuf {
        self.store().join(name.as_str())
    }
    pub fn version_dir(&self, name: &SkillName, version: &Version) -> PathBuf {
        self.skill_versions(name).join(version.to_string())
    }
    /// The skill itself is nested beneath its version so its folder name matches its name.
    pub fn store_dir(&self, name: &SkillName, version: &Version) -> PathBuf {
        self.version_dir(name, version).join(name.as_str())
    }
    pub fn draft_dir(&self, name: &SkillName) -> PathBuf {
        self.drafts().join(name.as_str())
    }
}

/// Resolve symlinks and return an absolute path; missing paths are errors.
pub fn canonicalize(path: &Path) -> Result<PathBuf, Error> {
    fs::canonicalize(path)
        .map_err(|source| path_io(format!("Canonicalize {}", path.display()), source))
}

/// Filesystem identity on Unix (macOS and Linux), not a textual path comparison.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
}

#[cfg(unix)]
impl FileIdentity {
    /// Follow symlinks so aliases of the same object have the same identity.
    pub fn from_path(path: &Path) -> Result<Self, Error> {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path)
            .map_err(|source| path_io(format!("Read identity of {}", path.display()), source))?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

#[cfg(unix)]
pub fn same_file(left: &Path, right: &Path) -> Result<bool, Error> {
    Ok(FileIdentity::from_path(left)? == FileIdentity::from_path(right)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_tables_and_serde() {
        for value in [".agents/skills", "scripts", "a/b", "./a", "a/./b", "a b/é"] {
            let path = RelPath::new(value).unwrap();
            let json = serde_json::to_string(&path).unwrap();
            assert_eq!(serde_json::from_str::<RelPath>(&json).unwrap(), path);
        }
        for value in [
            "",
            ".",
            "./",
            "/",
            "/skills",
            "..",
            "../skills",
            "a/../b",
            "a/..",
        ] {
            assert!(RelPath::new(value).is_err(), "accepted {value:?}");
            let json = serde_json::to_string(value).unwrap();
            assert!(serde_json::from_str::<RelPath>(&json).is_err());
        }
    }

    #[test]
    fn home_paths_expand_without_changing_serialized_form() {
        let sandbox = tempfile::tempdir().unwrap();
        let path = HomePath::new("~/.claude/skills").unwrap();
        assert_eq!(
            path.expand_with(sandbox.path()).unwrap(),
            sandbox.path().join(".claude/skills")
        );
        assert_eq!(
            serde_json::to_string(&path).unwrap(),
            "\"~/.claude/skills\""
        );
        assert_eq!(
            serde_json::from_str::<HomePath>("\"~/.claude/skills\"").unwrap(),
            path
        );
        let absolute = HomePath::new(sandbox.path().join("skills")).unwrap();
        assert_eq!(
            absolute.expand_with(Path::new("relative")).unwrap(),
            absolute.as_path()
        );
        assert!(path.expand_with(Path::new("relative")).is_err());
        for value in [
            "",
            "~",
            "~alice/skills",
            "skills",
            "~/../skills",
            "~/a/../b",
            "/a/../b",
        ] {
            assert!(HomePath::new(value).is_err(), "accepted {value:?}");
            assert!(
                serde_json::from_str::<HomePath>(&serde_json::to_string(value).unwrap()).is_err()
            );
        }
    }

    #[test]
    fn home_resolution_precedence_relative_and_tilde_overrides() {
        let sandbox = tempfile::tempdir().unwrap();
        let user = sandbox.path().join("user");
        let cwd = sandbox.path().join("work");
        let explicit = sandbox.path().join("explicit");
        let environment = sandbox.path().join("environment");
        let resolve = |a, b| Paths::resolve_from(a, b, Some(&user), &cwd).unwrap();
        assert_eq!(
            resolve(Some(&explicit), Some(&environment)).home(),
            explicit
        );
        assert_eq!(resolve(None, Some(&environment)).home(), environment);
        assert_eq!(resolve(None, None).home(), user.join(".skills"));
        assert_eq!(
            resolve(Some(Path::new("local")), None).home(),
            cwd.join("local")
        );
        assert_eq!(
            resolve(None, Some(Path::new("~/.custom"))).home(),
            user.join(".custom")
        );
        assert!(Paths::resolve_from(None, None, None, &cwd).is_err());
        assert!(Paths::resolve_from(Some(Path::new("")), None, Some(&user), &cwd).is_err());
        // An absolute override works even when no user home or working directory is available.
        assert!(Paths::resolve_from(Some(&explicit), None, None, Path::new("")).is_ok());
        assert!(!explicit.exists(), "resolution must not create the home");
    }

    #[test]
    fn store_layout_preserves_name_under_version() {
        let sandbox = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(Some(sandbox.path())).unwrap();
        let name = SkillName::new("org-review").unwrap();
        let version = Version::parse("1.4.0-beta.1+build.2").unwrap();
        assert_eq!(paths.registry(), sandbox.path().join("registry.yml"));
        assert_eq!(paths.registry_lock(), sandbox.path().join("registry.lock"));
        assert_eq!(paths.templates(), sandbox.path().join("templates"));
        assert_eq!(
            paths.store_dir(&name, &version),
            sandbox
                .path()
                .join("store/org-review/1.4.0-beta.1+build.2/org-review")
        );
        assert_eq!(
            paths.draft_dir(&name),
            sandbox.path().join("drafts/org-review")
        );
        assert!(!paths.store().exists());
    }

    #[test]
    fn canonicalization_reports_missing_path_with_io_source() {
        let sandbox = tempfile::tempdir().unwrap();
        assert!(canonicalize(sandbox.path()).unwrap().is_absolute());
        let error = canonicalize(&sandbox.path().join("missing")).unwrap_err();
        assert!(matches!(error, Error::Io { .. }));
        assert!(std::error::Error::source(&error).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn identity_follows_symlinks_and_distinguishes_other_objects() {
        use std::os::unix::fs::symlink;
        let sandbox = tempfile::tempdir().unwrap();
        let real = sandbox.path().join("real");
        let alias = sandbox.path().join("alias");
        let other = sandbox.path().join("other");
        fs::create_dir(&real).unwrap();
        fs::create_dir(&other).unwrap();
        symlink(&real, &alias).unwrap();
        assert!(same_file(&real, &alias).unwrap());
        assert!(!same_file(&real, &other).unwrap());
        assert_eq!(canonicalize(&real).unwrap(), canonicalize(&alias).unwrap());
        assert!(FileIdentity::from_path(&sandbox.path().join("missing")).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_tmp_alias_has_same_identity() {
        assert!(same_file(Path::new("/tmp"), Path::new("/private/tmp")).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn differently_cased_paths_match_when_filesystem_is_case_insensitive() {
        let sandbox = tempfile::tempdir().unwrap();
        let original = sandbox.path().join("MixedCase");
        let alternate = sandbox.path().join("mixedcase");
        fs::create_dir(&original).unwrap();
        if !alternate.exists() {
            eprintln!("Case-insensitive identity check skipped: filesystem is case-sensitive");
            return;
        }
        assert!(same_file(&original, &alternate).unwrap());
    }
}
