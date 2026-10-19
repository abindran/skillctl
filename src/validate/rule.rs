//! Cursor rule artifact rules K201–K207. A rule is `RULE.mdc`: Cursor's
//! frontmatter (`description`, optional `globs`, `alwaysApply`) plus skillctl's
//! `name` and `metadata.version`, and a Markdown body that becomes the rule
//! text. Linked as `.cursor/rules/<name>.mdc`, which Cursor loads for every
//! turn when `alwaysApply` is true, so size is checked here as in `lint`'s I302.
use super::{Finding, Rule, Severity};
use crate::{names::SkillName, skill::Skill};
use serde_yaml_ng::Value;

/// Rules an allow-list can never suppress (K201 is the runner's gate).
pub const STRUCTURAL: [&str; 4] = ["K201", "K202", "K203", "K204"];
pub const MAX_LINES: usize = 500;
pub const ALWAYS_APPLY_MAX_LINES: usize = 100;

pub struct RuleRules;

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

/// Whether the frontmatter sets `alwaysApply: true`.
pub fn always_apply(skill: &Skill) -> bool {
    skill
        .frontmatter
        .extra
        .get("alwaysApply")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

impl Rule for RuleRules {
    fn check(&self, skill: &Skill) -> Vec<Finding> {
        let mut findings = Vec::new();
        let fields = &skill.frontmatter;
        match fields.name.value() {
            Some(name) if SkillName::new(name.clone()).is_err() => findings.push(finding(
                skill,
                "K202",
                false,
                "name must match the artifact-name pattern and contain 1–64 characters",
                "Use lowercase ASCII letters and digits separated by single hyphens.",
            )),
            Some(name) if skill.dir.file_name().and_then(|v| v.to_str()) != Some(name.as_str()) => {
                findings.push(finding(
                    skill,
                    "K202",
                    false,
                    "name does not equal the rule folder's name",
                    "Make the frontmatter name and rule folder name identical.",
                ))
            }
            Some(_) => {}
            None => findings.push(finding(
                skill,
                "K202",
                false,
                "name is required and must be a string",
                "Set name to 1–64 lowercase ASCII letters or digits separated by single hyphens.",
            )),
        }
        if !fields
            .description
            .value()
            .is_some_and(|value| !value.trim().is_empty() && value.chars().count() <= 1024)
        {
            findings.push(finding(
                skill,
                "K203",
                false,
                "description must be a non-empty string of at most 1,024 characters; Cursor shows it when deciding whether to attach the rule",
                "Describe when the rule applies.",
            ));
        }
        if skill.version().ok().flatten().is_none() {
            findings.push(finding(
                skill,
                "K204",
                false,
                "metadata.version must be present and parse as semver",
                "Set metadata.version to a quoted semver string under a metadata mapping.",
            ));
        }
        if let Some(value) = fields.extra.get("alwaysApply") {
            if !value.is_bool() {
                findings.push(finding(
                    skill,
                    "K205",
                    false,
                    "alwaysApply must be true or false",
                    "Write alwaysApply: true or alwaysApply: false.",
                ));
            }
        }
        if let Some(value) = fields.extra.get("globs") {
            let ok = value.as_str().is_some_and(|text| !text.trim().is_empty())
                || value.as_sequence().is_some_and(|items| {
                    !items.is_empty() && items.iter().all(|item| item.as_str().is_some())
                });
            if !ok {
                findings.push(finding(
                    skill,
                    "K205",
                    false,
                    "globs must be a pattern string or a list of pattern strings",
                    "Write globs: \"**/*.ts\" or globs: [\"src/**\", \"lib/**\"].",
                ));
            }
        }
        if skill.body.trim().is_empty() {
            findings.push(finding(
                skill,
                "K206",
                false,
                "the rule body is empty, so the rule says nothing when it applies",
                "Write the rule text below the frontmatter.",
            ));
        }
        let lines = skill.body.lines().count();
        if lines > MAX_LINES {
            findings.push(finding(
                skill,
                "K207",
                true,
                format!("rule body is {lines} lines; the budget is {MAX_LINES}"),
                "Split the rule by topic or move procedures into skills.",
            ));
        } else if always_apply(skill) && lines > ALWAYS_APPLY_MAX_LINES {
            findings.push(finding(
                skill,
                "K207",
                true,
                format!("always-apply rule body is {lines} lines; it is paid for on every turn (budget {ALWAYS_APPLY_MAX_LINES})"),
                "Scope the rule with globs or a description so it loads on demand, or shorten it.",
            ));
        }
        findings
    }
}
