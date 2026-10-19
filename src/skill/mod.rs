//! Parsed skill documents, independent of store layout and command execution.

pub mod frontmatter;

use std::path::{Path, PathBuf};

use semver::Version;

use crate::{artifact::Kind, error::Error, services::fs::Fs};
pub use frontmatter::{Field, Frontmatter};

/// A parsed root document: `SKILL.md` for skills, `HOOK.md` for hooks. The name
/// predates hooks; both kinds share the frontmatter-plus-body shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub dir: PathBuf,
    pub kind: Kind,
    pub frontmatter: Frontmatter,
    pub body: String,
}

impl Skill {
    /// Parse an in-memory SKILL.md without reading or canonicalizing its directory.
    pub fn parse(dir: impl Into<PathBuf>, document: &str) -> Result<Self, Error> {
        Self::parse_kind(dir, document, Kind::Skill)
    }

    /// Parse an in-memory root document of the given kind.
    pub fn parse_kind(dir: impl Into<PathBuf>, document: &str, kind: Kind) -> Result<Self, Error> {
        let (frontmatter, body) = frontmatter::parse(document)?;
        Ok(Self {
            dir: dir.into(),
            kind,
            frontmatter,
            body,
        })
    }

    /// The root document's path.
    pub fn document(&self) -> PathBuf {
        self.dir.join(self.kind.document())
    }

    /// Load the root SKILL.md through the shared filesystem boundary.
    pub fn load(fs: &dyn Fs, dir: &Path) -> Result<Self, Error> {
        Self::load_kind(fs, dir, Kind::Skill)
    }

    /// Load the root document of the given kind.
    pub fn load_kind(fs: &dyn Fs, dir: &Path, kind: Kind) -> Result<Self, Error> {
        let path = dir.join(kind.document());
        let bytes = fs.read(&path).map_err(|source| Error::Io {
            operation: format!("Read {}", path.display()),
            source,
            hint: format!(
                "Check that the {kind} folder contains a readable {} file.",
                kind.document()
            ),
        })?;
        let document = std::str::from_utf8(&bytes).map_err(|error| {
            frontmatter::invalid(format!("{} is not valid UTF-8: {error}", path.display()))
        })?;
        Self::parse_kind(dir, document, kind).map_err(|error| match error {
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
