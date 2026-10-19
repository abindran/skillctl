//! Install metadata stored beside, never inside, a version's skill folder.
//!
//! Callers supply the timestamp and content hash and hold the appropriate registry
//! lock. Saving is atomic; staging and making versions immutable are separate steps.

use std::{io, path::Path};

use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{artifact::Kind, error::Error, names::SkillName, services::fs::Fs};

/// Provenance of a stored version. Git references and subdirectories are optional
/// for installs using the default branch or a skill at the repository root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    Git {
        url: String,
        #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
        commit: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subdir: Option<String>,
    },
    /// Installed from a local folder or sealed from a draft.
    Local,
    /// Created from a scaffold.
    New,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    pub name: SkillName,
    pub version: Version,
    /// Absent in versions stored before hooks existed, which are all skills.
    #[serde(default)]
    pub kind: Kind,
    pub source: Source,
    pub content_hash: String,
    pub installed_at: DateTime<Utc>,
    pub skillctl_version: Version,
}

impl Metadata {
    /// Parse metadata without accessing the filesystem.
    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        serde_json::from_slice(bytes).map_err(|source| {
            io_error(
                "Parse store metadata".into(),
                io::Error::new(io::ErrorKind::InvalidData, source),
            )
        })
    }

    /// Human-readable JSON with a final newline for tools and diffs.
    pub fn to_json(&self) -> Result<String, Error> {
        let mut json = serde_json::to_string_pretty(self).map_err(|source| {
            io_error(
                "Serialize store metadata".into(),
                io::Error::new(io::ErrorKind::InvalidData, source),
            )
        })?;
        json.push('\n');
        Ok(json)
    }

    /// Read a full `meta.json` path. Missing or malformed files are errors.
    pub fn load(fs: &dyn Fs, path: &Path) -> Result<Self, Error> {
        let bytes = fs
            .read(path)
            .map_err(|source| io_error(format!("Read {}", path.display()), source))?;
        Self::from_json(&bytes).map_err(|error| match error {
            Error::Io { source, .. } => io_error(format!("Parse {}", path.display()), source),
            other => other,
        })
    }

    /// Atomically replace a full `meta.json` path without creating its parents.
    /// Dry-run filesystems record the write without changing the file.
    pub fn save(&self, fs: &dyn Fs, path: &Path) -> Result<(), Error> {
        let json = self.to_json()?;
        fs.write_atomic(path, json.as_bytes())
            .map_err(|source| io_error(format!("Write {}", path.display()), source))
    }
}

fn io_error(operation: String, source: io::Error) -> Error {
    Error::Io {
        operation,
        source,
        hint: "Check the version's meta.json and filesystem permissions; reinstall the version if its metadata is damaged.".into(),
    }
}
