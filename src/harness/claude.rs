//! Merge skillctl's hook entries into Claude Code's `settings.json` without
//! disturbing anything else in it. The file is parsed as generic JSON, so keys
//! skillctl does not know about round-trip untouched; comments would not, which
//! is why an unparsable file is a hard stop rather than an overwrite.
//!
//! An entry is recognised as skillctl's by its command path, which always runs
//! through the per-hook link folder (`~/.claude/hooks/<name>/`). That is what
//! lets `unregister` remove only our entries and `register` stay idempotent.
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{error::Error, names::SkillName, services::fs::Fs};

/// One `settings.json` entry to ensure for a hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub event: String,
    /// Tool-name regex for tool events; omitted for lifecycle events.
    pub matcher: Option<String>,
    pub timeout: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Outcome {
    pub event: String,
    /// `added`, `updated` (matcher, timeout or entry changed), `present` or `removed`.
    pub status: &'static str,
}

/// The shell command Claude Code runs: through the stable link folder, so
/// upgrading the hook repoints the link and never rewrites settings. `$HOME`
/// is expanded by the shell the harness uses, which keeps a synced dotfiles
/// copy of the file portable.
pub fn command(name: &SkillName, entry: &str) -> String {
    format!("\"$HOME/.claude/hooks/{name}/{entry}\"")
}

/// The substring every command of this hook contains.
fn marker(name: &SkillName) -> String {
    format!("/.claude/hooks/{name}/")
}

fn is_ours(item: &Value, marker: &str) -> bool {
    item.get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| command.contains(marker))
}

/// Read the settings file: missing is an empty object, anything that is not a
/// JSON object is refused so a hand-edited file is never clobbered.
pub fn load(fs: &dyn Fs, path: &Path) -> Result<Value, Error> {
    match fs.read(path) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes).map_err(|error| Error::Usage {
                message: format!("{} is not valid JSON: {error}", path.display()),
                hint: "Fix the file by hand (Claude Code settings are plain JSON without comments); skillctl never overwrites a file it cannot parse.".into(),
            })?;
            if !value.is_object() {
                return Err(Error::Usage {
                    message: format!("{} must contain a JSON object", path.display()),
                    hint: "Replace the file's top level with an object, for example {}.".into(),
                });
            }
            Ok(value)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(json!({})),
        Err(source) => Err(io_error(path, source)),
    }
}

/// Write the settings back: the previous bytes go to `<file>.bak` first, the
/// parent folder is created when missing, and the write is atomic.
pub fn save(fs: &dyn Fs, path: &Path, value: &Value) -> Result<PathBuf, Error> {
    let backup = backup_path(path);
    match fs.read(path) {
        Ok(previous) => fs
            .write_atomic(&backup, &previous)
            .map_err(|source| io_error(&backup, source))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                match fs.symlink_metadata(parent) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        fs.create_dir(parent)
                            .map_err(|source| io_error(parent, source))?;
                    }
                    Err(source) => return Err(io_error(parent, source)),
                    Ok(_) => {}
                }
            }
        }
        Err(source) => return Err(io_error(path, source)),
    }
    let mut text = serde_json::to_string_pretty(value).map_err(|error| Error::Io {
        operation: format!("Serialize {}", path.display()),
        source: io::Error::new(io::ErrorKind::InvalidData, error),
        hint: "Report this as a skillctl bug.".into(),
    })?;
    text.push('\n');
    fs.write_atomic(path, text.as_bytes())
        .map_err(|source| io_error(path, source))?;
    Ok(backup)
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(".bak");
    path.with_file_name(name)
}

/// Events under which this hook currently has at least one entry.
pub fn registered_events(value: &Value, name: &SkillName) -> BTreeSet<String> {
    let marker = marker(name);
    let mut events = BTreeSet::new();
    if let Some(hooks) = value.get("hooks").and_then(Value::as_object) {
        for (event, groups) in hooks {
            let present = groups.as_array().is_some_and(|groups| {
                groups.iter().any(|group| {
                    group
                        .get("hooks")
                        .and_then(Value::as_array)
                        .is_some_and(|items| items.iter().any(|item| is_ours(item, &marker)))
                })
            });
            if present {
                events.insert(event.clone());
            }
        }
    }
    events
}

