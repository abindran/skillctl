//! Hook artifact rules K101–K111. A hook is `HOOK.md` declaring `events` and an
//! `entry` script, plus the script itself. Structural rules make the hook
//! unregistrable and cannot be allow-listed; the rest are quality lints that
//! reuse the script heuristics shared with skills. Nothing here runs the script.
use super::{Finding, Rule, Severity, io_error, org};
use crate::{
    error::Error,
    names::SkillName,
    services::fs::Fs,
    skill::{Field, Skill},
};
use serde_yaml_ng::Value;
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};

/// Seconds a harness waits for the hook when `HOOK.md` sets no `timeout`.
pub const DEFAULT_TIMEOUT: u64 = 10;
/// The longest `timeout` a hook may declare, matching Claude Code's ceiling.
pub const MAX_TIMEOUT: u64 = 600;

/// Rules an allow-list can never suppress (K101 is the runner's gate).
pub const STRUCTURAL: [&str; 6] = ["K101", "K102", "K103", "K104", "K105", "K107"];

/// Claude Code lifecycle events as documented; Cursor maps a subset of them.
/// An unknown name is a warning (K106), because the list grows with releases.
pub const KNOWN_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "Setup",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "Notification",
    "MessageDisplay",
    "SubagentStart",
    "SubagentStop",
    "TaskCreated",
    "TaskCompleted",
    "Stop",
    "StopFailure",
    "TeammateIdle",
    "InstructionsLoaded",
    "ConfigChange",
    "CwdChanged",
    "DirectoryAdded",
    "FileChanged",
    "WorktreeCreate",
    "WorktreeRemove",
    "PreCompact",
    "PostCompact",
    "PreModelSwitch",
    "PostModelSwitch",
    "Elicitation",
    "ElicitationResult",
];

pub struct HookRules;

