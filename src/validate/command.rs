//! Slash-command artifact rules K301–K305. A command is `COMMAND.md`: the
//! frontmatter both harnesses read (`description`, and Claude Code's optional
//! `argument-hint`, `allowed-tools`, `model`) plus skillctl's `name` and
//! `metadata.version`, and a body that is the prompt the command expands to.
//! Linked as `.claude/commands/<name>.md` and `.cursor/commands/<name>.md`.
use super::{Finding, Rule, Severity};
use crate::{names::SkillName, skill::Skill};

/// Rules an allow-list can never suppress (K301 is the runner's gate).
pub const STRUCTURAL: [&str; 4] = ["K301", "K302", "K303", "K304"];

pub struct CommandRules;

fn finding(
    skill: &Skill,
    id: &str,
    warning: bool,
    message: impl Into<String>,
    hint: &str,
) -> Finding {
    let mut finding = Finding::error(id, skill.document(), message, hint);
    if warning {
        finding.severity = Severity::Warning;
    }
    finding
}

impl Rule for CommandRules {
    fn check(&self, skill: &Skill) -> Vec<Finding> {
        let mut findings = Vec::new();
        let fields = &skill.frontmatter;
        match fields.name.value() {
            Some(name) if SkillName::new(name.clone()).is_err() => findings.push(finding(
                skill,
                "K302",
                false,
                "name must match the artifact-name pattern and contain 1–64 characters",
                "Use lowercase ASCII letters and digits separated by single hyphens; it becomes the slash command.",
            )),
            Some(name) if skill.dir.file_name().and_then(|v| v.to_str()) != Some(name.as_str()) => {
                findings.push(finding(
                    skill,
                    "K302",
                    false,
                    "name does not equal the command folder's name",
                    "Make the frontmatter name and command folder name identical.",
                ))
            }
            Some(_) => {}
            None => findings.push(finding(
                skill,
                "K302",
                false,
                "name is required and must be a string",
                "Set name to the slash command's name.",
            )),
        }
        if !fields
            .description
            .value()
            .is_some_and(|value| !value.trim().is_empty() && value.chars().count() <= 1024)
        {
            findings.push(finding(
                skill,
                "K303",
                false,
                "description must be a non-empty string of at most 1,024 characters; harnesses show it in the command list",
                "Describe what the command does in one sentence.",
            ));
        }
        if skill.version().ok().flatten().is_none() {
            findings.push(finding(
                skill,
                "K304",
                false,
                "metadata.version must be present and parse as semver",
                "Set metadata.version to a quoted semver string under a metadata mapping.",
            ));
        }
        if skill.body.trim().is_empty() {
            findings.push(finding(
                skill,
                "K305",
                false,
                "the command body is empty, so the command expands to nothing",
                "Write the prompt below the frontmatter; $ARGUMENTS is replaced by what the user typed after the command.",
            ));
        }
        findings
    }
}
