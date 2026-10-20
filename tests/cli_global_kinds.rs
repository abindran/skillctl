//! Global commands and agents: `global add` links them into the user-level
//! folders both harnesses read, `global remove` sweeps only those links,
//! rules stay project-level, and the log's slash commands count as uses.
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Env {
    _temp: tempfile::TempDir,
    home: PathBuf,
    user_home: PathBuf,
    repo: PathBuf,
}

fn env() -> Env {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let home = base.join("skills");
    let user_home = base.join("userhome");
    let repo = base.join("repo");
    fs::create_dir_all(&user_home).unwrap();
    fs::create_dir(&repo).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap()
            .success()
    );
    Env {
        _temp: temp,
        home,
        user_home,
        repo,
    }
}

fn run(e: &Env, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &e.home)
        .env("HOME", &e.user_home)
        .current_dir(&e.repo)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

fn ok(e: &Env, args: &[&str]) -> Output {
    let out = run(e, args);
    assert!(
        out.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap()
}

fn pinned_by(e: &Env, name: &str) -> Vec<String> {
    json(&ok(e, &["--json", "artifacts"]))
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .map(|row| {
            row["pinned_by"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn link(path: &Path) -> Option<PathBuf> {
    fs::read_link(path).ok()
}

#[test]
fn commands_and_agents_can_be_global_and_rules_cannot() {
    let e = env();
    ok(&e, &["new", "--kind", "command", "checkpoint"]);
    ok(&e, &["seal", "checkpoint"]);
    ok(&e, &["new", "--kind", "agent", "scout"]);
    ok(&e, &["agent", "derive", "scout", "--model", "gpt-5"]);
    ok(&e, &["seal", "scout"]);
    ok(&e, &["new", "--kind", "rule", "style"]);
    ok(&e, &["seal", "style"]);
    // A file the user owns in a user-level folder is never touched.
    fs::create_dir_all(e.user_home.join(".claude/commands")).unwrap();
    fs::write(e.user_home.join(".claude/commands/mine.md"), "# mine\n").unwrap();

    ok(&e, &["global", "add", "checkpoint"]);
    ok(&e, &["global", "add", "scout"]);
    let store = e.home.join("store");
    assert_eq!(
        link(&e.user_home.join(".claude/commands/checkpoint.md")),
        Some(store.join("checkpoint/0.1.0/checkpoint/COMMAND.md"))
    );
    assert_eq!(
        link(&e.user_home.join(".cursor/commands/checkpoint.md")),
        Some(store.join("checkpoint/0.1.0/checkpoint/COMMAND.md"))
    );
    assert_eq!(
        link(&e.user_home.join(".claude/agents/scout.md")),
        Some(store.join("scout/0.1.0/scout/AGENT.md"))
    );
    assert_eq!(
        link(&e.user_home.join(".cursor/agents/scout.md")),
        Some(store.join("scout/0.1.0/scout/cursor.md")),
        "the Cursor override is linked for Cursor"
    );
    assert!(
        !e.user_home.join(".claude/skills/checkpoint").exists(),
        "never linked as a skill folder"
    );
    assert_eq!(pinned_by(&e, "checkpoint"), ["global"]);

    let list = json(&ok(&e, &["--json", "global", "list"]));
    let rows = list.as_array().unwrap();
    let command = rows.iter().find(|r| r["name"] == "checkpoint").unwrap();
    assert_eq!(command["kind"], "command");
    let statuses: Vec<&str> = command["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["ok", "ok"]);

    let doctor = json(&run(&e, &["--json", "doctor"]));
    assert_eq!(doctor["remaining"], 0, "{}", doctor["problems"]);
    fs::remove_file(e.user_home.join(".claude/agents/scout.md")).unwrap();
    let doctor = json(&run(&e, &["--json", "doctor"]));
    let checks: Vec<&str> = doctor["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["check"].as_str().unwrap())
        .collect();
    assert_eq!(checks, ["link-missing"], "{}", doctor["problems"]);
    ok(&e, &["doctor", "--fix"]);
    assert!(
        link(&e.user_home.join(".claude/agents/scout.md")).is_some(),
        "doctor --fix relinks global agents"
    );

    // Rules and hooks are refused with the reason.
    let out = run(&e, &["global", "add", "style"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("project-level"));

    // A global artifact is not pinned into projects, and the other way round.
    ok(&e, &["attach", "--new", "web", "--skills", "style"]);
    ok(&e, &["new", "--kind", "command", "deploy"]);
    ok(&e, &["seal", "deploy"]);
    ok(&e, &["add", "deploy"]);
    let out = run(&e, &["global", "add", "deploy"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("pinned by a project"));

    // Slash-command runs in the log count as uses.
    fs::create_dir_all(e.home.join("log")).unwrap();
    fs::write(
        e.home.join("log/events.jsonl"),
        "{\"ts\":\"2026-10-20T09:00:00Z\",\"tool\":\"claude-code\",\"event\":\"UserPromptSubmit\",\"session\":\"s\",\"command\":\"checkpoint\"}\n",
    )
    .unwrap();
    let rows = json(&ok(&e, &["--json", "artifacts"]));
    let checkpoint = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "checkpoint")
        .unwrap();
    assert_eq!(checkpoint["uses"], 1);
    assert_eq!(checkpoint["last_used"], "2026-10-20T09:00:00Z");
    let deploy = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "deploy")
        .unwrap();
    assert_eq!(
        deploy["uses"], 0,
        "commands are no longer invisible to the log"
    );

    // Removing sweeps exactly the managed links.
    ok(&e, &["global", "remove", "checkpoint"]);
    assert!(!e.user_home.join(".claude/commands/checkpoint.md").exists());
    assert!(!e.user_home.join(".cursor/commands/checkpoint.md").exists());
    assert!(
        link(&e.user_home.join(".claude/agents/scout.md")).is_some(),
        "other globals stay"
    );
    assert_eq!(
        fs::read_to_string(e.user_home.join(".claude/commands/mine.md")).unwrap(),
        "# mine\n"
    );
    assert!(pinned_by(&e, "checkpoint").is_empty());
}
