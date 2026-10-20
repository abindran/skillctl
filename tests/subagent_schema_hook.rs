//! The bundled subagent-schema hook: validates and installs, finds an agent's
//! schema through the project link, blocks invalid output with exit 2 up to
//! three times per agent run, and stays silent without a schema.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn hook_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("hooks/subagent-schema")
}

fn python() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("python3"))
        .find(|path| path.is_file())
}

fn run(cwd: &Path, tmp: &Path, event: &Value) -> (i32, String, String) {
    let mut child = Command::new(hook_dir().join("hook.py"))
        .env("TMPDIR", tmp)
        // The hook also reads ~/.claude/agents; keep the user's out of it.
        .env("HOME", tmp)
        .env("SKILLCTL_SUBAGENT_SCHEMA_DEBUG", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn subagent_schema_hook_validates_and_installs() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("HOME", temp.path())
        .arg("install")
        .arg(hook_dir())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("HOME", temp.path())
        .args(["validate", "subagent-schema@1.0.1"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "0 error(s), 0 warning(s)"
    );
}

#[test]
fn invalid_output_is_sent_back_three_times_then_reported() {
    if python().is_none() {
        eprintln!("python3 not on PATH; skipping");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let tmp = temp.path().join("tmp");
    let store = temp.path().join("store/explore/1.0.0/explore");
    fs::create_dir_all(project.join(".claude/agents")).unwrap();
    fs::create_dir_all(&tmp).unwrap();
    fs::create_dir_all(&store).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("agents/explore/AGENT.md"),
        store.join("AGENT.md"),
    )
    .unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("agents/explore/schema.json"),
        store.join("schema.json"),
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        store.join("AGENT.md"),
        project.join(".claude/agents/explore.md"),
    )
    .unwrap();
    let event = |message: &str| {
        json!({
            "session_id": "s", "cwd": project, "hook_event_name": "SubagentStop",
            "agent_id": "agent_42", "agent_type": "Explore", "last_assistant_message": message
        })
    };

    // Valid, fenced output: silent exit 0.
    let good = r#"Done. ```json
{"status":"ok","answer":"Found it","files":[],"open_questions":[],"confidence":0.9,"artifact":".claude/scratch/x.md"}
```"#;
    let (code, out, err) = run(&project, &tmp, &event(good));
    assert_eq!((code, out.as_str()), (0, ""), "{err}");
    assert!(err.contains("matches its schema"), "{err}");

    // Prose: blocked with the reason, three times, then a systemMessage.
    for attempt in 1..=3 {
        let (code, out, err) = run(
            &project,
            &tmp,
            &event("I looked around and found three files."),
        );
        assert_eq!(code, 2, "attempt {attempt}: {err}");
        assert_eq!(out, "");
        assert!(err.contains(&format!("attempt {attempt} of 3")), "{err}");
        assert!(err.contains("contains no JSON object"), "{err}");
    }
    let (code, out, _) = run(&project, &tmp, &event("still prose"));
    assert_eq!(code, 0);
    let message: Value = serde_json::from_str(out.trim()).unwrap();
    assert!(
        message["systemMessage"]
            .as_str()
            .unwrap()
            .contains("after 3 attempts")
    );

    // Wrong shape names the fields; a different agent id starts a new count.
    let bad = r#"{"status":"done","answer":5,"files":[]}"#;
    let mut other = event(bad);
    other["agent_id"] = json!("agent_43");
    let (code, _, err) = run(&project, &tmp, &other);
    assert_eq!(code, 2);
    for needle in [
        "$.status must be one of",
        "$.answer must be string",
        "$.open_questions is required",
        "$.confidence is required",
    ] {
        assert!(err.contains(needle), "{needle}: {err}");
    }

    // No schema (unknown agent), auxiliary agent (no type), garbage: silent.
    let mut unknown = event(bad);
    unknown["agent_type"] = json!("Plan");
    let (code, out, err) = run(&project, &tmp, &unknown);
    assert_eq!((code, out.as_str()), (0, ""));
    assert!(err.contains("no schema for Plan"), "{err}");
    let mut auxiliary = event(bad);
    auxiliary.as_object_mut().unwrap().remove("agent_type");
    assert_eq!(run(&project, &tmp, &auxiliary).0, 0);
    let mut child = Command::new(hook_dir().join("hook.py"))
        .env("TMPDIR", &tmp)
        .current_dir(&project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"garbage").unwrap();
    assert!(child.wait().unwrap().success());
}

#[cfg(unix)]
#[test]
fn the_schema_is_found_by_frontmatter_name_and_in_global_agents() {
    if python().is_none() {
        eprintln!("python3 not on PATH; skipping");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let tmp = temp.path().join("tmp");
    let store = temp.path().join("store/explore/1.0.0/explore");
    fs::create_dir_all(project.join(".claude/agents")).unwrap();
    fs::create_dir_all(&tmp).unwrap();
    fs::create_dir_all(&store).unwrap();
    for file in ["AGENT.md", "schema.json"] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("agents/explore")
                .join(file),
            store.join(file),
        )
        .unwrap();
    }
    let prose = |agent: &str| {
        json!({
            "session_id": "s", "cwd": project, "hook_event_name": "SubagentStop",
            "agent_id": format!("id-{agent}"), "agent_type": agent,
            "last_assistant_message": "only prose"
        })
    };
    // A file whose name differs from the agent's `name` field is still matched.
    std::os::unix::fs::symlink(
        store.join("AGENT.md"),
        project.join(".claude/agents/renamed-explorer.md"),
    )
    .unwrap();
    let (code, _, err) = run(&project, &tmp, &prose("explore"));
    assert_eq!(code, 2, "{err}");
    fs::remove_file(project.join(".claude/agents/renamed-explorer.md")).unwrap();

    // A global agent under ~/.claude/agents (HOME is the temp folder in `run`).
    fs::create_dir_all(tmp.join(".claude/agents")).unwrap();
    std::os::unix::fs::symlink(
        store.join("AGENT.md"),
        tmp.join(".claude/agents/explore.md"),
    )
    .unwrap();
    let (code, _, err) = run(&project, &tmp, &prose("explore"));
    assert_eq!(code, 2, "{err}");

    // Neither: no schema, no check.
    fs::remove_file(tmp.join(".claude/agents/explore.md")).unwrap();
    let (code, out, _) = run(&project, &tmp, &prose("explore"));
    assert_eq!((code, out.as_str()), (0, ""));
}
