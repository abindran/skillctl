//! The bundled skillctl-logger hook: it validates clean, installs through the
//! normal path, and writes the documented record for Claude Code and Cursor
//! shaped payloads without ever failing.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn hook_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("hooks/skillctl-logger")
}

fn python() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("python3"))
        .find(|path| path.is_file())
}

fn run_hook(home: &Path, payload: &Value, cursor_env: bool) -> std::process::Output {
    let mut command = Command::new(hook_dir().join("hook.py"));
    command
        .env("SKILLCTL_HOME", home)
        .env_remove("CURSOR_VERSION")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if cursor_env {
        command.env("CURSOR_VERSION", "1.2.3");
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn lines(home: &Path) -> Vec<Value> {
    fs::read_to_string(home.join("log/events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn logger_hook_validates_and_installs() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("HOME", temp.path())
        .args(["--json", "install"])
        .arg(hook_dir())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("HOME", temp.path())
        .args(["validate", "skillctl-logger@1.2.1"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "0 error(s), 0 warning(s)"
    );
}

#[test]
fn logger_records_claude_code_and_cursor_events_and_never_fails() {
    if python().is_none() {
        eprintln!("python3 not on PATH; skipping");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");

    let out = run_hook(
        &home,
        &json!({
            "session_id": "s1", "cwd": "/work/crm", "hook_event_name": "PreCompact",
            "transcript_path": "/t/x.jsonl", "trigger": "auto", "custom_instructions": "keep decisions"
        }),
        false,
    );
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    let out = run_hook(
        &home,
        &json!({
            "session_id": "s1", "cwd": "/work/crm", "hook_event_name": "UserPromptSubmit",
            "prompt": "please refactor the payment module"
        }),
        false,
    );
    assert!(out.status.success());
    run_hook(
        &home,
        &json!({
            "session_id": "s1", "cwd": "/work/crm", "hook_event_name": "PostToolUse",
            "tool_name": "Skill", "tool_input": {"skill": "db-migrations", "args": ""}, "tool_response": "..."
        }),
        false,
    );
    run_hook(
        &home,
        &json!({
            "session_id": "s1", "cwd": "/work/crm", "hook_event_name": "SubagentStop",
            "agent_id": "a9", "agent_type": "repo-explorer", "stop_hook_active": false
        }),
        false,
    );
    run_hook(
        &home,
        &json!({
            "session_id": "s1", "cwd": "/work/crm", "hook_event_name": "PostToolUse",
            "tool_name": "mcp__github__list_pull_requests", "tool_input": {"name": "ignored"}
        }),
        false,
    );
    // Bash commands: only whether a test or build command ran raw or wrapped.
    for command in [
        "cargo test --locked",
        ".claude/scripts/test-quiet/test-quiet.sh cargo test",
        "ls -la",
        "cd crates/x && cargo test",
    ] {
        run_hook(
            &home,
            &json!({
                "session_id": "s1", "cwd": "/work/crm", "hook_event_name": "PostToolUse",
                "tool_name": "Bash", "tool_input": {"command": command, "description": "x"}
            }),
            false,
        );
    }
    // Cursor-shaped payload, camelCase event, cursor_version present.
    run_hook(
        &home,
        &json!({
            "conversation_id": "c7", "generation_id": "g1", "hook_event_name": "sessionEnd",
            "cursor_version": "1.2.3", "workspace_roots": ["/work/api"], "transcript_path": null
        }),
        false,
    );
    // Claude-shaped payload under the Cursor environment variable.
    run_hook(
        &home,
        &json!({"session_id": "s2", "cwd": "/work/api", "hook_event_name": "Stop", "stop_hook_active": true}),
        true,
    );
    // Cursor's own event names and its shell payload shape.
    run_hook(
        &home,
        &json!({
            "conversation_id": "c7", "hook_event_name": "beforeSubmitPrompt",
            "cursor_version": "3.25.9", "workspace_roots": ["/work/api"], "prompt": "hi there"
        }),
        false,
    );
    run_hook(
        &home,
        &json!({
            "conversation_id": "c7", "hook_event_name": "afterShellExecution",
            "cursor_version": "3.25.9", "workspace_roots": ["/work/api"], "command": "cargo test", "output": "..."
        }),
        false,
    );
    // Garbage and non-object input exit 0 and write nothing.
    let mut child = Command::new(hook_dir().join("hook.py"))
        .env("SKILLCTL_HOME", &home)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not json").unwrap();
    assert!(child.wait().unwrap().success());
    let out = run_hook(&home, &json!([1, 2]), false);
    assert!(out.status.success());

    let records = lines(&home);
    assert_eq!(records.len(), 13);
    let first = &records[0];
    assert_eq!(first["tool"], "claude-code");
    assert_eq!(first["event"], "PreCompact");
    assert_eq!(first["session"], "s1");
    assert_eq!(first["cwd"], "/work/crm");
    assert_eq!(first["transcript"], "/t/x.jsonl");
    assert_eq!(first["trigger"], "auto");
    assert!(first.get("custom_instructions").is_none());
    assert!(first["ts"].as_str().unwrap().ends_with('Z'));
    assert_eq!(records[1]["prompt_chars"], 34);
    assert!(records[1].get("prompt").is_none());
    assert_eq!(records[2]["tool_name"], "Skill");
    assert_eq!(records[2]["skill"], "db-migrations");
    assert!(records[2].get("tool_response").is_none());
    assert_eq!(records[3]["agent"], "repo-explorer");
    assert_eq!(records[3]["agent_id"], "a9");
    assert_eq!(records[3]["stop_hook_active"], false);
    assert_eq!(records[4]["tool_name"], "mcp__github__list_pull_requests");
    assert_eq!(records[4]["server"], "github");
    assert!(
        records[4].get("skill").is_none(),
        "name only counts for the Skill tool"
    );
    assert_eq!(records[5]["tool_name"], "Bash");
    assert_eq!(records[5]["test_command"], "raw");
    assert!(
        records[5].get("command").is_none(),
        "command text is never logged"
    );
    assert_eq!(records[6]["test_command"], "wrapped");
    assert!(
        records[7].get("test_command").is_none(),
        "ls is not a test command"
    );
    assert!(
        records[8].get("test_command").is_none(),
        "compound lines are not classified"
    );
    assert_eq!(records[9]["tool"], "cursor");
    assert_eq!(records[9]["event"], "SessionEnd");
    assert_eq!(records[9]["session"], "c7");
    assert_eq!(records[9]["cwd"], "/work/api");
    assert!(records[9].get("transcript").is_none());
    assert_eq!(records[10]["tool"], "cursor");
    assert_eq!(records[10]["event"], "Stop");
    assert_eq!(records[10]["stop_hook_active"], true);
    assert_eq!(records[9]["harness_version"], "1.2.3");
    assert_eq!(records[11]["event"], "UserPromptSubmit");
    assert_eq!(records[11]["prompt_chars"], 8);
    assert_eq!(records[11]["harness_version"], "3.25.9");
    assert_eq!(records[12]["event"], "PostToolUse");
    assert_eq!(records[12]["tool_name"], "Shell");
    assert_eq!(records[12]["test_command"], "raw");
    assert!(records[12].get("command").is_none());
}
