use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(home: &Path, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
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
fn versions(home: &Path) -> Vec<String> {
    let dir = home.join("store/demo");
    if !dir.exists() {
        return Vec::new();
    }
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn gc_keeps_pinned_and_newest_versions_and_asks_first() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    ok(&home, &project, &["new", "demo"]);
    ok(&home, &project, &["seal", "demo"]);
    for _ in 0..2 {
        ok(&home, &project, &["edit", "demo"]);
        ok(&home, &project, &["seal", "demo"]);
    }
    assert_eq!(versions(&home), ["0.1.0", "0.1.1", "0.1.2"]);
    ok(
        &home,
        &project,
        &["attach", "--new", "web", "--skills", "demo@0.1.0"],
    );
    // Without a terminal the confirmation needs --yes.
    let out = run(&home, &project, &["gc"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    assert_eq!(versions(&home), ["0.1.0", "0.1.1", "0.1.2"]);
    // Dry run lists only the unpinned, non-newest version.
    let out = ok(&home, &project, &["--dry-run", "gc"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Would delete"));
    assert!(text.contains("demo@0.1.1"));
    assert!(!text.contains("demo@0.1.0") && !text.contains("demo@0.1.2"));
    assert_eq!(versions(&home), ["0.1.0", "0.1.1", "0.1.2"]);
    let out = ok(&home, &project, &["--yes", "gc", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["deleted"].as_array().unwrap().len(), 1);
    assert_eq!(versions(&home), ["0.1.0", "0.1.2"]);
    // --keep 0 removes every unpinned version; the pinned one survives.
    ok(&home, &project, &["--yes", "gc", "--keep", "0"]);
    assert_eq!(versions(&home), ["0.1.0"]);
    let out = ok(&home, &project, &["--yes", "gc", "--keep", "0"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Nothing to delete."));
    // Once unpinned, the last version and the empty skill folder go too.
    ok(&home, &project, &["remove", "demo"]);
    ok(&home, &project, &["--yes", "gc", "--keep", "0"]);
    assert!(versions(&home).is_empty());
    assert!(!home.join("store/demo").exists());
    let out = ok(&home, &project, &["doctor"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("No problems found."));
}
