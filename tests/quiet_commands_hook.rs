//! The bundled quiet-commands hook: validates and installs; blocks bare test
//! and build commands with the wrapped command as the reason when the
//! test-quiet wrapper is linked; rewrites in place when asked to; and stays
//! silent in Cursor, without the wrapper, and for everything else.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn hook_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("hooks/quiet-commands")
}

fn python() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("python3"))
        .find(|path| path.is_file())
}

fn run(cwd: &Path, command: &str, env: &[(&str, &str)]) -> Output {
    let mut process = Command::new(hook_dir().join("hook.py"));
    process
        .env_remove("CURSOR_VERSION")
        .env_remove("SKILLCTL_QUIET_REWRITE")
        .env("SKILLCTL_QUIET_COMMANDS_DEBUG", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        process.env(key, value);
    }
    let mut child = process.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({
                "session_id": "s", "cwd": cwd, "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": command, "description": "run it", "timeout": 1000}
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn quiet_commands_hook_validates_and_installs() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("HOME", temp.path())
        .args(["install"])
        .arg(hook_dir())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("HOME", temp.path())
        .args(["validate", "quiet-commands@1.0.0"])
        .output()
        .unwrap();
    assert_eq!(text(&out.stdout).trim(), "0 error(s), 0 warning(s)");
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("HOME", temp.path())
        .args(["hook", "add", "quiet-commands"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    let settings: Value = serde_json::from_str(
        &fs::read_to_string(temp.path().join(".claude/settings.json")).unwrap(),
    )
    .unwrap();
    let entry = &settings["hooks"]["PreToolUse"][0];
    assert_eq!(entry["matcher"], "Bash");
    assert!(
        entry["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("/.claude/hooks/quiet-commands/")
    );
}

#[test]
fn quiet_commands_block_raw_tests_only_where_the_wrapper_is_linked() {
    if python().is_none() {
        eprintln!("python3 not on PATH; skipping");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let sub = project.join("crates/core");
    fs::create_dir_all(&sub).unwrap();

    // No wrapper anywhere above: silent.
    let out = run(&project, "cargo test", &[]);
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(text(&out.stderr).contains("no test-quiet wrapper"));

    // Link the real wrapper the way skillctl does: a folder link under .claude/scripts.
    let scripts = project.join(".claude/scripts");
    fs::create_dir_all(&scripts).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("wrappers/test-quiet"),
        scripts.join("test-quiet"),
    )
    .unwrap();
    #[cfg(not(unix))]
    {
        fs::create_dir_all(scripts.join("test-quiet")).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("wrappers/test-quiet/test-quiet.sh"),
            scripts.join("test-quiet/test-quiet.sh"),
        )
        .unwrap();
    }

    // A bare test command at the project root: blocked, relative wrapped command.
    let out = run(&project, "cargo test --locked", &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let err = text(&out.stderr);
    assert!(
        err.contains("\n.claude/scripts/test-quiet/test-quiet.sh cargo test --locked\n"),
        "{err}"
    );

    // From a subfolder the wrapped command uses the absolute wrapper path.
    let out = run(&sub, "npm test", &[]);
    assert_eq!(out.status.code(), Some(2));
    let err = text(&out.stderr);
    assert!(
        err.contains(&format!(
            "{} npm test",
            project
                .join(".claude/scripts/test-quiet/test-quiet.sh")
                .display()
        )),
        "{err}"
    );

    // Already wrapped, compound, and unrelated commands pass untouched.
    for command in [
        ".claude/scripts/test-quiet/test-quiet.sh cargo test",
        "cargo build && cargo test",
        "cargo test 2>&1 | tail -20",
        "git status",
        "npm run dev",
    ] {
        let out = run(&project, command, &[]);
        assert!(out.status.success(), "{command}: {}", text(&out.stderr));
        assert!(out.stdout.is_empty(), "{command}");
    }

    // Cursor: silent even with the wrapper linked.
    let out = run(&project, "cargo test", &[("CURSOR_VERSION", "1.2.3")]);
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(text(&out.stderr).contains("Cursor"));

    // Opt-in rewrite: allow with updatedInput, other fields kept.
    let out = run(&project, "pytest -q", &[("SKILLCTL_QUIET_REWRITE", "1")]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let output: Value = serde_json::from_slice(&out.stdout).unwrap();
    let specific = &output["hookSpecificOutput"];
    assert_eq!(specific["hookEventName"], "PreToolUse");
    assert_eq!(specific["permissionDecision"], "allow");
    assert_eq!(
        specific["updatedInput"]["command"],
        ".claude/scripts/test-quiet/test-quiet.sh pytest -q"
    );
    assert_eq!(specific["updatedInput"]["description"], "run it");
    assert_eq!(specific["updatedInput"]["timeout"], 1000);

    // Garbage never fails the call.
    let mut child = Command::new(hook_dir().join("hook.py"))
        .current_dir(&project)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not json").unwrap();
    assert!(child.wait().unwrap().success());
}
