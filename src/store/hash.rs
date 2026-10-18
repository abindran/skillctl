//! Canonical SHA-256 identity of a skill folder, not its enclosing version folder.

use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{error::Error, services::fs::Fs};

/// Hash all regular files in a skill, returning `sha256:<lowercase hex>`.
///
/// Files are sorted globally by their UTF-8 relative path, with `/` separators.
/// Each contributes: path, NUL, executable flag (`0` or `1`), NUL, decimal
/// byte length, NUL, and raw contents. Empty directories, timestamps and
/// non-executable permission bits do not contribute. Any Unix execute bit
/// makes a file executable, so sealing 0755 to 0555 preserves the hash.
///
/// Symlinks (including the root), special files and non-UTF-8 paths are rejected
/// rather than followed, skipped or encoded lossily. Reads use the filesystem
/// service, so a dry run computes the same hash without recording mutations.
/// Callers must keep the tree unchanged for the duration of hashing.
pub fn content_hash(fs: &dyn Fs, dir: &Path) -> Result<String, Error> {
    let root = fs
        .symlink_metadata(dir)
        .map_err(|source| io_error("Inspect", dir, source))?;
    if !root.is_dir() || root.file_type().is_symlink() {
        return Err(invalid(dir, "the skill root must be a real directory"));
    }

    let mut files = Vec::new();
    collect_files(fs, dir, dir, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut digest = ContentHasher::new();
    for (relative, path) in files {
        let metadata = fs
            .symlink_metadata(&path)
            .map_err(|source| io_error("Inspect", &path, source))?;
        // Recheck before reading: never deliberately follow a discovered symlink.
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid(
                &path,
                "expected a regular file, not a symlink or special file",
            ));
        }
        let executable = executable(&metadata)
            .map_err(|source| io_error("Read executable bit of", &path, source))?;
        let bytes = fs
            .read(&path)
            .map_err(|source| io_error("Read", &path, source))?;
        digest.file(&relative, executable, &bytes);
    }
    Ok(digest.finish())
}

/// Shared canonical encoding for hashing existing trees and the exact staged bytes.
pub(crate) struct ContentHasher(Sha256);

impl ContentHasher {
    pub(crate) fn new() -> Self {
        Self(Sha256::new())
    }

    /// Call in globally sorted relative-path order.
    pub(crate) fn file(&mut self, relative: &str, executable: bool, bytes: &[u8]) {
        self.0.update(relative.as_bytes());
        self.0.update(b"\0");
        self.0.update(if executable { b"1" } else { b"0" });
        self.0.update(b"\0");
        self.0.update(bytes.len().to_string().as_bytes());
        self.0.update(b"\0");
        self.0.update(bytes);
    }

    pub(crate) fn finish(self) -> String {
        format!("sha256:{}", hex::encode(self.0.finalize()))
    }
}

fn collect_files(
    fs: &dyn Fs,
    root: &Path,
    dir: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), Error> {
    for path in fs
        .read_dir(dir)
        .map_err(|source| io_error("List", dir, source))?
    {
        let metadata = fs
            .symlink_metadata(&path)
            .map_err(|source| io_error("Inspect", &path, source))?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(&path, "symlinks inside a skill are not allowed"));
        }
        if metadata.is_dir() {
            collect_files(fs, root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid(&path, "file path is outside the skill directory"))?;
            let components = relative
                .iter()
                .map(|part| {
                    part.to_str()
                        .ok_or_else(|| invalid(&path, "file paths must be valid UTF-8"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            files.push((components.join("/"), path));
        } else {
            return Err(invalid(
                &path,
                "only directories and regular files are allowed",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn executable(metadata: &std::fs::Metadata) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    Ok(metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_metadata: &std::fs::Metadata) -> io::Result<bool> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "content hashing requires Unix executable bits",
    ))
}

fn invalid(path: &Path, reason: &str) -> Error {
    Error::Validation {
        summary: format!("Cannot hash {}: {reason}", path.display()),
        hint: "Use a skill folder containing only regular files and directories with UTF-8 paths."
            .into(),
    }
}

fn io_error(operation: &str, path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("{operation} {} for content hash", path.display()),
        source,
        hint: "Check that the skill folder and its files exist and are readable.".into(),
    }
}
