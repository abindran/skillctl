//! Where installable artifacts come from: a Git URL cloned into a temporary
//! checkout, or a local folder. Discovery finds skill folders (`SKILL.md`) and
//! hook folders (`HOOK.md`); it is read-only and never follows symlinks.

pub mod git;

use std::{
    io,
    path::{Path, PathBuf},
};

use crate::{artifact::Kind, error::Error, names::SkillName, services::fs::Fs};

/// An install source as the user typed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Anything that is not an existing local folder: `git@…`, `https://…`,
    /// `ssh://…`, `file://…` or a bare path to a repository elsewhere.
    Git { url: String },
    /// An existing folder on this machine (canonical path).
    Local(PathBuf),
}

impl Source {
    /// Existing folders are local sources; everything else is handed to Git.
    pub fn classify(fs: &dyn Fs, value: &str) -> Result<Self, Error> {
        let path = Path::new(value);
        match fs.symlink_metadata(path) {
            Ok(info) if info.is_dir() || info.file_type().is_symlink() => {
                Ok(Self::Local(crate::paths::canonicalize(path)?))
            }
            Ok(_) => Err(Error::Usage {
                message: format!("{value} is not a folder or a Git URL"),
                hint: "Pass a folder containing SKILL.md or HOOK.md (or several such folders), or a Git clone URL.".into(),
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::Git {
                url: value.to_owned(),
            }),
            Err(source) => Err(Error::Io {
                operation: format!("Inspect {value}"),
                source,
                hint: "Check the source path and its permissions.".into(),
            }),
        }
    }
}

/// A skill or hook folder found in a checkout or local source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    /// Absolute folder containing the root document.
    pub dir: PathBuf,
    /// Which document was found.
    pub kind: Kind,
    /// Path relative to the source root, `None` when the root itself is the skill.
    pub subdir: Option<String>,
}

impl Discovered {
    /// The folder name, which the spec requires to equal the skill `name`.
    pub fn name(&self) -> Result<SkillName, Error> {
        self.dir
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid(&self.dir, "folder name is not valid UTF-8"))?
            .parse()
            .map_err(|_| {
                invalid(
                    &self.dir,
                    "folder name must be a valid artifact name (lowercase letters, digits, single hyphens)",
                )
            })
    }
}

/// Find artifact folders: every folder containing `SKILL.md` or `HOOK.md`, or
/// only `subdir` when given. A folder holding a root document is not searched
/// further; `.git` and other dot-folders are skipped; symlinks are never
/// followed. Results are sorted.
pub fn discover(fs: &dyn Fs, root: &Path, subdir: Option<&Path>) -> Result<Vec<Discovered>, Error> {
    let start = match subdir {
        Some(subdir) => {
            if subdir.is_absolute()
                || subdir
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err(Error::Usage {
                    message: format!("--path must be a relative folder inside the source, not {}", subdir.display()),
                    hint: "Pass the skill folder relative to the repository root, for example skills/org-code-review.".into(),
                });
            }
            root.join(subdir)
        }
        None => root.to_path_buf(),
    };
    let info = fs.symlink_metadata(&start).map_err(|source| Error::Io {
        operation: format!("Inspect {}", start.display()),
        source,
        hint: "Check that the source folder (or --path) exists.".into(),
    })?;
    if !info.is_dir() {
        return Err(invalid(&start, "expected a folder"));
    }
    let mut found = Vec::new();
    walk(fs, root, &start, &mut found)?;
    found.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(found)
}

fn walk(fs: &dyn Fs, root: &Path, dir: &Path, found: &mut Vec<Discovered>) -> Result<(), Error> {
    let entries = fs.read_dir(dir).map_err(|source| Error::Io {
        operation: format!("List {}", dir.display()),
        source,
        hint: "Check the source folder permissions.".into(),
    })?;
    if let Some(kind) = Kind::detect(fs, dir)? {
        let subdir = dir
            .strip_prefix(root)
            .ok()
            .filter(|relative| !relative.as_os_str().is_empty())
            .map(|relative| relative.to_string_lossy().replace('\\', "/"));
        found.push(Discovered {
            dir: dir.to_path_buf(),
            kind,
            subdir,
        });
        return Ok(());
    }
    for entry in entries {
        let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let info = fs.symlink_metadata(&entry).map_err(|source| Error::Io {
            operation: format!("Inspect {}", entry.display()),
            source,
            hint: "Check the source folder permissions.".into(),
        })?;
        if info.is_dir() && !info.file_type().is_symlink() {
            walk(fs, root, &entry, found)?;
        }
    }
    Ok(())
}

fn invalid(path: &Path, reason: &str) -> Error {
    Error::Validation {
        summary: format!("Cannot install from {}: {reason}", path.display()),
        hint: "Point skillctl install at a folder whose name is the artifact name and contains SKILL.md or HOOK.md."
            .into(),
    }
}
