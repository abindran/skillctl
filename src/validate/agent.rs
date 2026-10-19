//! Subagent artifact rules H220–H227. An agent is `AGENT.md`: Claude Code's
//! frontmatter (`name`, `description`, `tools`, `model`, `skills`, `maxTurns`,
//! …) plus Cursor's `readonly`, with the system prompt as the body; optionally
//! a `cursor.md` override for Cursor and a `schema.json` describing the JSON
//! the agent must return. The rules catch what the strategy saw go wrong
//! across the two harnesses: an agent Cursor would run unscoped, a Claude-only
//! model alias with no override, and a stale override.
use super::{Finding, Rule, Severity, io_error};
use crate::{error::Error, linker::AGENT_CURSOR_OVERRIDE, services::fs::Fs, skill::Skill};
use serde_yaml_ng::Value;

/// Rules an allow-list can never suppress (H220 is the runner's gate).
pub const STRUCTURAL: [&str; 4] = ["H220", "H221", "H223", "H225"];
/// Model names Claude Code resolves itself and Cursor does not know.
pub const CLAUDE_ALIASES: [&str; 5] = ["sonnet", "opus", "haiku", "fable", "inherit"];
pub const SCHEMA_FILE: &str = "schema.json";

pub struct AgentRules;

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

fn string_or_list(value: &Value) -> bool {
    value.as_str().is_some()
        || value
            .as_sequence()
            .is_some_and(|items| items.iter().all(|item| item.as_str().is_some()))
}

