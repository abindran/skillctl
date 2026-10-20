//! `skillctl uninstall <name>[@version]`: deletes unpinned store versions,
//! refuses pinned ones naming the pins, previews with --dry-run.
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(home: &Path, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .env("HOME", home.parent().unwrap())
        .current_dir(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}
fn ok(home: &Path, dir: &Path, args: &[&str]) -> Output {
    let out = run(home, dir, args);
    assert!(
        out.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn versions(home: &Path, dir: &Path, name: &str) -> Vec<String> {
    let out = ok(home, dir, &["--json", "artifacts"]);
    let rows: Value = serde_json::from_slice(&out.stdout).unwrap();
    rows.as_array()
        .unwrap()
        .iter()
        .filter(|row| row["name"] == name)
        .map(|row| row["version"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn uninstall_deletes_unpinned_versions_and_refuses_pinned_ones() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap()
            .success()
    );
    ok(&home, &repo, &["new", "review"]);
    ok(&home, &repo, &["seal", "review"]);
    ok(&home, &repo, &["edit", "review"]);
    ok(&home, &repo, &["seal", "review"]);
    ok(
        &home,
        &repo,
        &["attach", "--new", "web", "--skills", "review"],
    );
    assert_eq!(versions(&home, &repo, "review"), ["0.1.0", "0.1.1"]);

    // The pinned version is refused, naming the project.
    let out = run(
        &home,
        &repo,
        &["--json", "--yes", "uninstall", "review@0.1.1"],
    );
    assert_eq!(out.status.code(), Some(4));
    let error: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("review@0.1.1 is pinned by web"),
        "{error}"
    );
    assert!(
        error["hint"]
            .as_str()
            .unwrap()
            .contains("skillctl remove review")
    );
    // So is "every version" while one of them is pinned.
    let out = run(&home, &repo, &["--yes", "uninstall", "review"]);
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(versions(&home, &repo, "review"), ["0.1.0", "0.1.1"]);

    // A dry run names the unpinned version and deletes nothing.
    let out = ok(
        &home,
        &repo,
        &["--json", "--dry-run", "uninstall", "review@0.1.0"],
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["deleted"][0]["version"], "0.1.0");
    assert_eq!(versions(&home, &repo, "review"), ["0.1.0", "0.1.1"]);

    // Deleting the unpinned version works and leaves the pinned one.
    let out = ok(&home, &repo, &["--yes", "uninstall", "review@0.1.0"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Deleted:\n  review@0.1.0"));
    assert_eq!(versions(&home, &repo, "review"), ["0.1.1"]);
    assert!(!home.join("store/review/0.1.0").exists());
    assert!(home.join("store/review/0.1.1").exists());

    // Without --yes and without a terminal the question cannot be answered,
    // which is a usage error and deletes nothing.
    ok(&home, &repo, &["remove", "review"]);
    let out = run(&home, &repo, &["--json", "uninstall", "review"]);
    assert_eq!(out.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["error"], "missing_answer", "{report}");
    assert_eq!(versions(&home, &repo, "review"), ["0.1.1"]);

    // Unpinned: every version goes and the artifact folder with it.
    ok(&home, &repo, &["--yes", "uninstall", "review"]);
    assert!(versions(&home, &repo, "review").is_empty());
    assert!(!home.join("store/review").exists());

    // Unknown names and versions are usage errors.
    assert_eq!(
        run(&home, &repo, &["--yes", "uninstall", "review"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run(&home, &repo, &["--yes", "uninstall", "Bad Name"])
            .status
            .code(),
        Some(2)
    );
    ok(&home, &repo, &["new", "other"]);
    ok(&home, &repo, &["seal", "other"]);
    let out = run(&home, &repo, &["--yes", "uninstall", "other@9.9.9"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("stored: 0.1.0"));
}
