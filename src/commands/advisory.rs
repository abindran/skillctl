//! Advisory checks printed after a pin changes or on `status --budget`: the
//! directive-description rules (O140–O145) for the skills just put in front
//! of an agent, and the skill index budget (O143). Advisory means stderr and
//! an `advisory` object in JSON results; nothing here changes an exit code.
use crate::{
    artifact::Kind,
    context::Context,
    error::Error,
    names::SkillName,
    registry::Pin,
    skill::Skill,
    store,
    validate::{self, Finding},
};
use serde::Serialize;
use std::collections::BTreeMap;

/// The context window the 1% index budget is measured against.
pub const CONTEXT_WINDOW_TOKENS: usize = 200_000;
/// Claude Code caps the skill listing at this share of the context window.
pub const INDEX_SHARE_PERCENT: usize = 1;
/// Claude Code truncates a description beyond this many characters.
pub const DESCRIPTION_MAX_CHARS: usize = 1536;

#[derive(Debug, Clone, Serialize)]
pub struct IndexEntry {
    pub name: SkillName,
    pub pin: Pin,
    /// Characters of name plus description, what the listing costs per skill.
    pub chars: usize,
    pub tokens: usize,
    pub description_chars: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Index {
    pub skills: Vec<IndexEntry>,
    pub tokens: usize,
    pub budget_tokens: usize,
    pub over_budget: bool,
}

/// Size the listing an agent sees for these pins. Hooks and missing versions
/// are skipped: they are not in any skill listing.
pub fn index(context: &Context<'_>, pins: &BTreeMap<SkillName, Pin>) -> Result<Index, Error> {
    let fs = context.fs.as_ref();
    let mut skills = Vec::new();
    for (name, pin) in pins {
        let dir = store::store_dir(&context.home, name, pin);
        if Kind::detect(fs, &dir)? != Some(Kind::Skill) {
            continue;
        }
        let Ok(skill) = Skill::load(fs, &dir) else {
            continue;
        };
        let description_chars = skill
            .frontmatter
            .description
            .value()
            .map(|text| text.chars().count())
            .unwrap_or(0);
        let chars = name.as_str().chars().count() + description_chars;
        skills.push(IndexEntry {
            name: name.clone(),
            pin: pin.clone(),
            chars,
            tokens: chars.div_ceil(4),
            description_chars,
            truncated: description_chars > DESCRIPTION_MAX_CHARS,
        });
    }
    let tokens = skills.iter().map(|entry| entry.tokens).sum();
    let budget_tokens = CONTEXT_WINDOW_TOKENS * INDEX_SHARE_PERCENT / 100;
    Ok(Index {
        skills,
        tokens,
        budget_tokens,
        over_budget: tokens > budget_tokens,
    })
}

/// O140–O145 over the skills among `pins`; hooks and unreadable folders skip.
pub fn description_findings(
    context: &Context<'_>,
    pins: &BTreeMap<SkillName, Pin>,
) -> Result<Vec<Finding>, Error> {
    let fs = context.fs.as_ref();
    let mut findings = Vec::new();
    for (name, pin) in pins {
        let dir = store::store_dir(&context.home, name, pin);
        if Kind::detect(fs, &dir)? != Some(Kind::Skill) {
            continue;
        }
        let found =
            validate::run_with_rules(fs, &dir, &[&validate::description::DescriptionRules])?;
        findings.extend(
            found
                .into_iter()
                .filter(|finding| finding.rule.starts_with("O14")),
        );
    }
    Ok(findings)
}

/// Lines for the text side of the advisory: one per finding, then the index.
pub fn text(findings: &[Finding], index: &Index) -> String {
    let mut lines = Vec::new();
    for finding in findings {
        lines.push(format!(
            "{} {} {} {}\n  Hint: {}",
            finding.path.display(),
            finding.rule,
            finding.severity,
            finding.message,
            finding.hint
        ));
    }
    lines.push(format!(
        "Index: {} skill(s), {} / {} tokens ({INDEX_SHARE_PERCENT}% of a {CONTEXT_WINDOW_TOKENS}-token context window)",
        index.skills.len(),
        index.tokens,
        index.budget_tokens
    ));
    if index.over_budget {
        lines.push(
            "  O143 warning: the skill listing exceeds the budget; skills past the cap lose their description and become invisible. Unpin skills or shorten descriptions."
                .into(),
        );
    }
    for entry in index.skills.iter().filter(|entry| entry.truncated) {
        lines.push(format!(
            "  O143 warning: {} has a {}-character description; harnesses truncate at {DESCRIPTION_MAX_CHARS}",
            entry.name, entry.description_chars
        ));
    }
    lines.join("\n")
}

/// Check the skills in `checked` and size the listing in `visible`, print the
/// result to stderr in text mode, and return the JSON object for the caller's
/// result document.
pub fn report(
    context: &mut Context<'_>,
    checked: &BTreeMap<SkillName, Pin>,
    visible: &BTreeMap<SkillName, Pin>,
) -> Result<serde_json::Value, Error> {
    let findings = description_findings(context, checked)?;
    let index = index(context, visible)?;
    let has_warnings = !findings.is_empty()
        || index.over_budget
        || index.skills.iter().any(|entry| entry.truncated);
    // In JSON mode the result object carries the same data, and stderr must
    // stay free for the single error document that may follow.
    if has_warnings && !context.flags.json {
        context
            .output
            .message(&text(&findings, &index))
            .map_err(super::output_error)?;
    }
    Ok(serde_json::json!({ "descriptions": findings, "index": index }))
}
