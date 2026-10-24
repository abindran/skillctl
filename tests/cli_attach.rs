use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn cli(home: &Path, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .arg("--home")
        .arg(home)
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}
fn ok(home: &Path, dir: &Path, args: &[&str]) -> Output {
    let o = cli(home, dir, args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    o
}
fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("repo");
    fs::create_dir(&root).unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&root)
            .status()
            .unwrap()
            .success()
    );
    ok(&home, &root, &["new", "demo"]);
    ok(&home, &root, &["seal", "demo"]);
    (tmp, home, root)
}
#[test]
fn attach_is_git_invisible_status_resolves_subfolders_and_repeated_attach_is_safe() {
    let (_tmp, home, root) = fixture();
    ok(
        &home,
        &root,
        &["attach", "--new", "web", "--skills", "demo"],
    );
    let status = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(status.stdout.is_empty());
    let sub = root.join("sub");
    fs::create_dir(&sub).unwrap();
    let o = ok(&home, &sub, &["--json", "status"]);
    let value: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(value["project"], "web");
    assert_eq!(value["links"][0]["status"], "ok");
    ok(&home, &root, &["attach", "--project", "web"]);
    let registry =
        skillctl::registry::Registry::from_yaml(&fs::read(home.join("registry.yml")).unwrap())
            .unwrap();
    assert_eq!(registry.projects["web"].instances.len(), 1);
}
#[test]
fn missing_answer_nesting_missing_versions_and_conflicts() {
    let (_tmp, home, root) = fixture();
    assert_eq!(cli(&home, &root, &["attach"]).status.code(), Some(2));
    assert_eq!(
        cli(
            &home,
            &root,
            &["attach", "--new", "web", "--skills", "demo@9.0.0"]
        )
        .status
        .code(),
        Some(1)
    );
    fs::create_dir_all(root.join(".claude/skills/demo")).unwrap();
    assert_eq!(
        cli(
            &home,
            &root,
            &["attach", "--new", "web", "--skills", "demo"]
        )
        .status
        .code(),
        Some(4)
    );
    assert!(root.join(".claude/skills/demo").is_dir());
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    // --yes chooses the Git root, so this is a move to an existing instance.
    assert_eq!(
        cli(&home, &nested, &["--yes", "attach", "--project", "web"])
            .status
            .code(),
        Some(4)
    );
}
#[test]
fn dry_run_does_not_change_registry_or_links_and_unattached_status_is_success() {
    let (_tmp, home, root) = fixture();
    let before = fs::read(home.join("registry.yml")).unwrap();
    ok(
        &home,
        &root,
        &["--dry-run", "attach", "--new", "web", "--skills", "demo"],
    );
    assert_eq!(before, fs::read(home.join("registry.yml")).unwrap());
    assert!(!root.join(".claude").exists());
    let o = ok(&home, &root, &["status"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("Not attached"));
}
#[test]
fn subfolder_attach_is_answered_by_flags_and_nesting_is_a_usage_error() {
    let (_tmp, home, root) = fixture();
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let out = cli(
        &home,
        &nested,
        &["attach", "--new", "web", "--skills", "demo"],
    );
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--top-level") && stderr.contains("--here"),
        "{stderr}"
    );
    ok(
        &home,
        &nested,
        &["attach", "--new", "web", "--skills", "demo", "--here"],
    );
    let registry =
        skillctl::registry::Registry::from_yaml(&fs::read(home.join("registry.yml")).unwrap())
            .unwrap();
    assert_eq!(
        registry.projects["web"].instances[0].path,
        nested.canonicalize().unwrap()
    );
    assert!(nested.join(".claude/skills/demo").is_symlink());
    // The repository root would now contain an instance: a usage error, not a corrupt registry.
    let out = cli(&home, &root, &["attach", "--new", "api"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("nested"));
    ok(&home, &nested, &["detach"]);
    ok(
        &home,
        &nested,
        &["attach", "--project", "web", "--top-level"],
    );
    let registry =
        skillctl::registry::Registry::from_yaml(&fs::read(home.join("registry.yml")).unwrap())
            .unwrap();
    assert_eq!(
        registry.projects["web"].instances[0].path,
        root.canonicalize().unwrap()
    );
}
#[test]
fn json_conflict_output_is_a_single_stdout_document() {
    let (_tmp, home, root) = fixture();
    fs::create_dir_all(root.join(".claude/skills/demo")).unwrap();
    let out = cli(
        &home,
        &root,
        &["--json", "attach", "--new", "web", "--skills", "demo"],
    );
    assert_eq!(out.status.code(), Some(4));
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["project"], "web");
    let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["exit_code"], 4);
    let out = ok(&home, &root, &["status"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains(".claude/skills: conflict"));
}
