//! Stored skill versions and their content identity.
//!
//! The directory tree is the index; queries do not need to parse `meta.json` or
//! `SKILL.md`. Callers hold the registry lock for a consistent view of the store.

use std::{
    io,
    path::{Path, PathBuf},
};

use semver::Version;

use crate::{error::Error, names::SkillName, paths::Paths, registry::Pin, services::fs::Fs};

pub mod hash;
pub mod meta;
pub mod stage;

/// List valid skill directories in name order, including those with no versions.
/// Missing stores are empty. Files, symlinks and invalid names are ignored.
pub fn list_skills(fs: &dyn Fs, paths: &Paths) -> Result<Vec<SkillName>, Error> {
    let root = paths.store();
    if !directory_exists(fs, &root)? {
        return Ok(Vec::new());
    }
    let mut skills = Vec::new();
    for entry in entries(fs, &root)? {
        let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(name) = name.parse::<SkillName>() else {
            continue;
        };
        if is_directory(fs, &entry)? {
            skills.push(name);
        }
    }
    skills.sort();
    Ok(skills)
}

/// List version directories in ascending semver order, including prereleases and
/// build metadata. Staging directories, invalid versions, files and symlinks are
/// ignored. A missing skill has no versions.
pub fn list_versions(fs: &dyn Fs, paths: &Paths, name: &SkillName) -> Result<Vec<Version>, Error> {
    if !directory_exists(fs, &paths.store())? || !directory_exists(fs, &paths.skill_versions(name))?
    {
        return Ok(Vec::new());
    }
    let root = paths.skill_versions(name);
    let mut versions = Vec::new();
    for entry in entries(fs, &root)? {
        let Some(text) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(version) = Version::parse(text) else {
            continue;
        };
        if is_directory(fs, &entry)? {
            versions.push(version);
        }
    }
    // Version's total ordering breaks equal-precedence ties by build metadata.
    versions.sort();
    Ok(versions)
}

/// Highest installed stable release. Never fall back to a prerelease; callers
/// may select one explicitly. Build metadata does not make a release unstable.
pub fn latest_stable(fs: &dyn Fs, paths: &Paths, name: &SkillName) -> Result<Version, Error> {
    list_versions(fs, paths, name)?
        .into_iter()
        .rev()
        .find(|version| version.pre.is_empty())
        .ok_or_else(|| unavailable(format!("No stable version of {name} is installed")))
}

/// Compute the link target for a pin without accessing the filesystem. This also
/// works for missing pins so reconciliation can inspect and repair dangling links.
pub fn store_dir(paths: &Paths, name: &SkillName, pin: &Pin) -> PathBuf {
    match pin {
        Pin::Version(version) => paths.store_dir(name, version),
        Pin::Draft => paths.draft_dir(name),
    }
}

/// Resolve an exact version, draft, or (when omitted) the highest stable version.
/// Explicit pins never fall back to another version. The target must be a real
/// directory; parsing, validation and integrity checks belong to their own modules.
pub fn resolve_pin(
    fs: &dyn Fs,
    paths: &Paths,
    name: &SkillName,
    pin: Option<&Pin>,
) -> Result<Pin, Error> {
    let pin = match pin {
        Some(pin) => pin.clone(),
        None => Pin::Version(latest_stable(fs, paths, name)?),
    };
    let parents = match &pin {
        Pin::Version(version) => vec![
            paths.store(),
            paths.skill_versions(name),
            paths.version_dir(name, version),
        ],
        Pin::Draft => vec![paths.drafts()],
    };
    let target = store_dir(paths, name, &pin);
    for path in parents.iter().chain(std::iter::once(&target)) {
        if !directory_exists(fs, path)? {
            return Err(unavailable(format!(
                "Skill {name}@{pin} is missing at {}",
                path.display()
            )));
        }
    }
    Ok(pin)
}

fn entries(fs: &dyn Fs, path: &Path) -> Result<Vec<PathBuf>, Error> {
    fs.read_dir(path)
        .map_err(|source| io_error(format!("List {}", path.display()), source))
}

fn is_directory(fs: &dyn Fs, path: &Path) -> Result<bool, Error> {
    fs.symlink_metadata(path)
        .map(|metadata| metadata.is_dir())
        .map_err(|source| io_error(format!("Inspect {}", path.display()), source))
}

/// Unlike listing entries, a specifically requested path must not be a symlink
/// or another file type. Check each managed ancestor before traversing it.
fn directory_exists(fs: &dyn Fs, path: &Path) -> Result<bool, Error> {
    match fs.symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(io_error(
            format!("Inspect {}", path.display()),
            io::Error::new(
                io::ErrorKind::InvalidData,
                "expected a real directory, not a symlink or file",
            ),
        )),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(io_error(format!("Inspect {}", path.display()), source)),
    }
}

fn unavailable(message: String) -> Error {
    Error::Registry {
        message,
        hint: "Run skillctl install <source> first, select an installed version, or use skillctl new <skill> for a draft.".into(),
    }
}

fn io_error(operation: String, source: io::Error) -> Error {
    Error::Io {
        operation,
        source,
        hint: "Check the store layout and filesystem permissions; run skillctl doctor to investigate damaged versions.".into(),
    }
}
