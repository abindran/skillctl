//! Directive-description rules O140–O145. The description is the whole
//! activation surface: at session start a harness sees only each skill's name
//! and description, and loads the body after deciding the skill applies. In a
//! 650-trial study, descriptions of the form "ALWAYS invoke this skill when …
//! Do not X directly" activated 100% of the time with no hook, weak ones fell
//! to 37% even with a hook, and a `keywords` field changed nothing. All rules
//! are warnings so legacy skills keep sealing while the report says what to fix.
use super::{Finding, Rule, Severity};
use crate::skill::Skill;
use regex::Regex;
use std::sync::LazyLock;

pub struct DescriptionRules;

/// A trigger verb addressing "this skill" within one sentence. The "when"
/// clause itself is O103, so the two rules compose without overlapping.
static DIRECTIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:always\s+|must\s+)?(?:invoke|use|apply|run|load|activate|call)\b[^.\n]{0,40}?\bthis skill\b",
    )
    .unwrap()
});
static NEGATIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:do not|don't|never|must not)\b").unwrap());
static QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""[^"\n]{3,}"|“[^”\n]{3,}”|'[^'\n]{3,}'"#).unwrap());
static EXAMPLES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:including|such as|for example|for instance|e\.g\.|phrasing like|phrases like|requests like)\b")
        .unwrap()
});
static HEDGING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:helps?(?: you)? (?:with|to)|assists?(?: with)?|can be used|may be used|useful for|various|general[- ]purpose|and more|etc\.?)\b",
    )
    .unwrap()
});
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\]\n]{1,60}\]").unwrap());

fn warning(skill: &Skill, id: &str, message: &str, hint: &str) -> Finding {
    let mut finding = Finding::error(id, skill.document(), message, hint);
    finding.severity = Severity::Warning;
    finding
}

impl Rule for DescriptionRules {
    fn check(&self, skill: &Skill) -> Vec<Finding> {
        let mut findings = Vec::new();
        // No description is S005; nothing to judge here.
        let Some(description) = skill.frontmatter.description.value() else {
            return findings;
        };
        if !DIRECTIVE.is_match(description) {
            findings.push(warning(
                skill,
                "O140",
                "description does not address the agent in directive form (a trigger verb and \"this skill\")",
                "Start with the directive, for example: ALWAYS invoke this skill when the user asks to …",
            ));
        }
        if !NEGATIVE.is_match(description) {
            findings.push(warning(
                skill,
                "O141",
                "description has no negative constraint telling the agent what not to do without the skill",
                "End with the constraint, for example: Do not write migration files directly without this skill.",
            ));
        }
        let quoted = QUOTED.find_iter(description).count();
        if quoted < 2 && !EXAMPLES.is_match(description) {
            findings.push(warning(
                skill,
                "O142",
                "description gives no concrete trigger phrasings, so indirect requests are likely to be missed",
                "Add the ways people actually ask, for example: including indirect phrasing like \"add a column\" or \"rename this field\".",
            ));
        }
        if let Some(found) = HEDGING.find(description) {
            findings.push(warning(
                skill,
                "O144",
                &format!(
                    "description hedges ({:?}); vague wording is the pattern that fails to activate",
                    found.as_str()
                ),
                "State exactly which requests the skill handles instead of what it can help with.",
            ));
        } else if let Some(found) = PLACEHOLDER.find(description) {
            findings.push(warning(
                skill,
                "O144",
                &format!(
                    "description still contains the scaffold placeholder {}",
                    found.as_str()
                ),
                "Replace every [placeholder] from the template with the skill's real triggers.",
            ));
        }
        if skill.frontmatter.extra.contains_key("keywords") {
            findings.push(warning(
                skill,
                "O145",
                "keywords field has no measured effect on activation; the description is what the agent reads",
                "Remove keywords and put the trigger words into the description itself.",
            ));
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(description: &str, extra: &str) -> Skill {
        Skill::parse(
            "/tmp/db-migrations",
            &format!("---\nname: db-migrations\ndescription: {description}\n{extra}metadata:\n  version: '1.0.0'\n---\nbody\n"),
        )
        .unwrap()
    }
    fn ids(description: &str, extra: &str) -> Vec<String> {
        DescriptionRules
            .check(&skill(description, extra))
            .into_iter()
            .map(|f| f.rule)
            .collect()
    }

    #[test]
    fn the_directive_form_is_clean() {
        let good = r#"ALWAYS invoke this skill when the user asks to create, modify, roll back or review a database migration, schema change or ORM model change, including indirect phrasing like "add a column" or "rename this field". Do not write migration files directly without this skill."#;
        assert_eq!(ids(good, ""), Vec::<String>::new());
        let also_good = "Use this skill whenever a request touches payment refunds, such as partial refunds or chargebacks. Never issue refunds without it.";
        assert_eq!(ids(also_good, ""), Vec::<String>::new());
    }

    #[test]
    fn weak_descriptions_trigger_each_rule() {
        assert_eq!(
            ids("Helps with database migrations and more.", ""),
            ["O140", "O141", "O142", "O144"]
        );
        assert_eq!(
            ids(
                r#"ALWAYS use this skill when asked to migrate, including "add a column". Do not do it by hand."#,
                "keywords: [migration, schema]\n"
            ),
            ["O145"]
        );
        assert_eq!(
            ids(
                r#"ALWAYS invoke this skill when the user asks to [describe the task], including indirect phrasing like "[example request]" or "[another example]". Do not [do the task] directly without this skill."#,
                ""
            ),
            ["O144"]
        );
        assert_eq!(
            ids("Use when reviewing code. Never skip the checklist.", ""),
            ["O140", "O142"]
        );
        // The when clause is O103's job; a directive without one passes O140.
        assert!(
            !ids(
                r#"Invoke this skill for refunds such as "partial refund". Do not refund by hand."#,
                ""
            )
            .contains(&"O140".to_owned())
        );
    }
}
