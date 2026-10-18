use std::{fs, path::Path, process::Command};
fn ok(home: &Path, dir: &Path, args: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .arg("--home")
        .arg(home)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn detach_removes_only_managed_paths_and_keeps_project_and_committed_content() {
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
    ok(
        &home,
        &root,
        &["attach", "--new", "web", "--skills", "demo"],
    );
    fs::write(root.join(".agents/skills/own"), "project data").unwrap();
    let before = fs::read(home.join("registry.yml")).unwrap();
    ok(&home, &root, &["--dry-run", "detach"]);
    assert_eq!(before, fs::read(home.join("registry.yml")).unwrap());
    assert!(root.join(".claude/skills/demo").is_symlink());
    ok(&home, &root, &["detach"]);
    assert_eq!(
        fs::read_to_string(root.join(".agents/skills/own")).unwrap(),
        "project data"
    );
    assert!(!root.join(".agents/skills/demo").exists());
    assert!(!root.join(".claude").exists());
    assert!(
        !fs::read_to_string(root.join(".git/info/exclude"))
            .unwrap()
            .contains("skillctl")
    );
    let r = skillctl::registry::Registry::from_yaml(&fs::read(home.join("registry.yml")).unwrap())
        .unwrap();
    assert!(r.projects["web"].instances.is_empty());
}
#[test]
fn keep_links_and_delete_last_project() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("instance");
    fs::create_dir(&root).unwrap();
    ok(&home, &root, &["new", "demo"]);
    ok(&home, &root, &["seal", "demo"]);
    ok(
        &home,
        &root,
        &["attach", "--new", "web", "--skills", "demo"],
    );
    ok(
        &home,
        &root,
        &["detach", "--keep-links", "--delete-project"],
    );
    assert!(root.join(".agents/skills/demo").is_symlink());
    let r = skillctl::registry::Registry::from_yaml(&fs::read(home.join("registry.yml")).unwrap())
        .unwrap();
    assert!(r.projects.is_empty());
}
