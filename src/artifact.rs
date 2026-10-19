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
    /// A Cursor rule: `RULE.mdc` with `description`, `globs` and `alwaysApply`,
    /// linked as `.cursor/rules/<name>.mdc`.
    Rule,
    /// A slash command: `COMMAND.md` whose body is the prompt, linked as
    /// `.claude/commands/<name>.md` and `.cursor/commands/<name>.md`.
    Command,
    /// A subagent: `AGENT.md` (Claude Code's frontmatter plus Cursor's
    /// `readonly`), an optional `cursor.md` override and an optional
    /// `schema.json` for its output, linked as `.claude/agents/<name>.md` and,
    /// when `cursor.md` exists, `.cursor/agents/<name>.md`.
    Agent,
    /// A script bundle: `SCRIPT.md` describing the wrappers in the folder,
    /// linked as the folder `.claude/scripts/<name>` and referenced from
    /// `AGENTS.md` so agents run the quiet wrapper instead of the raw command.
    Script,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Skill,
        Kind::Hook,
        Kind::Rule,
        Kind::Command,
        Kind::Agent,
        Kind::Script,
    ];

    /// The frontmatter document at the folder root.
    pub fn document(self) -> &'static str {
        match self {
            Self::Skill => "SKILL.md",
            Self::Hook => "HOOK.md",
            Self::Rule => "RULE.mdc",
            Self::Command => "COMMAND.md",
            Self::Agent => "AGENT.md",
            Self::Script => "SCRIPT.md",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Hook => "hook",
            Self::Rule => "rule",
            Self::Command => "command",
            Self::Agent => "agent",
            Self::Script => "script",
        }
    }

    /// Whether `add`/`attach` may pin this kind into a project. Hooks are
    /// registered with `hook add` instead.
    pub fn linkable(self) -> bool {
        !matches!(self, Self::Hook)
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
                summary: format!(
                    "{} contains more than one root document ({})",
                    dir.display(),
                    present
                        .iter()
                        .map(|kind| kind.document())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                hint: "A folder is one artifact kind; keep exactly one of SKILL.md, HOOK.md, RULE.mdc, COMMAND.md, AGENT.md or SCRIPT.md.".into(),
            }),
        }
    }

    /// Like [`Kind::detect`], but a folder without either document is an error.
    pub fn of(fs: &dyn Fs, dir: &Path) -> Result<Kind, Error> {
        Self::detect(fs, dir)?.ok_or_else(|| Error::Validation {
            summary: format!(
                "{} contains no root document (SKILL.md, HOOK.md, RULE.mdc, COMMAND.md, AGENT.md or SCRIPT.md)",
                dir.display()
            ),
            hint: "Point skillctl at an artifact folder: a skill (SKILL.md), hook (HOOK.md), rule (RULE.mdc), command (COMMAND.md), agent (AGENT.md) or script bundle (SCRIPT.md).".into(),
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
