//! Artifact kinds stored side by side in one flat namespace. A skill folder
//! carries `SKILL.md`; a hook folder carries `HOOK.md` plus the script that a
//! harness runs on lifecycle events. The kind is detected from the folder, so
//! drafts, store versions and install sources need no extra marker, and it is
//! recorded in `meta.json` for callers that must not read the folder.
use std::{io, path::Path};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::{error::Error, services::fs::Fs};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize, ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// An Agent Skills folder: `SKILL.md` with optional scripts and references.
    #[default]
    Skill,
    /// A lifecycle hook: `HOOK.md` declaring the events and the script to run.
    Hook,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Skill, Kind::Hook];

    /// The frontmatter document at the folder root.
    pub fn document(self) -> &'static str {
        match self {
            Self::Skill => "SKILL.md",
            Self::Hook => "HOOK.md",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Hook => "hook",
        }
    }

    /// Which kind a folder holds, from the document present at its root; `None`
    /// when neither is there. A folder with both is ambiguous and refused.
    pub fn detect(fs: &dyn Fs, dir: &Path) -> Result<Option<Kind>, Error> {
        let mut present = Vec::new();
        for kind in Self::ALL {
            let path = dir.join(kind.document());
            match fs.symlink_metadata(&path) {
                Ok(_) => present.push(kind),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(Error::Io {
                        operation: format!("Inspect {}", path.display()),
                        source,
                        hint: "Check the folder and its read permissions.".into(),
                    });
                }
            }
        }
        match present.as_slice() {
            [] => Ok(None),
            [kind] => Ok(Some(*kind)),
            _ => Err(Error::Validation {
                summary: format!("{} contains both SKILL.md and HOOK.md", dir.display()),
                hint: "A folder is either a skill or a hook; remove one of the documents.".into(),
            }),
        }
    }

    /// Like [`Kind::detect`], but a folder without either document is an error.
    pub fn of(fs: &dyn Fs, dir: &Path) -> Result<Kind, Error> {
        Self::detect(fs, dir)?.ok_or_else(|| Error::Validation {
            summary: format!("{} contains neither SKILL.md nor HOOK.md", dir.display()),
            hint: "Point skillctl at a skill folder (SKILL.md) or a hook folder (HOOK.md).".into(),
        })
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::fs::RealFs;

    #[test]
    fn kind_is_detected_from_the_root_document() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        assert_eq!(Kind::detect(&RealFs, dir).unwrap(), None);
        assert!(Kind::of(&RealFs, dir).is_err());
        std::fs::write(dir.join("HOOK.md"), "---\n---\n").unwrap();
        assert_eq!(Kind::of(&RealFs, dir).unwrap(), Kind::Hook);
        std::fs::write(dir.join("SKILL.md"), "---\n---\n").unwrap();
        assert!(Kind::detect(&RealFs, dir).is_err());
    }

    #[test]
    fn kind_serializes_lowercase_and_defaults_to_skill() {
        assert_eq!(serde_json::to_string(&Kind::Hook).unwrap(), "\"hook\"");
        assert_eq!(Kind::default(), Kind::Skill);
        assert_eq!(Kind::Hook.document(), "HOOK.md");
        assert_eq!(Kind::Skill.to_string(), "skill");
    }
}