/// Events under which this hook's command appears more than once (it would
/// run twice per event).
pub fn duplicated_events(value: &Value, name: &SkillName) -> Vec<String> {
    let marker = marker(name);
    let mut events = Vec::new();
    if let Some(hooks) = value.get("hooks").and_then(Value::as_object) {
        for (event, groups) in hooks {
            let count = groups
                .as_array()
                .map(|groups| {
                    groups
                        .iter()
                        .flat_map(|group| {
                            group
                                .get("hooks")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                        })
                        .filter(|item| is_ours(item, &marker))
                        .count()
                })
                .unwrap_or(0);
            if count > 1 {
                events.push(event.clone());
            }
        }
    }
    events
}

/// Whether any command in a hooks file (Claude or Cursor shaped) is this hook's.
pub fn mentions(value: &Value, name: &SkillName) -> bool {
    fn walk(value: &Value, marker: &str) -> bool {
        match value {
            Value::Object(map) => map.iter().any(|(key, item)| {
                (key == "command" && item.as_str().is_some_and(|c| c.contains(marker)))
                    || walk(item, marker)
            }),
            Value::Array(items) => items.iter().any(|item| walk(item, marker)),
            _ => false,
        }
    }
    walk(value, &marker(name))
}

/// Ensure one entry per registration. Existing entries of this hook under an
/// event are left exactly as they are, so re-running changes nothing.
pub fn register(
    value: &mut Value,
    name: &SkillName,
    entry: &str,
    registrations: &[Registration],
) -> Result<Vec<Outcome>, Error> {
    let already = registered_events(value, name);
    let root = value.as_object_mut().expect("settings object");
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks.as_object_mut().ok_or_else(|| Error::Usage {
        message: "settings.json has a \"hooks\" key that is not an object".into(),
        hint: "Make \"hooks\" an object keyed by event name, as Claude Code expects.".into(),
    })?;
    let marker = marker(name);
    let wanted = command(name, entry);
    let mut outcomes = Vec::new();
    for registration in registrations {
        if already.contains(&registration.event) {
            // Bring an existing entry in line with the hook's current
            // matcher, timeout and entry script; report whether that changed.
            let mut changed = false;
            if let Some(groups) = hooks
                .get_mut(&registration.event)
                .and_then(Value::as_array_mut)
            {
                for group in groups.iter_mut() {
                    let ours = group
                        .get("hooks")
                        .and_then(Value::as_array)
                        .is_some_and(|items| items.iter().any(|item| is_ours(item, &marker)));
                    if !ours {
                        continue;
                    }
                    let Some(object) = group.as_object_mut() else {
                        continue;
                    };
                    match &registration.matcher {
                        Some(matcher) => {
                            if object.get("matcher").and_then(Value::as_str) != Some(matcher) {
                                object.insert("matcher".into(), Value::String(matcher.clone()));
                                changed = true;
                            }
                        }
                        None => {
                            if object.remove("matcher").is_some() {
                                changed = true;
                            }
                        }
                    }
                    if let Some(items) = object.get_mut("hooks").and_then(Value::as_array_mut) {
                        for item in items.iter_mut().filter(|item| is_ours(item, &marker)) {
                            if let Some(item) = item.as_object_mut() {
                                if item.get("timeout").and_then(Value::as_u64)
                                    != Some(registration.timeout)
                                {
                                    item.insert("timeout".into(), json!(registration.timeout));
                                    changed = true;
                                }
                                if item.get("command").and_then(Value::as_str) != Some(&wanted) {
                                    item.insert("command".into(), Value::String(wanted.clone()));
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
            outcomes.push(Outcome {
                event: registration.event.clone(),
                status: if changed { "updated" } else { "present" },
            });
            continue;
        }
        let groups = hooks
            .entry(registration.event.clone())
            .or_insert_with(|| Value::Array(Vec::new()));
        let groups = groups.as_array_mut().ok_or_else(|| Error::Usage {
            message: format!("settings.json hooks.{} is not an array", registration.event),
            hint: "Make each event an array of matcher groups, as Claude Code expects.".into(),
        })?;
        let mut group = Map::new();
        if let Some(matcher) = &registration.matcher {
            group.insert("matcher".into(), Value::String(matcher.clone()));
        }
        group.insert(
            "hooks".into(),
            json!([{
                "type": "command",
                "command": command(name, entry),
                "timeout": registration.timeout,
            }]),
        );
        groups.push(Value::Object(group));
        outcomes.push(Outcome {
            event: registration.event.clone(),
            status: "added",
        });
    }
    Ok(outcomes)
}

/// Remove every entry of this hook, then prune groups, events and the `hooks`
/// object that became empty, so the file returns to its previous shape.
pub fn unregister(value: &mut Value, name: &SkillName) -> Vec<Outcome> {
    let marker = marker(name);
    let mut outcomes = Vec::new();
    let Some(hooks) = value.get_mut("hooks").and_then(Value::as_object_mut) else {
        return outcomes;
    };
    let events: Vec<String> = hooks.keys().cloned().collect();
    for event in events {
        let Some(groups) = hooks.get_mut(&event).and_then(Value::as_array_mut) else {
            continue;
        };
        let mut removed = false;
        for group in groups.iter_mut() {
            if let Some(items) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = items.len();
                items.retain(|item| !is_ours(item, &marker));
                removed |= items.len() != before;
            }
        }
        if removed {
            groups.retain(|group| {
                group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_none_or(|items| !items.is_empty())
            });
            if groups.is_empty() {
                hooks.remove(&event);
            }
            outcomes.push(Outcome {
                event,
                status: "removed",
            });
        }
    }
    if hooks.is_empty() {
        value
            .as_object_mut()
            .expect("settings object")
            .remove("hooks");
    }
    outcomes
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Update {}", path.display()),
        source,
        hint: "Check the settings file and its folder permissions.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name() -> SkillName {
        "logger".parse().unwrap()
    }
    fn regs() -> Vec<Registration> {
        vec![
            Registration {
                event: "SessionEnd".into(),
                matcher: None,
                timeout: 10,
            },
            Registration {
                event: "PostToolUse".into(),
                matcher: Some("Skill".into()),
                timeout: 10,
            },
        ]
    }

    #[test]
    fn register_merges_without_touching_other_keys_and_is_idempotent() {
        let mut value = json!({
            "permissions": {"allow": ["Bash(git *)"]},
            "hooks": {"SessionEnd": [{"hooks": [{"type": "command", "command": "say done"}]}]}
        });
        let outcomes = register(&mut value, &name(), "hook.py", &regs()).unwrap();
        assert_eq!(
            outcomes.iter().map(|o| o.status).collect::<Vec<_>>(),
            ["added", "added"]
        );
        assert_eq!(value["permissions"]["allow"][0], "Bash(git *)");
        assert_eq!(
            value["hooks"]["SessionEnd"][0]["hooks"][0]["command"],
            "say done"
        );
        assert_eq!(
            value["hooks"]["SessionEnd"][1]["hooks"][0]["command"],
            "\"$HOME/.claude/hooks/logger/hook.py\""
        );
        assert!(value["hooks"]["SessionEnd"][1].get("matcher").is_none());
        assert_eq!(value["hooks"]["PostToolUse"][0]["matcher"], "Skill");
        assert_eq!(value["hooks"]["PostToolUse"][0]["hooks"][0]["timeout"], 10);
        let snapshot = value.clone();
        let outcomes = register(&mut value, &name(), "hook.py", &regs()).unwrap();
        assert_eq!(
            outcomes.iter().map(|o| o.status).collect::<Vec<_>>(),
            ["present", "present"]
        );
        assert_eq!(value, snapshot);
        assert_eq!(
            registered_events(&value, &name()),
            ["PostToolUse", "SessionEnd"]
                .into_iter()
                .map(String::from)
                .collect()
        );
    }

    #[test]
    fn register_updates_a_changed_matcher_timeout_or_entry_in_place() {
        let mut value = json!({});
        register(&mut value, &name(), "hook.py", &regs()).unwrap();
        let mut changed = regs();
        changed[1].matcher = Some("Skill|mcp__.*".into());
        changed[0].timeout = 5;
        let outcomes = register(&mut value, &name(), "run.py", &changed).unwrap();
        assert_eq!(
            outcomes.iter().map(|o| o.status).collect::<Vec<_>>(),
            ["updated", "updated"]
        );
        assert_eq!(value["hooks"]["PostToolUse"][0]["matcher"], "Skill|mcp__.*");
        assert_eq!(value["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"], 5);
        assert_eq!(
            value["hooks"]["SessionEnd"][0]["hooks"][0]["command"],
            "\"$HOME/.claude/hooks/logger/run.py\""
        );
        // Still exactly one entry per event, and a repeat is a no-op.
        assert_eq!(value["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
        let outcomes = register(&mut value, &name(), "run.py", &changed).unwrap();
        assert!(outcomes.iter().all(|o| o.status == "present"));
        // Dropping a matcher removes the key.
        changed[1].matcher = None;
        register(&mut value, &name(), "run.py", &changed).unwrap();
        assert!(value["hooks"]["PostToolUse"][0].get("matcher").is_none());
    }

    #[test]
    fn unregister_removes_only_ours_and_prunes_empties() {
        let mut value = json!({"model": "sonnet"});
        register(&mut value, &name(), "hook.py", &regs()).unwrap();
        value["hooks"]["SessionEnd"]
            .as_array_mut()
            .unwrap()
            .push(json!({"hooks": [{"type": "command", "command": "say done"}]}));
        let outcomes = unregister(&mut value, &name());
        assert_eq!(
            outcomes
                .iter()
                .map(|o| o.event.as_str())
                .collect::<Vec<_>>(),
            ["PostToolUse", "SessionEnd"]
        );
        assert_eq!(
            value,
            json!({"model": "sonnet", "hooks": {"SessionEnd": [{"hooks": [{"type": "command", "command": "say done"}]}]}})
        );
        unregister(&mut value, &name());
        let mut only_ours = json!({});
        register(&mut only_ours, &name(), "hook.py", &regs()).unwrap();
        unregister(&mut only_ours, &name());
        assert_eq!(only_ours, json!({}));
    }

    #[test]
    fn duplicates_and_mentions_are_detected() {
        let mut value = json!({});
        register(&mut value, &name(), "hook.py", &regs()).unwrap();
        assert!(duplicated_events(&value, &name()).is_empty());
        let group = value["hooks"]["SessionEnd"][0].clone();
        value["hooks"]["SessionEnd"]
            .as_array_mut()
            .unwrap()
            .push(group);
        assert_eq!(duplicated_events(&value, &name()), ["SessionEnd"]);
        assert!(mentions(&value, &name()));
        let cursor = json!({"version": 1, "hooks": {"sessionEnd": [{"command": "\"$HOME/.claude/hooks/logger/hook.py\""}]}});
        assert!(mentions(&cursor, &name()));
        assert!(!mentions(&json!({"hooks": {}}), &name()));
    }

    #[test]
    fn load_refuses_invalid_json_and_non_objects() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        assert_eq!(
            load(&crate::services::fs::RealFs, &path).unwrap(),
            json!({})
        );
        std::fs::write(&path, "{ not json").unwrap();
        assert!(matches!(
            load(&crate::services::fs::RealFs, &path),
            Err(Error::Usage { .. })
        ));
        std::fs::write(&path, "[]").unwrap();
        assert!(load(&crate::services::fs::RealFs, &path).is_err());
    }

    #[test]
    fn save_backs_up_previous_bytes_and_creates_the_folder() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".claude/settings.json");
        let fs = crate::services::fs::RealFs;
        let backup = save(&fs, &path, &json!({"a": 1})).unwrap();
        assert_eq!(backup, temp.path().join(".claude/settings.json.bak"));
        assert!(!backup.exists());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\n  \"a\": 1\n}\n"
        );
        save(&fs, &path, &json!({"a": 2})).unwrap();
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "{\n  \"a\": 1\n}\n"
        );
    }
}
