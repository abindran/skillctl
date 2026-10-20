//! The event log written by the bundled `skillctl-logger` hook:
//! `$SKILLCTL_HOME/log/events.jsonl`, one JSON object per harness lifecycle
//! event. Every reader (`artifacts`, `doctor`, `log summary`) goes through
//! this module so the field names live in one place. Lines that are not JSON
//! objects with a `ts` string are skipped, never fatal: the log is written by
//! a hook that may be interrupted mid-line.
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::path::Path;

/// One recorded event. Fields the hook did not write are `None` or empty.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Event {
    /// UTC time the hook ran, RFC 3339 with a `Z` suffix.
    pub ts: String,
    /// `claude-code` or `cursor`.
    #[serde(default)]
    pub tool: String,
    /// The event in Claude Code's spelling (`PostToolUse`, `PreCompact`, …).
    #[serde(default)]
    pub event: String,
    pub session: Option<String>,
    pub cwd: Option<String>,
    /// The skill invoked, on `PostToolUse` of the `Skill` tool.
    pub skill: Option<String>,
    /// The subagent type, on subagent events of real (non-auxiliary) agents.
    pub agent: Option<String>,
    /// The MCP server, on `PostToolUse` of an `mcp__<server>__<tool>` tool.
    pub server: Option<String>,
    pub tool_name: Option<String>,
    /// `raw` or `wrapped`, on `PostToolUse` of a shell test or build command.
    pub test_command: Option<String>,
    /// `auto` or `manual`, on compaction events.
    pub trigger: Option<String>,
    /// The harness version when the payload carries it (Cursor's `cursor_version`).
    pub harness_version: Option<String>,
    /// The skills the forced-eval hook shortlisted, on `SkillShortlist`.
    pub candidates: Option<Vec<String>>,
}

impl Event {
    pub fn time(&self) -> Option<DateTime<Utc>> {
        parse_time(&self.ts)
    }
}

/// Parse a log timestamp (RFC 3339) into UTC.
pub fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

/// Every well-formed event in the text, in file order.
pub fn parse(text: &str) -> Vec<Event> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Event>(line).ok())
        .filter(|event| !event.ts.is_empty())
        .collect()
}

/// Every well-formed event in the file; a missing or unreadable file is an
/// empty log.
pub fn read(path: &Path) -> Vec<Event> {
    std::fs::read_to_string(path)
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Whole days between the first and the last event, 0 for fewer than two.
pub fn span_days(events: &[Event]) -> i64 {
    let times: Vec<DateTime<Utc>> = events.iter().filter_map(Event::time).collect();
    match (times.iter().min(), times.iter().max()) {
        (Some(from), Some(to)) => (*to - *from).num_days(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_lines_are_skipped_and_fields_are_optional() {
        let events = parse(concat!(
            r#"{"ts":"2026-10-01T09:00:00Z","tool":"claude-code","event":"SessionStart","session":"s1"}"#,
            "\n",
            "not json\n",
            r#"{"event":"PostToolUse"}"#,
            "\n",
            r#"{"ts":null,"event":"Stop"}"#,
            "\n",
            r#"{"ts":"2026-10-03T09:00:00Z","tool":"cursor","event":"PostToolUse","session":null,"tool_name":"Bash","test_command":"raw","cursor_version":"ignored","harness_version":"3.1.0"}"#,
            "\n",
        ));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].session.as_deref(), Some("s1"));
        assert_eq!(events[1].tool, "cursor");
        assert_eq!(events[1].session, None);
        assert_eq!(events[1].test_command.as_deref(), Some("raw"));
        assert_eq!(events[1].harness_version.as_deref(), Some("3.1.0"));
        assert_eq!(span_days(&events), 2);
        assert_eq!(span_days(&events[..1]), 0);
        assert!(read(Path::new("/nonexistent/events.jsonl")).is_empty());
    }
}
