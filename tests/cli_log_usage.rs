//! `skillctl log usage`: ccusage's daily figures joined with the log, and a
//! clear report when ccusage is absent.
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn run(home: &Path, path_env: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .env("PATH", path_env)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn usage_joins_ccusage_days_with_the_log_or_reports_its_absence() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let bin = temp.path().join("bin");
    let empty = temp.path().join("empty");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&empty).unwrap();
    fs::create_dir_all(home.join("log")).unwrap();
    let today = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).date_naive();
    let yesterday = today - chrono::Duration::days(1);
    fs::write(
        bin.join("ccusage"),
        format!(
            "#!/bin/sh\necho '{{\"daily\":[{{\"period\":\"{yesterday}\",\"inputTokens\":100,\"outputTokens\":4000,\"cacheCreationTokens\":900,\"cacheReadTokens\":9000,\"totalTokens\":14000,\"totalCost\":8.0,\"modelBreakdowns\":[{{\"modelName\":\"claude-opus-5-5\",\"cost\":8.0,\"outputTokens\":4000}}]}}],\"totals\":{{\"totalCost\":8.0}}}}'\n"
        ),
    )
    .unwrap();
    fs::set_permissions(bin.join("ccusage"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        home.join("log/events.jsonl"),
        format!(
            "{{\"ts\":\"{yesterday}T09:00:00Z\",\"tool\":\"claude-code\",\"event\":\"UserPromptSubmit\",\"session\":\"a\"}}\n{{\"ts\":\"{yesterday}T10:00:00Z\",\"tool\":\"claude-code\",\"event\":\"UserPromptSubmit\",\"session\":\"b\"}}\n"
        ),
    )
    .unwrap();

    let out = run(&home, &bin, &["--json", "log", "usage", "--days", "7"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let usage: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(usage["available"], true);
    assert_eq!(usage["days"].as_array().unwrap().len(), 1);
    assert_eq!(usage["days"][0]["date"], yesterday.to_string());
    assert_eq!(usage["days"][0]["prompts"], 2);
    assert_eq!(usage["days"][0]["sessions"], 2);
    assert_eq!(usage["days"][0]["cost_per_prompt"], 4.0);
    assert_eq!(usage["days"][0]["output_tokens_per_prompt"], 2000.0);
    assert_eq!(usage["days"][0]["cache_hit_ratio"], 0.9);
    assert_eq!(usage["days"][0]["models"][0]["model"], "claude-opus-5-5");
    assert_eq!(usage["total_cost"], 8.0);
    assert_eq!(usage["cost_per_prompt"], 4.0);
    let out = run(&home, &bin, &["log", "usage"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("Usage (Claude Code, via ccusage)"),
        "{text}"
    );
    assert!(text.contains(&yesterday.to_string()), "{text}");

    let out = run(&home, &empty, &["--json", "log", "usage"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let usage: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(usage["available"], false);
    assert!(
        usage["note"]
            .as_str()
            .unwrap()
            .contains("npm install -g ccusage")
    );
    assert!(usage["days"].as_array().unwrap().is_empty());
}
