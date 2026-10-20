//! `skillctl show <name>[@version|@draft]`: the root document and file list
//! of a stored version or a draft, as JSON and as plain text.
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}
fn ok(home: &Path, args: &[&str]) -> Output {
    let out = run(home, args);
    assert!(
        out.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

#[test]
fn show_prints_the_root_document_of_drafts_and_versions() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    ok(&home, &["new", "review"]);
    fs::create_dir_all(home.join("drafts/review/references")).unwrap();
    fs::write(home.join("drafts/review/references/notes.md"), "# notes\n").unwrap();

    let out = ok(&home, &["--json", "show", "review@draft"]);
    let shown: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(shown["name"], "review");
    assert_eq!(shown["version"], "draft");
    assert_eq!(shown["kind"], "skill");
    assert_eq!(shown["document"], "SKILL.md");
    assert!(shown["content"].as_str().unwrap().contains("name: review"));
    assert!(shown["dir"].as_str().unwrap().ends_with("drafts/review"));
    let files: Vec<&str> = shown["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["SKILL.md", "references/notes.md"], "{files:?}");
    assert_eq!(shown["files"][1]["bytes"], 8);

    ok(&home, &["seal", "review"]);
    let out = ok(&home, &["--json", "show", "review"]);
    let shown: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        shown["version"], "0.1.0",
        "no version means the newest stable"
    );
    assert!(
        shown["dir"]
            .as_str()
            .unwrap()
            .ends_with("store/review/0.1.0/review")
    );
    let out = ok(&home, &["--json", "show", "review@0.1.0"]);
    let shown: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(shown["version"], "0.1.0");

    // Text output is the document itself.
    let out = ok(&home, &["show", "review@0.1.0"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("---\n"), "{text}");
    assert!(text.contains("name: review"));

    // Hooks show HOOK.md.
    ok(&home, &["new", "--kind", "hook", "logger"]);
    let out = ok(&home, &["--json", "show", "logger@draft"]);
    let shown: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(shown["kind"], "hook");
    assert_eq!(shown["document"], "HOOK.md");

    // Missing targets are usage errors.
    assert_eq!(run(&home, &["show", "review@9.9.9"]).status.code(), Some(2));
    assert_eq!(run(&home, &["show", "nothing"]).status.code(), Some(2));
    assert_eq!(run(&home, &["show", "review@nope"]).status.code(), Some(2));
    assert_eq!(run(&home, &["show", "logger@draft"]).status.code(), Some(0));
}
