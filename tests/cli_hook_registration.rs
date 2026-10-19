//! `hook add|remove|list` against an isolated HOME: registry pins, the
//! ~/.claude/hooks link, the settings.json merge with backup, idempotency,
//! removal, refusal paths, doctor's registration checks and gc's pin set.
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Env {
    _temp: tempfile::TempDir,
    home: PathBuf,
    user_home: PathBuf,
    work: PathBuf,
}

fn env() -> Env {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let user_home = temp.path().join("userhome");
    let work = temp.path().join("work");
    fs::create_dir_all(&user_home).unwrap();
    fs::create_dir(&work).unwrap();
    Env {
        _temp: temp,
        home,
        user_home,
        work,
    }
}

fn run(e: &Env, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &e.home)
        .env("HOME", &e.user_home)
        .current_dir(&e.work)
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
fn settings_path(e: &Env) -> PathBuf {
    e.user_home.join(".claude/settings.json")
}
fn settings(e: &Env) -> Value {
    serde_json::from_slice(&fs::read(settings_path(e)).unwrap()).unwrap()
}
fn our_commands(value: &Value, event: &str) -> Vec<String> {
    value["hooks"][event]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["hooks"].as_array().cloned().unwrap_or_default())
        .filter_map(|item| item["command"].as_str().map(str::to_owned))
        .filter(|command| command.contains("/.claude/hooks/"))
        .collect()
}

/// A sealed hook from the built-in template plus a matcher and a timeout.
fn seal_logger(e: &Env) {
    ok(e, &["new", "--kind", "hook", "logger"]);
    // `new` re-serialises the frontmatter, so write the document outright.
    fs::write(
        e.home.join("drafts/logger/HOOK.md"),
        "---\nname: logger\ndescription: Appends one JSON line per lifecycle event so sessions can be compared across tools.\nmetadata:\n  version: '0.1.0'\nevents: [SessionEnd, PostToolUse, PreCompact]\nmatchers:\n  PostToolUse: Skill\ntimeout: 7\nentry: hook.py\n---\n# logger\n",
    )
    .unwrap();
    ok(e, &["seal", "logger"]);
}

