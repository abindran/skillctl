//! Temporary shallow checkouts for `install`.

use std::path::{Path, PathBuf};

use crate::{error::Error, services::git::Git};

/// A shallow clone that is deleted when dropped.
pub struct Checkout {
    temp: tempfile::TempDir,
    pub commit: String,
}

impl Checkout {
    /// Clone `url` (optionally at `reference`) into a fresh temporary folder.
    /// The clone always touches the real filesystem, even in a dry run, because
    /// discovery and validation need the files; nothing under the home changes.
    pub fn new(git: &dyn Git, url: &str, reference: Option<&str>) -> Result<Self, Error> {
        let temp = tempfile::Builder::new()
            .prefix("skillctl-install-")
            .tempdir()
            .map_err(|source| Error::Io {
                operation: "Create temporary checkout folder".into(),
                source,
                hint: "Check the temporary directory and free disk space.".into(),
            })?;
        let into = temp.path().join("checkout");
        let commit = git.shallow_clone(url, reference, &into)?;
        Ok(Self { temp, commit })
    }

    pub fn root(&self) -> PathBuf {
        self.temp.path().join("checkout")
    }
}

impl AsRef<Path> for Checkout {
    fn as_ref(&self) -> &Path {
        self.temp.path()
    }
}
