//! Every rule `validate` and `lint` report, with what it checks: the
//! reference behind `skillctl rules` and the app's Rules tab. A test fails
//! when a rule ID used in the rule modules is missing here, so the catalog
//! cannot drift from the checks.
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct RuleInfo {
    pub id: &'static str,
    /// `skill`, `hook`, `rule`, `command`, `agent`, `script`, or
    /// `instructions` for the lint rules.
    pub applies_to: &'static str,
    pub group: &'static str,
    /// The default severity; `--strict` promotes warnings to errors.
    pub severity: &'static str,
    pub summary: &'static str,
}

const fn rule(
    id: &'static str,
    applies_to: &'static str,
    group: &'static str,
    severity: &'static str,
    summary: &'static str,
) -> RuleInfo {
    RuleInfo {
        id,
        applies_to,
        group,
        severity,
        summary,
    }
}

const SPEC: &str = "Agent Skills spec";
const ORG: &str = "Organisation rules";
const DIRECTIVE: &str = "Directive descriptions";
const HARNESS: &str = "Harness compatibility";
const HOOK: &str = "Hook artifacts";
const RULE: &str = "Rule artifacts";
const COMMAND: &str = "Command artifacts";
const AGENT: &str = "Agent artifacts";
const SCRIPT: &str = "Script bundles";
const LINT: &str = "Instruction files";
const E: &str = "error";
const W: &str = "warning";

