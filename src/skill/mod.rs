//! Parsed skill documents, independent of store layout and command execution.

pub mod frontmatter;

use std::path::{Path, PathBuf};

use semver::Version;

use crate::{error::Error, services::fs::Fs};
pub use frontmatter::{Field, Frontmatter};

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub dir: PathBuf,
    pub frontmatter: Frontmatter,
    pub body: String,
}

impl Skill {
    /// Parse an in-memory SKILL.md without reading or canonicalizing its directory.
    pub fn parse(dir: impl Into<PathBuf>, document: &str) -> Result<Self, Error> {
        let (frontmatter, body) = frontmatter::parse(document)?;
        Ok(Self {
            dir: dir.into(),
            frontmatter,
            body,
        })
    }

    /// Load the root SKILL.md through the shared filesystem boundary.
    pub fn load(fs: &dyn Fs, dir: &Path) -> Result<Self, Error> {
        let path = dir.join("SKILL.md");
        let bytes = fs.read(&path).map_err(|source| Error::Io {
            operation: format!("Read {}", path.display()),
            source,
            hint: "Check that the skill folder contains a readable SKILL.md file.".into(),
        })?;
        let document = std::str::from_utf8(&bytes).map_err(|error| {
            frontmatter::invalid(format!("{} is not valid UTF-8: {error}", path.display()))
        })?;
        Self::parse(dir, document).map_err(|error| match error {
            Error::Validation { summary, hint } => Error::Validation {
                summary: format!("{}: {summary}", path.display()),
                hint,
            },
            other => other,
        })
    }

    pub fn version(&self) -> Result<Option<Version>, Error> {
        self.frontmatter.version()
    }
}