/// Preloaded skill names from `skills`, when well-formed.
pub fn skills(skill: &Skill) -> Vec<String> {
    skill
        .frontmatter
        .extra
        .get("skills")
        .and_then(|value| match value {
            Value::Sequence(items) => Some(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            ),
            Value::String(text) => Some(
                text.split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

pub fn model(skill: &Skill) -> Option<&str> {
    skill.frontmatter.extra.get("model").and_then(Value::as_str)
}

impl Rule for AgentRules {
    fn check(&self, skill: &Skill) -> Vec<Finding> {
        let mut findings = Vec::new();
        let fields = &skill.frontmatter;
        let folder = skill.dir.file_name().and_then(|v| v.to_str()).unwrap_or("");
        match fields.name.value() {
            Some(name) if name.is_empty() || name.contains(':') || name.starts_with('-') => {
                findings.push(finding(
                    skill,
                    "H221",
                    false,
                    "name must be non-empty, must not contain ':' and must not start with '-'; Claude Code skips the file otherwise",
                    "Use a plain name such as repo-explorer; a built-in name such as Explore overrides the built-in agent.",
                ))
            }
            Some(name) if !name.eq_ignore_ascii_case(folder) => findings.push(finding(
                skill,
                "H221",
                false,
                format!("name {name:?} does not match the agent folder {folder:?} (case-insensitively)"),
                "Keep the folder name lowercase and the frontmatter name equal to it apart from case, so the link .claude/agents/<folder>.md is the agent Claude Code reports as agent_type.",
            )),
            Some(_) => {}
            None => findings.push(finding(
                skill,
                "H221",
                false,
                "name is required and must be a string",
                "Set name; hooks receive it as agent_type.",
            )),
        }
        if !fields
            .description
            .value()
            .is_some_and(|value| !value.trim().is_empty() && value.chars().count() <= 1024)
        {
            findings.push(finding(
                skill,
                "H223",
                false,
                "description must be a non-empty string of at most 1,024 characters; it is how the main agent decides to delegate",
                "Say what the agent does and when to use it, for example: Use proactively for \"where is X\" questions before edits.",
            ));
        }
        if skill.version().ok().flatten().is_none() {
            findings.push(finding(
                skill,
                "H223",
                false,
                "metadata.version must be present and parse as semver",
                "Set metadata.version to a quoted semver string under a metadata mapping.",
            ));
        }
        if skill.body.trim().is_empty() {
            findings.push(finding(
                skill,
                "H223",
                false,
                "the agent body is empty, so the subagent has no system prompt",
                "Write the system prompt below the frontmatter, including the output contract.",
            ));
        }
        for key in ["tools", "disallowedTools"] {
            if let Some(value) = fields.extra.get(key) {
                if !string_or_list(value) {
                    findings.push(finding(
                        skill,
                        "H225",
                        false,
                        format!("{key} must be a comma-separated string or a list of tool names"),
                        "Write tools: Read, Grep, Glob or a YAML list.",
                    ));
                }
            }
        }
        if let Some(value) = fields.extra.get("readonly") {
            if !value.is_bool() {
                findings.push(finding(
                    skill,
                    "H225",
                    false,
                    "readonly must be true or false",
                    "Write readonly: true for an agent that must never edit (Cursor honours this field; Claude Code honours tools).",
                ));
            }
        }
        if let Some(value) = fields.extra.get("skills") {
            if !string_or_list(value) {
                findings.push(finding(
                    skill,
                    "H225",
                    false,
                    "skills must be a list of skill names",
                    "Write skills: [project-architecture].",
                ));
            }
        }
        if let Some(value) = fields.extra.get("maxTurns") {
            if value.as_u64().is_none_or(|turns| turns == 0) {
                findings.push(finding(
                    skill,
                    "H225",
                    false,
                    "maxTurns must be a positive integer",
                    "Write maxTurns: 15 or remove it.",
                ));
            }
        }
        if let Some(value) = fields.extra.get("model") {
            if value.as_str().is_none() {
                findings.push(finding(
                    skill,
                    "H225",
                    false,
                    "model must be a string",
                    "Write model: haiku, sonnet, opus, inherit or a full model id.",
                ));
            }
        }
        let scoped =
            fields.extra.contains_key("tools") || fields.extra.contains_key("disallowedTools");
        let readonly = fields
            .extra
            .get("readonly")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if scoped && !readonly {
            findings.push(finding(
                skill,
                "H224",
                true,
                "tools scope the agent in Claude Code only; Cursor ignores tools and would run this agent with every tool",
                "Add readonly: true (the only scope Cursor honours) when the agent must not edit, or accept that Cursor runs it unscoped.",
            ));
        }
        findings
    }

    fn check_fs(&self, fs: &dyn Fs, skill: &Skill) -> Result<Vec<Finding>, Error> {
        let mut findings = self.check(skill);
        let cursor = skill.dir.join(AGENT_CURSOR_OVERRIDE);
        let has_override = fs
            .symlink_metadata(&cursor)
            .is_ok_and(|info| info.is_file());
        if let Some(model) = model(skill) {
            if CLAUDE_ALIASES.contains(&model) && !has_override {
                findings.push(finding(
                    skill,
                    "H222",
                    true,
                    format!("model {model:?} is a Claude Code alias that Cursor does not know; Cursor would run this agent on its default model"),
                    "Run skillctl agent derive <name> --model <cursor model id> to add a cursor.md override, or set a full model id both tools accept.",
                ));
            }
        }
        if has_override {
            let bytes = fs.read(&cursor).map_err(|error| io_error(&cursor, error))?;
            let text = String::from_utf8_lossy(&bytes);
            let stale = match crate::skill::frontmatter::parse(&text) {
                Ok((frontmatter, body)) => {
                    let name_differs = frontmatter.name.value() != skill.frontmatter.name.value();
                    // The override may prepend preload instructions; the agent's
                    // own prompt must still be there verbatim.
                    let body_missing = !body.contains(skill.body.trim());
                    name_differs || body_missing
                }
                Err(_) => true,
            };
            if stale {
                let mut item = finding(
                    skill,
                    "H226",
                    true,
                    "cursor.md no longer matches AGENT.md (different name, or the agent's prompt is not contained in it)",
                    "Regenerate it with skillctl agent derive <name> --model <cursor model id> after editing AGENT.md.",
                );
                item.path = cursor.clone();
                findings.push(item);
            }
        }
        let schema = skill.dir.join(SCHEMA_FILE);
        if fs
            .symlink_metadata(&schema)
            .is_ok_and(|info| info.is_file())
        {
            let bytes = fs.read(&schema).map_err(|error| io_error(&schema, error))?;
            let valid = serde_json::from_slice::<serde_json::Value>(&bytes)
                .is_ok_and(|value| value.is_object());
            if !valid {
                let mut item = finding(
                    skill,
                    "H227",
                    false,
                    "schema.json is not a JSON object",
                    "Write the JSON Schema (type, required, properties) the agent's final message must satisfy.",
                );
                item.path = schema;
                findings.push(item);
            }
        }
        Ok(findings)
    }
}
