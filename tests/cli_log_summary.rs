//! `skillctl log summary`: two windows over the event log, per-skill and
//! per-agent counts, alerts, `--strict`, and `--until` for reproducible runs.
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .env("HOME", home.parent().unwrap())
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

fn line(ts: &str, event: &str, rest: &str) -> String {
    format!(
        r#"{{"ts":"{ts}","tool":"claude-code","event":"{event}","session":"s-{ts}"{}{rest}}}"#,
        if rest.is_empty() { "" } else { "," }
    ) + "\n"
}

#[test]
fn summary_compares_windows_and_alerts() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    fs::create_dir_all(home.join("log")).unwrap();
    let mut log = String::new();
    // Previous week: 24 prompts with 12 activations, in 6 sessions.
    for i in 0..24 {
        let ts = format!("2026-10-{:02}T{:02}:00:00Z", 7 + i % 6, i);
        log += &line(&ts, "UserPromptSubmit", "");
        if i % 2 == 0 {
            log += &line(
                &ts,
                "PostToolUse",
                r#""tool_name":"Skill","skill":"db-migrations""#,
            );
        }
    }
    // Current week: 24 prompts with 2 activations, two raw test commands,
    // one real subagent run and one MCP call.
    for i in 0..24 {
        let ts = format!("2026-10-{:02}T{:02}:00:00Z", 14 + i % 6, i);
        log += &line(&ts, "UserPromptSubmit", "");
        if i < 2 {
            log += &line(
                &ts,
                "PostToolUse",
                r#""tool_name":"Skill","skill":"Review""#,
            );
        }
    }
    log += &line(
        "2026-10-15T01:30:00Z",
        "PostToolUse",
        r#""tool_name":"Bash","test_command":"raw""#,
    );
    log += &line(
        "2026-10-15T02:30:00Z",
        "PostToolUse",
        r#""tool_name":"Bash","test_command":"raw""#,
    );
    log += &line(
        "2026-10-15T03:30:00Z",
        "PostToolUse",
        r#""tool_name":"Bash","test_command":"wrapped""#,
    );
    log += &line(
        "2026-10-15T04:30:00Z",
        "SubagentStop",
        r#""agent":"explore","agent_id":"a1""#,
    );
    log += &line(
        "2026-10-15T05:30:00Z",
        "SubagentStop",
        r#""agent_id":"aux""#,
    );
    log += &line(
        "2026-10-15T06:30:00Z",
        "PostToolUse",
        r#""tool_name":"mcp__github__list_prs","server":"github""#,
    );
    log += "garbage line\n";
    fs::write(home.join("log/events.jsonl"), log).unwrap();

    let out = run(
        &home,
        &["--json", "log", "summary", "--until", "2026-10-20"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let summary: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(summary["days"], 7);
    assert_eq!(summary["current"]["from"], "2026-10-13T00:00:00Z");
    assert_eq!(summary["current"]["prompts"], 24);
    assert_eq!(summary["previous"]["prompts"], 24);
    assert_eq!(summary["current"]["activations"], 2);
    assert_eq!(summary["previous"]["activations"], 12);
    assert_eq!(summary["current"]["skills"]["review"], 2);
    assert_eq!(summary["previous"]["skills"]["db-migrations"], 12);
    assert_eq!(summary["current"]["subagent_runs"], 1);
    assert_eq!(summary["current"]["agents"]["explore"], 1);
    assert_eq!(summary["current"]["mcp_calls"]["github"], 1);
    assert_eq!(summary["current"]["test_commands_raw"], 2);
    assert_eq!(summary["current"]["test_commands_wrapped"], 1);
    assert_eq!(summary["current"]["sessions"], 24 + 6);
    let checks: Vec<&str> = summary["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["check"].as_str().unwrap())
        .collect();
    assert_eq!(checks, ["activation-drop", "raw-test-commands"]);
    assert!(summary["note"].as_str().unwrap().contains("ccusage"));

    let out = run(&home, &["log", "summary", "--until", "2026-10-20"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with(
            "Log summary: 2026-10-13 → 2026-10-20 (7 days), previous 2026-10-06 → 2026-10-13"
        ),
        "{text}"
    );
    assert!(
        text.contains("2 (prev 12)   8.3 per 100 prompts (prev 50)"),
        "{text}"
    );
    assert!(text.contains("    review 2"), "{text}");
    assert!(
        text.contains("[warning] skill activations fell from 50 to 8.3 per 100 prompts"),
        "{text}"
    );
    assert!(
        text.contains("[info] 2 bare test or build command(s) ran next to 1 wrapped one(s)"),
        "{text}"
    );

    // --strict turns a warning alert into exit 1; info alone does not.
    let out = run(
        &home,
        &["log", "summary", "--until", "2026-10-20", "--strict"],
    );
    assert_eq!(out.status.code(), Some(1));
    let out = run(
        &home,
        &[
            "log",
            "summary",
            "--until",
            "2026-10-27",
            "--days",
            "7",
            "--strict",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "a silent week after an active one"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("[warning] no events in the last 7 days")
    );
    let out = run(
        &home,
        &["log", "summary", "--until", "2026-10-13", "--strict"],
    );
    assert!(
        out.status.success(),
        "the first week has no previous window"
    );

    // An unreadable --until is a usage error; a missing log is an empty summary.
    let out = run(&home, &["log", "summary", "--until", "last tuesday"]);
    assert_eq!(out.status.code(), Some(2));
    fs::remove_file(home.join("log/events.jsonl")).unwrap();
    let out = run(&home, &["log", "summary"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Alerts: none"));
}
