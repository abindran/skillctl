//! Cross-harness portability warnings. Never execute skill content.
use super::{Finding, Rule, Severity, fields};
use crate::skill::{Field, Skill};

pub struct HarnessRules;
impl Rule for HarnessRules {
    fn check(&self, skill: &Skill) -> Vec<Finding> {
        let mut findings = Vec::new();
        let mut warn = |id, message: String, hint| {
            let mut finding = Finding::error(id, skill.dir.join("SKILL.md"), message, hint);
            finding.severity = Severity::Warning;
            findings.push(finding);
        };
        for key in skill.frontmatter.extra.keys() {
            if fields::CLAUDE_ONLY.contains(&key.as_str()) {
                warn(
                    "H201",
                    format!("{key}: other harnesses ignore this"),
                    "Keep essential instructions portable in the body.",
                );
            } else if !fields::known(key) {
                warn(
                    "H205",
                    format!("Unknown frontmatter field: {key}"),
                    "Remove the field or verify its support in each harness.",
                );
            }
        }
        if !matches!(skill.frontmatter.allowed_tools, Field::Missing) {
            warn(
                "H202",
                "allowed-tools pre-approves tools; support varies by harness".into(),
                "Review the tool permissions in each harness.",
            );
        }
        if skill.body.contains("${CLAUDE_SKILL_DIR}")
            || skill.body.contains("$ARGUMENTS")
            || skill.body.contains("!`")
        {
            warn(
                "H203",
                "Claude Code substitutions appear as plain text in other harnesses".into(),
                "Provide portable paths and instructions instead of harness substitutions.",
            );
        }
        if skill
            .frontmatter
            .extra
            .get("context")
            .and_then(|value| value.as_str())
            == Some("fork")
        {
            let body = skill.body.to_lowercase();
            if [
                "ask the user",
                "ask user",
                "prompt the user",
                "question the user",
            ]
            .iter()
            .any(|text| body.contains(text))
            {
                warn(
                    "H204",
                    "context: fork cannot ask the user from a Claude Code subagent".into(),
                    "Remove fork or move user interaction to the parent agent.",
                );
            }
        }
        findings
    }
}