/// The declared `events`, or `None` when the field is missing or malformed.
pub fn events(skill: &Skill) -> Option<Vec<String>> {
    let value = skill.frontmatter.extra.get("events")?;
    let items = value.as_sequence()?;
    items
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

/// The declared `entry` script, relative to the hook folder.
pub fn entry(skill: &Skill) -> Option<&str> {
    skill.frontmatter.extra.get("entry").and_then(Value::as_str)
}

/// Per-event tool-name matchers (`matchers: {PostToolUse: Skill}`), or `None`
/// when absent or malformed.
pub fn matchers(skill: &Skill) -> Option<BTreeMap<String, String>> {
    let value = skill.frontmatter.extra.get("matchers")?;
    let map = value.as_mapping()?;
    map.iter()
        .map(|(key, value)| Some((key.as_str()?.to_owned(), value.as_str()?.to_owned())))
        .collect()
}

/// The declared `timeout` in seconds, or `None` when absent or malformed.
pub fn timeout(skill: &Skill) -> Option<u64> {
    skill.frontmatter.extra.get("timeout")?.as_u64()
}

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

impl Rule for HookRules {
    fn check(&self, skill: &Skill) -> Vec<Finding> {
        let mut findings = Vec::new();
        let fields = &skill.frontmatter;
        match fields.name.value() {
            Some(name) => {
                if SkillName::new(name.clone()).is_err() {
                    findings.push(finding(
                        skill,
                        "K102",
                        false,
                        "name must match the artifact-name pattern and contain 1–64 characters",
                        "Use lowercase ASCII letters and digits separated by single hyphens.",
                    ));
                } else if skill.dir.file_name().and_then(|value| value.to_str())
                    != Some(name.as_str())
                {
                    findings.push(finding(
                        skill,
                        "K102",
                        false,
                        "name does not equal the hook folder's name",
                        "Make the frontmatter name and hook folder name identical.",
                    ));
                }
            }
            None => findings.push(finding(
                skill,
                "K102",
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
                "K103",
                false,
                "description must be a non-empty string of at most 1,024 characters",
                "Describe which events the hook handles and what it does.",
            ));
        }
        if matches!(fields.metadata, Field::Invalid(_)) || skill.version().ok().flatten().is_none()
        {
            findings.push(finding(
                skill,
                "K104",
                false,
                "metadata.version must be present and parse as semver",
                "Set metadata.version to a quoted semver string under a metadata mapping.",
            ));
        }
        match events(skill) {
            Some(events) if !events.is_empty() => {
                for event in events {
                    if !KNOWN_EVENTS.contains(&event.as_str()) {
                        findings.push(finding(
                            skill,
                            "K106",
                            true,
                            format!("Unknown hook event: {event}"),
                            "Check the event name against the harness documentation; known names are case-sensitive.",
                        ));
                    }
                }
            }
            _ => findings.push(finding(
                skill,
                "K105",
                false,
                "events must be a non-empty list of event names",
                "Declare the lifecycle events the hook handles, for example events: [SessionEnd, PreCompact].",
            )),
        }
        match entry(skill) {
            Some(entry) => {
                let path = Path::new(entry);
                if entry.trim().is_empty()
                    || path.is_absolute()
                    || path
                        .components()
                        .any(|part| !matches!(part, Component::Normal(_)))
                {
                    findings.push(finding(
                        skill,
                        "K107",
                        false,
                        format!(
                            "entry must be a relative path inside the hook folder, not {entry:?}"
                        ),
                        "Point entry at the script file, for example entry: hook.py.",
                    ));
                }
            }
            None => findings.push(finding(
                skill,
                "K107",
                false,
                "entry is required and names the script the harness runs",
                "Add entry: hook.py (or the script's relative path) to the frontmatter.",
            )),
        }
        // Registration fields: matchers name declared events; timeout is 1–600 s.
        if let Some(value) = fields.extra.get("matchers") {
            match matchers(skill) {
                Some(map) => {
                    let declared = events(skill).unwrap_or_default();
                    for event in map.keys().filter(|event| !declared.contains(event)) {
                        findings.push(finding(
                            skill,
                            "K111",
                            false,
                            format!("matchers names {event}, which is not in events"),
                            "Only events the hook declares can have a matcher.",
                        ));
                    }
                }
                None => {
                    let _ = value;
                    findings.push(finding(
                        skill,
                        "K111",
                        false,
                        "matchers must map event names to tool-name patterns (strings)",
                        "Write matchers as a mapping, for example matchers: {PostToolUse: Skill}.",
                    ));
                }
            }
        }
        if let Some(value) = fields.extra.get("timeout") {
            if !value
                .as_u64()
                .is_some_and(|seconds| (1..=MAX_TIMEOUT).contains(&seconds))
            {
                findings.push(finding(
                    skill,
                    "K111",
                    false,
                    format!("timeout must be a whole number of seconds from 1 to {MAX_TIMEOUT}"),
                    "Set timeout to the seconds the harness should wait, or remove it for the default.",
                ));
            }
        }
        findings
    }

    fn check_fs(&self, fs: &dyn Fs, skill: &Skill) -> Result<Vec<Finding>, Error> {
        let mut findings = self.check(skill);
        let mut files = Vec::new();
        org::walk(
            fs,
            &skill.dir,
            &mut files,
            &mut findings,
            "K110",
            "Symlinks are not allowed inside a hook",
            "Replace the link with a regular file inside the hook.",
        )?;
        // The entry script: a regular file with a shebang and the exec bit (K107).
        let entry_ok = !findings.iter().any(|item| item.rule == "K107");
        let entry_path = entry(skill)
            .filter(|_| entry_ok)
            .map(|entry| skill.dir.join(entry));
        if let Some(path) = &entry_path {
            let regular = files.iter().any(|file| file == path);
            if !regular {
                findings.push(finding(
                    skill,
                    "K107",
                    false,
                    format!(
                        "entry {} is not a regular file in the hook folder",
                        path.display()
                    ),
                    "Create the script at the declared entry path.",
                ));
            }
        }
        for path in files {
            let bytes = fs.read(&path).map_err(|error| io_error(&path, error))?;
            let text = String::from_utf8_lossy(&bytes);
            if entry_path.as_ref() == Some(&path) {
                let info = fs
                    .symlink_metadata(&path)
                    .map_err(|error| io_error(&path, error))?;
                if !bytes.starts_with(b"#!") || !org::executable(&info) {
                    let mut item = finding(
                        skill,
                        "K107",
                        false,
                        "entry script needs a shebang and executable permission",
                        "Add an interpreter shebang (for example #!/usr/bin/env python3) and chmod +x the script.",
                    );
                    item.path = path.clone();
                    findings.push(item);
                }
            }
            // Every script-like file is held to the skill script hygiene bar.
            let is_script = entry_path.as_ref() == Some(&path) || bytes.starts_with(b"#!");
            if is_script {
                for (offset, line) in text.lines().enumerate() {
                    if line.trim_start().starts_with('#') {
                        continue;
                    }
                    if org::interactive(line) {
                        let mut item = finding(
                            skill,
                            "K108",
                            true,
                            "Script may request interactive terminal input; hooks run unattended",
                            "Read the event from stdin and never prompt.",
                        );
                        item.path = path.clone();
                        item.line = Some(offset + 1);
                        findings.push(item);
                    }
                }
                if org::dangerous(&text) {
                    let mut item = finding(
                        skill,
                        "K108",
                        false,
                        "Script uses sudo or pipes a download into a shell",
                        "Remove privilege escalation and download-and-execute commands.",
                    );
                    item.path = path.clone();
                    findings.push(item);
                }
            }
            for (offset, line) in text.lines().enumerate() {
                if org::likely_secret(line) {
                    let mut item = finding(
                        skill,
                        "K109",
                        false,
                        "Likely secret detected (value redacted)",
                        "Remove the credential, rotate it, and use environment-based credentials.",
                    );
                    item.path = path.clone();
                    item.line = Some(offset + 1);
                    findings.push(item);
                }
            }
        }
        Ok(findings)
    }
}
