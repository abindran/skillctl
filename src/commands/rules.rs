//! `rules [--for <kind>]`: the rule catalog, for reference. Read-only and
//! needs no home folder.
use crate::{
    context::Context,
    error::Error,
    validate::catalog::{self, RuleInfo},
};
use serde::Serialize;

const KINDS: [&str; 7] = [
    "skill",
    "hook",
    "rule",
    "command",
    "agent",
    "script",
    "instructions",
];

#[derive(Serialize)]
struct Entry {
    #[serde(flatten)]
    rule: RuleInfo,
    /// `validate.allow` can silence it; structural rules never are.
    allow_listable: bool,
    /// `validate`, `lint`, or `advisory` (O143, reported after pin changes).
    reported_by: &'static str,
}

pub fn run(context: &mut Context<'_>, applies_to: Option<&str>) -> Result<(), Error> {
    if let Some(kind) = applies_to.filter(|kind| !KINDS.contains(kind)) {
        return Err(Error::Usage {
            message: format!("Unknown kind '{kind}'"),
            hint: format!("Use one of: {}.", KINDS.join(", ")),
        });
    }
    let entries: Vec<Entry> = catalog::RULES
        .iter()
        .filter(|rule| applies_to.is_none_or(|kind| rule.applies_to == kind))
        .map(|rule| Entry {
            rule: *rule,
            allow_listable: catalog::allow_listable(rule.id),
            reported_by: match (rule.applies_to, rule.id) {
                ("instructions", _) => "lint",
                (_, "O143") => "advisory",
                _ => "validate",
            },
        })
        .collect();
    let mut text = Vec::new();
    let mut group = "";
    for entry in &entries {
        if entry.rule.group != group {
            group = entry.rule.group;
            if !text.is_empty() {
                text.push(String::new());
            }
            text.push(format!("{group} ({})", entry.rule.applies_to));
        }
        text.push(format!(
            "  {} {:<7} {}{}",
            entry.rule.id,
            entry.rule.severity,
            entry.rule.summary,
            if entry.allow_listable {
                ""
            } else {
                " [cannot be allow-listed]"
            }
        ));
    }
    context
        .output
        .result(&entries, &text.join("\n"))
        .map_err(super::output_error)
}
