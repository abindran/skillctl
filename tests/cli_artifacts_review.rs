//! `skillctl artifacts [--unused]`: every store version with kind, pins and
//! recorded use; --unused keeps unpinned versions and pinned skills or agents
//! the log has not seen used after a week.
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(home: &Path, user_home: &Path, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .env("HOME", user_home)
        .current_dir(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[test]
#[allow(clippy::type_complexity)]
fn artifacts_lists_pins_and_usage_and_unused_filters() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let user_home = temp.path().join("userhome");
    let repo = temp.path().join("repo");
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
    let ok = |args: &[&str]| {
        let out = run(&home, &user_home, &repo, args);
        assert!(
            out.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    ok(&["new", "review"]);
    ok(&["seal", "review"]);
    ok(&["edit", "review"]);
    ok(&["seal", "review"]); // 0.1.1, leaving 0.1.0 unpinned
    ok(&["new", "triage"]);
    ok(&["seal", "triage"]);
    ok(&["new", "--kind", "hook", "logger"]);
    ok(&["seal", "logger"]);
    ok(&[
        "attach",
        "--new",
        "web",
        "--skills",
        "review@0.1.1",
        "--skills",
        "triage",
    ]);
    ok(&["hook", "add", "logger"]);
    // Eight days of log: review was activated, triage never.
    fs::create_dir_all(home.join("log")).unwrap();
    fs::write(
        home.join("log/events.jsonl"),
        concat!(
            r#"{"ts":"2026-10-01T09:00:00Z","tool":"claude-code","event":"SessionStart"}"#, "\n",
            r#"{"ts":"2026-10-05T09:00:00Z","tool":"claude-code","event":"PostToolUse","tool_name":"Skill","skill":"review"}"#, "\n",
            r#"{"ts":"2026-10-09T09:00:00Z","tool":"claude-code","event":"SessionEnd"}"#, "\n",
        ),
    )
    .unwrap();

    let out = ok(&["--json", "artifacts"]);
    let rows: Value = serde_json::from_slice(&out.stdout).unwrap();
    let summary: Vec<(String, String, String, Vec<String>, Option<u64>, bool)> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["name"].as_str().unwrap().to_owned(),
                r["version"].as_str().unwrap().to_owned(),
                r["kind"].as_str().unwrap().to_owned(),
                r["pinned_by"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| p.as_str().unwrap().to_owned())
                    .collect(),
                r["uses"].as_u64(),
                r["unused"].as_bool().unwrap(),
            )
        })
        .collect();
    let s = |v: &str| v.to_owned();
    assert_eq!(
        summary,
        [
            (
                s("logger"),
                s("0.1.0"),
                s("hook"),
                vec![s("hook")],
                None,
                false
            ),
            // Usage is recorded per skill name, so every version shares it.
            (s("review"), s("0.1.0"), s("skill"), vec![], Some(1), true),
            (
                s("review"),
                s("0.1.1"),
                s("skill"),
                vec![s("web")],
                Some(1),
                false
            ),
            (
                s("triage"),
                s("0.1.0"),
                s("skill"),
                vec![s("web")],
                Some(0),
                true
            ),
        ]
    );
    assert_eq!(rows[2]["last_used"], "2026-10-05T09:00:00Z");
    assert!(
        rows[2]["description"].as_str().unwrap_or("").len() > 10,
        "the root document's description travels with the row: {}",
        rows[2]
    );
    assert!(rows[0]["description"].is_string(), "hooks have one too");
    assert!(
        rows[3]["reason"]
            .as_str()
            .unwrap()
            .contains("no recorded use in 8 days")
    );

    let out = ok(&["artifacts", "--unused"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("review@0.1.0 (skill) — unpinned"), "{text}");
    assert!(
        text.contains("triage@0.1.0 (skill) — pinned by web; no recorded use"),
        "{text}"
    );
    assert!(!text.contains("review@0.1.1"), "{text}");
    assert!(!text.contains("logger"), "{text}");

    // A young log never calls a pinned skill unused.
    fs::write(
        home.join("log/events.jsonl"),
        r#"{"ts":"2026-10-09T09:00:00Z","tool":"claude-code","event":"SessionEnd"}"#,
    )
    .unwrap();
    let out = ok(&["--json", "artifacts", "--unused"]);
    let rows: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["version"], "0.1.0");
    assert_eq!(rows[0]["name"], "review");
}
