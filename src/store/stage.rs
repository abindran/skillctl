//! Transactional publication of validated skill versions.
//!
//! Callers validate the source first, keep it unchanged during staging, and hold
//! the exclusive registry lock for the whole operation. No existing version is
//! replaced. Every mutation uses `Fs`, including cleanup and dry-run plans.

use std::{
    io,
    path::{Path, PathBuf},
};

use super::{hash::ContentHasher, meta::Metadata};
use crate::{error::Error, paths::Paths, services::fs::Fs};

/// Copy a validated skill into a private staging directory, verify its hash,
/// write metadata, seal its contents, atomically rename it and seal the root.
/// The source (including a draft) is never modified or deleted.
///
/// `paths.store()` must already exist. Validation belongs to the caller; the
/// hash in `metadata` must describe the exact content being published. Dry runs
/// read the source and hash the planned bytes without needing staged paths to
/// exist. A returned error never publishes a partial version.
pub fn stage_version(
    fs: &dyn Fs,
    paths: &Paths,
    source: &Path,
    metadata: &Metadata,
) -> Result<PathBuf, Error> {
    let parent = paths.skill_versions(&metadata.name);
    let destination = paths.version_dir(&metadata.name, &metadata.version);
    // A dry run on a fresh home has only a planned store folder; create it through
    // the service so the plan stays complete and real runs never need it.
    match fs.symlink_metadata(&paths.store()) {
        Ok(_) => real_directory(fs, &paths.store())?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            operation(fs.create_dir(&paths.store()), "Create", &paths.store())?;
        }
        Err(error) => return Err(io_error("Inspect", &paths.store(), error)),
    }
    ensure_missing(fs, &destination)?;
    real_directory(fs, source)?;
    let mut directories = vec![PathBuf::new()];
    let mut files = Vec::new();
    collect(fs, source, source, &mut directories, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    match fs.symlink_metadata(&parent) {
        Ok(_) => real_directory(fs, &parent)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            operation(fs.create_dir(&parent), "Create", &parent)?;
        }
        Err(error) => return Err(io_error("Inspect", &parent, error)),
    }

    // tempfile generates collision-resistant names; the callback keeps directory
    // creation behind Fs, so dry runs do not create temporary directories either.
    let staging = tempfile::Builder::new()
        .prefix(".staging-")
        .make_in(&parent, |path| {
            fs.create_dir(path)?;
            Ok(path.to_path_buf())
        })
        .map_err(|error| io_error("Create staging directory in", &parent, error))?;
    // Disarm tempfile's file-oriented cleanup: rollback below owns this directory.
    let (_, temporary_path) = staging.into_parts();
    let staging = temporary_path
        .keep()
        .map_err(|error| io_error("Keep staging directory in", &parent, error.error))?;
    let mut created = vec![(staging.clone(), true)];
    let result = (|| {
        let skill = staging.join(metadata.name.as_str());
        for relative in directories {
            let path = skill.join(relative);
            operation(fs.create_dir(&path), "Create", &path)?;
            created.push((path, true));
        }
        let mut hash = ContentHasher::new();
        let mut modes = Vec::new();
        for (relative, executable) in files {
            let from = source.join(&relative);
            let info = operation(fs.symlink_metadata(&from), "Inspect", &from)?;
            if !info.is_file() || info.file_type().is_symlink() {
                return Err(invalid(&from, "expected a regular file"));
            }
            let bytes = operation(fs.read(&from), "Read", &from)?;
            hash.file(&relative, executable, &bytes);
            let to = skill.join(&relative);
            // Atomic writes can report a directory-sync error after publishing.
            // Track the path first so rollback also removes such committed files.
            created.push((to.clone(), false));
            operation(fs.write_atomic(&to, &bytes), "Write", &to)?;
            modes.push((to, if executable { 0o555 } else { 0o444 }));
        }
        if hash.finish() != metadata.content_hash {
            return Err(invalid(
                source,
                "content hash does not match the supplied metadata",
            ));
        }
        let meta = staging.join("meta.json");
        created.push((meta.clone(), false));
        metadata.save(fs, &meta)?;
        modes.push((meta, 0o444));
        for (path, mode) in modes {
            operation(fs.set_mode(&path, mode), "Seal", &path)?;
        }
        // Seal children first, but keep the root writable: macOS requires
        // write permission on a directory being renamed.
        for (path, directory) in created.iter().rev() {
            if *directory && path != &staging {
                operation(fs.set_mode(path, 0o555), "Seal", path)?;
            }
        }
        // Commands serialize publication with the registry lock. Recheck to
        // protect even an existing empty version from rename's replace behavior.
        ensure_missing(fs, &destination)?;
        operation(fs.rename(&staging, &destination), "Publish", &destination)?;
        // Rollback must use the published paths if sealing the root fails.
        for (path, _) in &mut created {
            *path = destination.join(path.strip_prefix(&staging).expect("staged path"));
        }
        operation(fs.set_mode(&destination, 0o555), "Seal", &destination)?;
        Ok(destination)
    })();
    match result {
        Ok(path) => Ok(path),
        Err(error) => {
            if let Err(cleanup) = cleanup(fs, &created) {
                return Err(Error::Io {
                    operation: format!(
                        "Clean staged version {} after: {error}",
                        created[0].0.display()
                    ),
                    source: io::Error::other(cleanup.to_string()),
                    hint: format!(
                        "Remove the abandoned version directory {} after restoring write permissions.",
                        created[0].0.display()
                    ),
                });
            }
            Err(error)
        }
    }
}