pub const RULES: &[RuleInfo] = &[
    rule(
        "S001",
        "skill",
        SPEC,
        E,
        "SKILL.md exists in the skill folder.",
    ),
    rule(
        "S002",
        "skill",
        SPEC,
        E,
        "SKILL.md starts with YAML frontmatter that is a mapping.",
    ),
    rule(
        "S003",
        "skill",
        SPEC,
        E,
        "name is present: 1–64 lowercase letters or digits separated by single hyphens.",
    ),
    rule(
        "S004",
        "skill",
        SPEC,
        E,
        "name equals the skill folder's name.",
    ),
    rule(
        "S005",
        "skill",
        SPEC,
        E,
        "description is a non-empty string of at most 1,024 characters.",
    ),
    rule(
        "S006",
        "skill",
        SPEC,
        E,
        "compatibility, when present, is a string of 1–500 characters.",
    ),
    rule(
        "S007",
        "skill",
        SPEC,
        E,
        "metadata maps strings to strings.",
    ),
    rule(
        "S008",
        "skill",
        SPEC,
        E,
        "license and allowed-tools, when present, are strings.",
    ),
    rule(
        "O101",
        "skill",
        ORG,
        E,
        "metadata.version is present and parses as semver.",
    ),
    rule(
        "O102",
        "skill",
        ORG,
        W,
        "description is at least 80 characters.",
    ),
    rule(
        "O103",
        "skill",
        ORG,
        W,
        "description says when the skill applies (a \"when\" clause).",
    ),
    rule("O104", "skill", ORG, E, "description contains no < or >."),
    rule(
        "O110",
        "skill",
        ORG,
        W,
        "The body is at most 500 lines; move detail into references/.",
    ),
    rule(
        "O111",
        "skill",
        ORG,
        W,
        "The body is at most about 5,000 tokens (characters / 4).",
    ),
    rule(
        "O112",
        "skill",
        ORG,
        W,
        "Each reference file is at most about 10,000 tokens.",
    ),
    rule(
        "O120",
        "skill",
        ORG,
        E,
        "Every path the body references exists and is not a symlink.",
    ),
    rule(
        "O121",
        "skill",
        ORG,
        W,
        "Every bundled file is referenced from SKILL.md.",
    ),
    rule(
        "O122",
        "skill",
        ORG,
        E,
        "No symlinks inside the skill folder.",
    ),
    rule(
        "O123",
        "skill",
        ORG,
        E,
        "No reference escapes the skill folder with .. components.",
    ),
    rule(
        "O130",
        "skill",
        ORG,
        E,
        "Scripts have an interpreter shebang and the executable bit.",
    ),
    rule(
        "O131",
        "skill",
        ORG,
        W,
        "Scripts do not ask for interactive terminal input.",
    ),
    rule(
        "O132",
        "skill",
        ORG,
        E,
        "Scripts use no sudo and pipe no download into a shell (curl … | sh).",
    ),
    rule(
        "O133",
        "skill",
        ORG,
        E,
        "No file contains a likely secret (the value is never printed).",
    ),
    rule(
        "O140",
        "skill",
        DIRECTIVE,
        W,
        "The description is a directive: a trigger verb addressing \"this skill\", for example \"ALWAYS invoke this skill when …\".",
    ),
    rule(
        "O141",
        "skill",
        DIRECTIVE,
        W,
        "The description ends with a negative constraint, for example \"Do not … without this skill\".",
    ),
    rule(
        "O142",
        "skill",
        DIRECTIVE,
        W,
        "The description gives concrete trigger phrasings (quoted, or after \"including\" or \"such as\").",
    ),
    rule(
        "O143",
        "skill",
        DIRECTIVE,
        W,
        "The skill listing fits the index budget (1% of the context window) and no description exceeds 1,536 characters. Reported by status --budget and after add, not by validate.",
    ),
    rule(
        "O144",
        "skill",
        DIRECTIVE,
        W,
        "No hedging (\"helps with\", \"various\", \"etc.\") and no [placeholders] left in the description.",
    ),
    rule(
        "O145",
        "skill",
        DIRECTIVE,
        W,
        "No keywords field; it has no measured effect on activation.",
    ),
    rule(
        "H201",
        "skill",
        HARNESS,
        W,
        "Claude-only frontmatter fields, which other harnesses ignore.",
    ),
    rule(
        "H202",
        "skill",
        HARNESS,
        W,
        "allowed-tools pre-approves tools, and support varies by harness.",
    ),
    rule(
        "H203",
        "skill",
        HARNESS,
        W,
        "Claude Code substitutions in the body (${CLAUDE_SKILL_DIR}, $ARGUMENTS, !`cmd`) appear as plain text elsewhere.",
    ),
    rule(
        "H204",
        "skill",
        HARNESS,
        W,
        "context: fork cannot ask the user questions from a subagent.",
    ),
    rule("H205", "skill", HARNESS, W, "Unknown frontmatter fields."),
    rule(
        "K101",
        "hook",
        HOOK,
        E,
        "HOOK.md exists and its frontmatter parses.",
    ),
    rule(
        "K102",
        "hook",
        HOOK,
        E,
        "name follows the artifact-name pattern and equals the folder name.",
    ),
    rule(
        "K103",
        "hook",
        HOOK,
        E,
        "description is a non-empty string of at most 1,024 characters.",
    ),
    rule(
        "K104",
        "hook",
        HOOK,
        E,
        "metadata.version is present and parses as semver.",
    ),
    rule(
        "K105",
        "hook",
        HOOK,
        E,
        "events is a non-empty list of event names.",
    ),
    rule(
        "K106",
        "hook",
        HOOK,
        W,
        "Every event name is a known Claude Code event (names are case-sensitive).",
    ),
    rule(
        "K107",
        "hook",
        HOOK,
        E,
        "entry names a regular file inside the folder with a shebang and the executable bit.",
    ),
    rule(
        "K108",
        "hook",
        HOOK,
        W,
        "Scripts never prompt, use no sudo and pipe no download into a shell; hooks run unattended.",
    ),
    rule("K109", "hook", HOOK, E, "No file contains a likely secret."),
    rule(
        "K110",
        "hook",
        HOOK,
        E,
        "No symlinks inside the hook folder.",
    ),
    rule(
        "K111",
        "hook",
        HOOK,
        E,
        "matchers name declared events, timeout is 1–600 seconds and companions is a list of artifact names.",
    ),
    rule(
        "K201",
        "rule",
        RULE,
        E,
        "RULE.mdc exists, its frontmatter parses, and there are no symlinks.",
    ),
    rule(
        "K202",
        "rule",
        RULE,
        E,
        "name follows the artifact-name pattern and equals the folder name.",
    ),
    rule(
        "K203",
        "rule",
        RULE,
        E,
        "description is non-empty, at most 1,024 characters; Cursor reads it to decide whether to attach the rule.",
    ),
    rule(
        "K204",
        "rule",
        RULE,
        E,
        "metadata.version is present and parses as semver.",
    ),
    rule(
        "K205",
        "rule",
        RULE,
        E,
        "alwaysApply is true or false, and globs is a pattern string or a list of them.",
    ),
    rule("K206", "rule", RULE, E, "The rule body is not empty."),
    rule(
        "K207",
        "rule",
        RULE,
        W,
        "The body is at most 500 lines, or 100 for an always-apply rule.",
    ),
    rule(
        "K301",
        "command",
        COMMAND,
        E,
        "COMMAND.md exists, its frontmatter parses, and there are no symlinks.",
    ),
    rule(
        "K302",
        "command",
        COMMAND,
        E,
        "name follows the artifact-name pattern and equals the folder name; it becomes the slash command.",
    ),
    rule(
        "K303",
        "command",
        COMMAND,
        E,
        "description is non-empty, at most 1,024 characters; harnesses show it in the command list.",
    ),
    rule(
        "K304",
        "command",
        COMMAND,
        E,
        "metadata.version is present and parses as semver.",
    ),
    rule(
        "K305",
        "command",
        COMMAND,
        E,
        "The command body (the prompt) is not empty.",
    ),
    rule(
        "H220",
        "agent",
        AGENT,
        E,
        "AGENT.md exists, its frontmatter parses, and there are no symlinks.",
    ),
    rule(
        "H221",
        "agent",
        AGENT,
        E,
        "name is set, has no ':' or leading '-', and matches the folder name (ignoring case).",
    ),
    rule(
        "H222",
        "agent",
        AGENT,
        W,
        "A Claude-only model alias (sonnet, opus, haiku, inherit) needs a cursor.md override, or Cursor uses its default model.",
    ),
    rule(
        "H223",
        "agent",
        AGENT,
        E,
        "description (at most 1,024 characters), metadata.version (semver) and a non-empty system prompt are present.",
    ),
    rule(
        "H224",
        "agent",
        AGENT,
        W,
        "tools scopes the agent in Claude Code only; add readonly: true for Cursor when it must not edit.",
    ),
    rule(
        "H225",
        "agent",
        AGENT,
        E,
        "Field types: tools a string or list, readonly a boolean, skills a list, maxTurns a positive integer, model a string.",
    ),
    rule(
        "H226",
        "agent",
        AGENT,
        W,
        "cursor.md still matches AGENT.md; regenerate it with agent derive after edits.",
    ),
    rule(
        "H227",
        "agent",
        AGENT,
        E,
        "schema.json, when present, is a JSON object (the output contract).",
    ),
    rule(
        "K501",
        "script",
        SCRIPT,
        E,
        "SCRIPT.md exists and its frontmatter parses.",
    ),
    rule(
        "K502",
        "script",
        SCRIPT,
        E,
        "name follows the artifact-name pattern and equals the folder name (linked as .claude/scripts/<name>).",
    ),
    rule(
        "K503",
        "script",
        SCRIPT,
        E,
        "description is a non-empty string of at most 1,024 characters.",
    ),
    rule(
        "K504",
        "script",
        SCRIPT,
        E,
        "metadata.version is present and parses as semver.",
    ),
    rule(
        "K505",
        "script",
        SCRIPT,
        E,
        "There is at least one wrapper, each with a shebang and the executable bit.",
    ),
    rule(
        "K506",
        "script",
        SCRIPT,
        W,
        "Wrappers never prompt for input; agents run them unattended.",
    ),
    rule(
        "K507",
        "script",
        SCRIPT,
        E,
        "No symlinks inside the bundle.",
    ),
    rule(
        "I301",
        "instructions",
        LINT,
        W,
        "AGENTS.md is at most 200 lines; every line is paid for on every turn in both tools.",
    ),
    rule(
        "I302",
        "instructions",
        LINT,
        W,
        "Cursor rules are at most 500 lines, or 100 when alwaysApply is true.",
    ),
    rule(
        "I303",
        "instructions",
        LINT,
        W,
        "No AGENTS.md section reads like a procedure; procedures belong in skills, which load only when needed.",
    ),
    rule(
        "I304",
        "instructions",
        LINT,
        E,
        "CLAUDE.md imports AGENTS.md (@AGENTS.md), and the imported AGENTS.md exists.",
    ),
    rule(
        "I305",
        "instructions",
        LINT,
        W,
        "Always-loaded files contain no timestamps or changing imports, which break the prompt cache.",
    ),
    rule(
        "I306",
        "instructions",
        LINT,
        W,
        "AGENTS.md tells agents to use each pinned script bundle (.claude/scripts/<name>).",
    ),
    rule(
        "I308",
        "instructions",
        LINT,
        W,
        "No line in CLAUDE.md repeats one in AGENTS.md; both tools already read it there.",
    ),
    rule(
        "I309",
        "instructions",
        LINT,
        E,
        "AGENTS.md is not shadowed: a CLAUDE.md or CLAUDE.local.md here or above makes Claude Code skip it.",
    ),
];

/// Whether an allow-list (`config set validate.allow`) can silence the rule.
pub fn allow_listable(id: &str) -> bool {
    super::suppressible(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_rule_id_in_the_checks_is_catalogued_once() {
        let catalogued: Vec<&str> = RULES.iter().map(|rule| rule.id).collect();
        let unique: BTreeSet<&str> = catalogued.iter().copied().collect();
        assert_eq!(unique.len(), catalogued.len(), "duplicate IDs");
        let pattern = regex::Regex::new(r#""([SOHKI][0-9]{3})""#).unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/validate");
        let mut used = BTreeSet::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|name| name == "catalog.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            for found in pattern.captures_iter(code) {
                used.insert(found[1].to_owned());
            }
        }
        let missing: Vec<&String> = used
            .iter()
            .filter(|id| !unique.contains(id.as_str()))
            .collect();
        assert!(missing.is_empty(), "not in the catalog: {missing:?}");
        assert!(!allow_listable("S001"));
        assert!(!allow_listable("K101"));
        assert!(allow_listable("O140"));
        assert!(allow_listable("I301"));
    }
}
