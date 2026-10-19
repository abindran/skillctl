//! Script bundles: scaffold, validate (K501–K507), link as a folder under
//! .claude/scripts, lint I306, and the two bundled quiet wrappers' behaviour.
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
fn rules(out: &Output) -> Vec<String> {
    json(out)
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["rule"].as_str().unwrap().to_owned())
        .collect()
}
fn wrappers() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("wrappers")
}

#[test]
fn script_bundles_are_scaffolded_linked_as_folders_and_wanted_by_agents_md() {
    let e = env();
    ok(&e, &["new", "--kind", "script", "quiet"]);
    let draft = e.home.join("drafts/quiet");
    assert!(draft.join("SCRIPT.md").is_file());
    assert!(draft.join("run.sh").is_file());
    let out = run(&e, &["--json", "validate", "quiet@draft"]);
    assert_eq!(rules(&out), Vec::<String>::new());
    ok(&e, &["seal", "quiet"]);
    ok(&e, &["install", wrappers().to_str().unwrap(), "--all"]);
    ok(
        &e,
        &[
            "attach",
            "--new",
            "web",
            "--skills",
            "test-quiet",
            "--skills",
            "quiet",
        ],
    );
    let link = e.repo.join(".claude/scripts/test-quiet");
    assert!(link.is_symlink());
    assert!(
        link.join("test-quiet.sh").is_file(),
        "folder link exposes the wrapper"
    );
    let exclude = fs::read_to_string(e.repo.join(".git/info/exclude")).unwrap();
    assert!(exclude.contains(".claude/scripts/test-quiet"), "{exclude}");
    let out = ok(&e, &["status"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("test-quiet@1.0.0 .claude/scripts: ok"));

    // I306 until AGENTS.md names both wrappers.
    let out = run(&e, &["--json", "lint"]);
    let mut got = rules(&out);
    got.sort();
    assert_eq!(got, ["I306", "I306"]);
    fs::write(
        e.repo.join("AGENTS.md"),
        "# Project\n\nRun tests with .claude/scripts/test-quiet/test-quiet.sh cargo test.\nFor logs use .claude/scripts/quiet/run.sh.\n",
    )
    .unwrap();
    let out = run(&e, &["--json", "lint"]);
    assert_eq!(rules(&out), Vec::<String>::new());
    ok(&e, &["remove", "quiet"]);
    assert!(!e.repo.join(".claude/scripts/quiet").exists());
    assert!(link.is_symlink());
}

#[test]
fn script_rules_fire_on_their_shapes() {
    let e = env();
    ok(&e, &["init"]);
    let dir = e.home.join("drafts/bundle");
    fs::create_dir_all(&dir).unwrap();
    let target = dir.to_str().unwrap().to_owned();
    fs::write(dir.join("SCRIPT.md"), "not frontmatter").unwrap();
    let out = run(&e, &["--json", "validate", &target]);
    assert_eq!(rules(&out), ["K501"]);
    fs::write(
        dir.join("SCRIPT.md"),
        "---\nname: other\ndescription: ''\n---\n",
    )
    .unwrap();
    let out = run(&e, &["--json", "validate", &target]);
    assert_eq!(rules(&out), ["K502", "K503", "K504", "K505"]);
    fs::write(
        dir.join("SCRIPT.md"),
        "---\nname: bundle\ndescription: Wraps the test runner so only failures are printed.\nmetadata:\n  version: '1.0.0'\n---\n# bundle\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(
            dir.join("w.sh"),
            "#!/bin/sh\nread -p 'ok?' a\nsudo rm -rf /\n",
        )
        .unwrap();
        fs::set_permissions(dir.join("w.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        let out = run(&e, &["--json", "validate", &target]);
        let got: Vec<(String, String)> = json(&out)
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["rule"].as_str().unwrap().to_owned(),
                    f["severity"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("K506".to_owned(), "warning".to_owned()),
                ("K506".to_owned(), "error".to_owned())
            ]
        );
        fs::write(dir.join("w.sh"), "echo no shebang\n").unwrap();
        let out = run(&e, &["--json", "validate", &target]);
        assert_eq!(rules(&out), ["K505"]);
    }
}

#[test]
fn bundled_wrappers_print_little_on_success_and_the_failures_on_failure() {
    let temp = tempfile::tempdir().unwrap();
    let quiet = wrappers().join("test-quiet/test-quiet.sh");
    let out = Command::new(&quiet)
        .args(["sh", "-c", "for i in 1 2 3 4 5; do echo line $i; done"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("ok: sh -c"), "{text}");
    assert!(text.contains("(5 lines suppressed)"), "{text}");
    assert!(
        text.contains("line 5") && !text.contains("line 1"),
        "{text}"
    );
    let out = Command::new(&quiet)
        .args([
            "sh",
            "-c",
            "echo building; echo 'test foo ... FAILED'; echo assertion failed: 1 == 2; exit 3",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "exit code is preserved");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("FAILED (exit 3)"), "{text}");
    assert!(
        text.contains("FAILED") && text.contains("assertion failed"),
        "{text}"
    );
    assert!(text.contains("--- last 20 lines ---"), "{text}");
    assert_eq!(
        Command::new(&quiet).output().unwrap().status.code(),
        Some(64)
    );

    let log = temp.path().join("server.log");
    let mut body = String::new();
    for i in 0..50 {
        body.push_str(&format!("INFO request {i} ok\n"));
    }
    body.push_str(
        "ERROR db timeout\nINFO retry\nERROR db timeout\nERROR db timeout\nWARN slow query 2s\n",
    );
    fs::write(&log, body).unwrap();
    let errors = wrappers().join("log-errors/log-errors.sh");
    let out = Command::new(&errors).arg(&log).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("ERROR db timeout"), "{text}");
    assert!(
        text.contains("(x2)"),
        "repeated lines are collapsed: {text}"
    );
    assert!(text.contains("WARN slow query"), "{text}");
    assert!(
        !text.contains("request 10 ok"),
        "unrelated lines are dropped: {text}"
    );
    let out = Command::new(&errors)
        .args([log.to_str().unwrap(), "slow"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    // A narrower pattern keeps its two lines of context but nothing else.
    assert!(text.contains("WARN slow query"), "{text}");
    assert!(text.lines().count() <= 4, "{text}");
    assert_eq!(
        Command::new(&errors)
            .arg("/nonexistent.log")
            .output()
            .unwrap()
            .status
            .code(),
        Some(66)
    );
}