/// Restore 0755 throughout a version before GC removes it. Symlinks and special
/// files are refused before any permission changes, never followed. The caller
/// owns the version and holds the exclusive registry lock; this does not delete it.
pub fn restore_write_mode(fs: &dyn Fs, version: &Path) -> Result<(), Error> {
    real_directory(fs, version)?;
    let mut directories = vec![PathBuf::new()];
    let mut files = Vec::new();
    collect(fs, version, version, &mut directories, &mut files)?;
    for relative in directories {
        let path = version.join(relative);
        operation(fs.set_mode(&path, 0o755), "Restore write mode on", &path)?;
    }
    for (relative, _) in files {
        let path = version.join(relative);
        operation(fs.set_mode(&path, 0o755), "Restore write mode on", &path)?;
    }
    Ok(())
}

fn collect(
    fs: &dyn Fs,
    root: &Path,
    directory: &Path,
    directories: &mut Vec<PathBuf>,
    files: &mut Vec<(String, bool)>,
) -> Result<(), Error> {
    for path in operation(fs.read_dir(directory), "List", directory)? {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| invalid(&path, "path escapes the source"))?;
        let info = operation(fs.symlink_metadata(&path), "Inspect", &path)?;
        if info.file_type().is_symlink() {
            return Err(invalid(&path, "symlinks are not allowed"));
        }
        if info.is_dir() {
            directories.push(relative.into());
            collect(fs, root, &path, directories, files)?;
        } else if info.is_file() {
            let relative = relative
                .to_str()
                .ok_or_else(|| invalid(&path, "file paths must be UTF-8"))?;
            files.push((relative.into(), executable(&info)?));
        } else {
            return Err(invalid(
                &path,
                "only regular files and directories are allowed",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn executable(info: &std::fs::Metadata) -> Result<bool, Error> {
    use std::os::unix::fs::PermissionsExt;
    Ok(info.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_info: &std::fs::Metadata) -> Result<bool, Error> {
    Err(io_error(
        "Read Unix executable bits",
        Path::new("."),
        io::Error::new(io::ErrorKind::Unsupported, "staging requires Unix modes"),
    ))
}

fn cleanup(fs: &dyn Fs, created: &[(PathBuf, bool)]) -> Result<(), Error> {
    for (path, _) in created {
        ignore_missing(
            fs.set_mode(path, 0o755),
            "Restore staging permissions on",
            path,
        )?;
    }
    for (path, directory) in created.iter().rev() {
        ignore_missing(
            if *directory {
                fs.remove_dir(path)
            } else {
                fs.remove_file(path)
            },
            "Remove staging entry",
            path,
        )?;
    }
    Ok(())
}

fn ignore_missing(result: io::Result<()>, action: &str, path: &Path) -> Result<(), Error> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => operation(other, action, path),
    }
}

fn real_directory(fs: &dyn Fs, path: &Path) -> Result<(), Error> {
    let info = operation(fs.symlink_metadata(path), "Inspect", path)?;
    if !info.is_dir() || info.file_type().is_symlink() {
        return Err(invalid(path, "expected a real directory, not a symlink"));
    }
    Ok(())
}

fn ensure_missing(fs: &dyn Fs, path: &Path) -> Result<(), Error> {
    match fs.symlink_metadata(path) {
        Ok(_) => Err(io_error(
            "Publish",
            path,
            io::Error::new(io::ErrorKind::AlreadyExists, "version already exists"),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("Inspect", path, error)),
    }
}

fn operation<T>(result: io::Result<T>, action: &str, path: &Path) -> Result<T, Error> {
    result.map_err(|error| io_error(action, path, error))
}

fn invalid(path: &Path, reason: &str) -> Error {
    Error::Validation {
        summary: format!("Cannot stage {}: {reason}", path.display()),
        hint: "Validate the skill and recompute its content hash before staging; remove symlinks and special files.".into(),
    }
}

fn io_error(action: &str, path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("{action} {} for store staging", path.display()),
        source,
        hint:
            "Check store permissions and available space; existing versions cannot be overwritten."
                .into(),
    }
}