#[test]
fn hook_add_links_registers_merges_backs_up_and_is_idempotent() {
    let e = env();
    seal_logger(&e);
    // Pre-existing settings with a hand-written hook on one of our events.
    fs::create_dir_all(e.user_home.join(".claude")).unwrap();
    let existing = json!({
        "permissions": {"allow": ["Bash(git *)"]},
        "model": "sonnet",
        "hooks": {"SessionEnd": [{"hooks": [{"type": "command", "command": "say done"}]}]}
    });
    fs::write(
        settings_path(&e),
        serde_json::to_string_pretty(&existing).unwrap(),
    )
    .unwrap();

    let out = ok(&e, &["--json", "hook", "add", "logger"]);
    let report = json(&out);
    assert_eq!(report["hook"], "logger");
    assert_eq!(report["hooks"]["logger"], "0.1.0");
    let registered: Vec<(&str, &str)> = report["registered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| (o["event"].as_str().unwrap(), o["status"].as_str().unwrap()))
        .collect();
    assert_eq!(
        registered,
        [
            ("SessionEnd", "added"),
            ("PostToolUse", "added"),
            ("PreCompact", "added")
        ]
    );
    // The link points into the store.
    let link = e.user_home.join(".claude/hooks/logger");
    assert!(link.is_symlink());
    assert_eq!(
        fs::canonicalize(&link).unwrap(),
        fs::canonicalize(e.home.join("store/logger/0.1.0/logger")).unwrap()
    );
    // Settings were merged, not replaced; the backup holds the previous bytes.
    let after = settings(&e);
    assert_eq!(after["permissions"], existing["permissions"]);
    assert_eq!(after["model"], "sonnet");
    assert_eq!(
        after["hooks"]["SessionEnd"][0]["hooks"][0]["command"],
        "say done"
    );
    assert_eq!(
        our_commands(&after, "SessionEnd"),
        ["\"$HOME/.claude/hooks/logger/hook.py\""]
    );
    assert_eq!(after["hooks"]["PostToolUse"][0]["matcher"], "Skill");
    assert!(after["hooks"]["PreCompact"][0].get("matcher").is_none());
    assert_eq!(after["hooks"]["PreCompact"][0]["hooks"][0]["timeout"], 7);
    let backup = e.user_home.join(".claude/settings.json.bak");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&backup).unwrap()).unwrap(),
        existing
    );
    // Registry carries the pin under global.hooks, not global.skills.
    let registry = fs::read_to_string(e.home.join("registry.yml")).unwrap();
    assert!(registry.contains("hooks:\n    logger: 0.1.0"), "{registry}");
    assert!(!registry.contains("skills:\n    logger"), "{registry}");

    // Second add: nothing changes, no new backup is written.
    let before = fs::read(settings_path(&e)).unwrap();
    fs::remove_file(&backup).unwrap();
    let out = ok(&e, &["--json", "hook", "add", "logger"]);
    let report = json(&out);
    assert!(
        report["registered"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["status"] == "present")
    );
    assert!(report["backup"].is_null());
    assert_eq!(fs::read(settings_path(&e)).unwrap(), before);
    assert!(!backup.exists());

    // List shows the link, the declared events and that all are registered.
    let out = ok(&e, &["hook", "list"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("logger@0.1.0"), "{text}");
    assert!(
        text.contains("events: SessionEnd, PostToolUse, PreCompact (all registered)"),
        "{text}"
    );
    let out = ok(&e, &["--json", "hook", "list"]);
    let rows = json(&out);
    assert_eq!(rows[0]["link"]["status"], "ok");
    assert_eq!(rows[0]["missing"], json!([]));
    // Skills listing marks the pinned hook as global; doctor is clean, and
    // with a hook registered but no Cursor event logged it asks to confirm
    // Third-Party Imports; one Cursor event turns that into confirmation.
    let out = ok(&e, &["--json", "skills"]);
    assert_eq!(json(&out)[0]["versions"][0]["global"], true);
    let out = ok(&e, &["--json", "doctor"]);
    let report = json(&out);
    assert_eq!(report["problems"], json!([]));
    let cursor = report["telemetry"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["check"] == "cursor-imports")
        .cloned()
        .expect("cursor-imports finding");
    assert_eq!(cursor["severity"], "warning");
    assert!(
        cursor["fix"]
            .as_str()
            .unwrap()
            .contains("Third-Party Imports")
    );
    fs::create_dir_all(e.home.join("log")).unwrap();
    fs::write(
        e.home.join("log/events.jsonl"),
        "{\"ts\":\"2026-10-19T12:00:00Z\",\"tool\":\"cursor\",\"event\":\"SessionEnd\"}\n",
    )
    .unwrap();
    let out = ok(&e, &["--json", "doctor"]);
    let cursor = json(&out)["telemetry"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["check"] == "cursor-imports")
        .cloned()
        .unwrap();
    assert_eq!(cursor["severity"], "info");
    assert!(cursor["message"].as_str().unwrap().contains("1 event(s)"));

    // Remove: our entries go, the hand-written one and other keys stay, the
    // link is gone, and the pin is dropped.
    let out = ok(&e, &["--json", "hook", "remove", "logger"]);
    let report = json(&out);
    assert_eq!(report["hooks"], json!({}));
    let after = settings(&e);
    assert_eq!(after["model"], "sonnet");
    assert_eq!(after["hooks"]["SessionEnd"].as_array().unwrap().len(), 1);
    assert_eq!(
        after["hooks"]["SessionEnd"][0]["hooks"][0]["command"],
        "say done"
    );
    assert!(after["hooks"].get("PostToolUse").is_none());
    assert!(after["hooks"].get("PreCompact").is_none());
    assert!(!link.exists());
    assert!(
        !fs::read_to_string(e.home.join("registry.yml"))
            .unwrap()
            .contains("hooks:")
    );
    let out = ok(&e, &["hook", "list"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "No hooks.");
}

#[test]
fn hook_add_creates_missing_settings_and_dry_run_writes_nothing() {
    let e = env();
    seal_logger(&e);
    assert!(!settings_path(&e).exists());
    let out = ok(&e, &["--json", "--dry-run", "hook", "add", "logger"]);
    assert_eq!(json(&out)["dry_run"], true);
    assert!(!settings_path(&e).exists());
    assert!(!e.user_home.join(".claude/hooks/logger").exists());
    assert!(
        !fs::read_to_string(e.home.join("registry.yml"))
            .unwrap()
            .contains("hooks:")
    );
    ok(&e, &["hook", "add", "logger"]);
    let after = settings(&e);
    assert_eq!(our_commands(&after, "SessionEnd").len(), 1);
    assert!(!e.user_home.join(".claude/settings.json.bak").exists());
}

#[test]
fn refusals_never_touch_settings_or_registry() {
    let e = env();
    seal_logger(&e);
    ok(&e, &["new", "demo"]);
    ok(&e, &["seal", "demo"]);
    // Invalid JSON is a hard stop.
    fs::create_dir_all(e.user_home.join(".claude")).unwrap();
    fs::write(settings_path(&e), "{ \"hooks\": [ oops").unwrap();
    let registry_before = fs::read(e.home.join("registry.yml")).unwrap();
    let out = run(&e, &["hook", "add", "logger"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("is not valid JSON"), "{stderr}");
    assert_eq!(
        fs::read_to_string(settings_path(&e)).unwrap(),
        "{ \"hooks\": [ oops"
    );
    assert_eq!(
        fs::read(e.home.join("registry.yml")).unwrap(),
        registry_before
    );
    assert!(!e.user_home.join(".claude/hooks/logger").exists());
    fs::remove_file(settings_path(&e)).unwrap();
    // A skill is not a hook, and a hook is not a skill.
    let out = run(&e, &["hook", "add", "demo"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("is a skill, not a hook"));
    let out = run(&e, &["global", "add", "logger"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("skillctl hook add logger"));
    assert!(!settings_path(&e).exists());
}

#[test]
fn doctor_reports_unregistered_and_duplicated_hooks_and_gc_keeps_pinned_hooks() {
    let e = env();
    seal_logger(&e);
    ok(&e, &["hook", "add", "logger"]);
    let path = settings_path(&e);
    // Drop one of our events by hand.
    let mut value = settings(&e);
    value["hooks"].as_object_mut().unwrap().remove("PreCompact");
    fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let out = run(&e, &["--json", "doctor"]);
    let problems = json(&out)["problems"].clone();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(problems.as_array().unwrap().len(), 1);
    assert_eq!(problems[0]["check"], "hook-unregistered");
    assert!(
        problems[0]["message"]
            .as_str()
            .unwrap()
            .contains("PreCompact")
    );
    assert_eq!(problems[0]["fix"], "skillctl hook add logger");
    // hook add re-registers only what is missing.
    let out = ok(&e, &["--json", "hook", "add", "logger"]);
    let statuses: Vec<&str> = json(&out)["registered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["status"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>()
        .iter()
        .map(|s| if s == "added" { "added" } else { "present" })
        .collect();
    assert_eq!(statuses, ["present", "present", "added"]);
    let out = ok(&e, &["--json", "doctor"]);
    assert_eq!(json(&out)["problems"], json!([]));

    // A duplicated entry, and a copy in Cursor's own hooks file, are warnings.
    let mut value = settings(&e);
    let group = value["hooks"]["SessionEnd"][0].clone();
    value["hooks"]["SessionEnd"]
        .as_array_mut()
        .unwrap()
        .push(group);
    fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    fs::create_dir_all(e.user_home.join(".cursor")).unwrap();
    fs::write(
        e.user_home.join(".cursor/hooks.json"),
        r#"{"version":1,"hooks":{"sessionEnd":[{"command":"\"$HOME/.claude/hooks/logger/hook.py\""}]}}"#,
    )
    .unwrap();
    let out = run(&e, &["--json", "doctor"]);
    let checks: Vec<String> = json(&out)["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["check"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(checks, ["hook-duplicate", "hook-duplicate"]);

    // gc keeps the pinned hook version and removes an unpinned older one.
    ok(&e, &["edit", "logger"]);
    ok(&e, &["seal", "logger", "--bump", "minor"]);
    ok(&e, &["hook", "add", "logger@0.2.0"]);
    let out = ok(&e, &["--json", "--yes", "gc", "--keep", "0"]);
    let deleted = json(&out)["deleted"].clone();
    assert_eq!(deleted.as_array().unwrap().len(), 1);
    assert_eq!(deleted[0]["version"], "0.1.0");
    assert!(e.home.join("store/logger/0.2.0").is_dir());
    assert_eq!(
        fs::canonicalize(e.user_home.join(".claude/hooks/logger")).unwrap(),
        fs::canonicalize(e.home.join("store/logger/0.2.0/logger")).unwrap()
    );
    // Upgrading repointed the link; settings were not rewritten for it.
    assert_eq!(our_commands(&settings(&e), "SessionEnd").len(), 2);
}

#[test]
fn relink_all_and_doctor_fix_restore_the_hook_link() {
    let e = env();
    seal_logger(&e);
    ok(&e, &["hook", "add", "logger"]);
    let link = e.user_home.join(".claude/hooks/logger");
    fs::remove_file(&link).unwrap();
    let out = run(&e, &["--json", "doctor"]);
    let problems = json(&out)["problems"].clone();
    assert_eq!(problems[0]["check"], "link-missing");
    assert!(
        problems[0]["message"]
            .as_str()
            .unwrap()
            .contains("the global hooks")
    );
    ok(&e, &["doctor", "--fix"]);
    assert!(link.is_symlink());
    fs::remove_file(&link).unwrap();
    ok(&e, &["relink", "--all"]);
    assert!(link.is_symlink());
}
