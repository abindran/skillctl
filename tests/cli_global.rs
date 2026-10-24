use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn run(home: &Path, dir: &Path, args: &[&str]) -> Output {
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
    let out = run(home, dir, args);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
#[test]
fn globals_link_into_isolated_target_clashes_are_refused_both_ways_and_remove_cleans() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("project");
    let personal = tmp.path().join("personal/skills");
    fs::create_dir(&root).unwrap();
    for name in ["demo", "other"] {
        ok(&home, &root, &["new", name]);
        ok(&home, &root, &["seal", name]);
    }
    let file = home.join("registry.yml");
    let mut registry = skillctl::registry::Registry::from_yaml(&fs::read(&file).unwrap()).unwrap();
    registry.settings.global_link_targets =
        vec![skillctl::paths::HomePath::new(personal.clone()).unwrap()];
    registry.settings.global_in_instances = true;
    fs::write(&file, registry.to_yaml().unwrap()).unwrap();
    ok(
        &home,
        &root,
        &["attach", "--new", "web", "--skills", "other"],
    );
    let before = fs::read(&file).unwrap();
    ok(&home, &root, &["--dry-run", "global", "add", "demo"]);
    assert_eq!(fs::read(&file).unwrap(), before);
    assert!(!personal.exists());
    ok(&home, &root, &["global", "add", "demo"]);
    assert!(personal.join("demo").is_symlink());
    assert!(root.join(".claude/skills/demo").is_symlink());
    assert_eq!(run(&home, &root, &["add", "demo"]).status.code(), Some(2));
    assert_eq!(
        run(&home, &root, &["global", "add", "other"]).status.code(),
        Some(2)
    );
    let out = ok(&home, &root, &["--json", "global", "list"]);
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rows[0]["links"][0]["status"], "ok");
    fs::remove_file(personal.join("demo")).unwrap();
    ok(&home, &root, &["relink", "--all"]);
    assert!(personal.join("demo").is_symlink());
    ok(&home, &root, &["global", "remove", "demo"]);
    assert!(!personal.join("demo").exists());
    assert!(!root.join(".claude/skills/demo").exists());
}
#[test]
fn personal_data_is_a_conflict_not_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let personal = tmp.path().join("personal");
    fs::create_dir_all(personal.join("demo")).unwrap();
    ok(&home, tmp.path(), &["new", "demo"]);
    ok(&home, tmp.path(), &["seal", "demo"]);
    let file = home.join("registry.yml");
    let mut registry = skillctl::registry::Registry::from_yaml(&fs::read(&file).unwrap()).unwrap();
    registry.settings.global_link_targets =
        vec![skillctl::paths::HomePath::new(personal.clone()).unwrap()];
    fs::write(&file, registry.to_yaml().unwrap()).unwrap();
    assert_eq!(
        run(&home, tmp.path(), &["global", "add", "demo"])
            .status
            .code(),
        Some(4)
    );
    assert!(personal.join("demo").is_dir());
    assert!(!personal.join("demo").is_symlink());
}
#[test]
#[cfg(unix)]
fn symlinked_parent_of_a_global_target_receives_per_skill_links() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let real = tmp.path().join("dotfiles/claude");
    fs::create_dir_all(real.join("skills")).unwrap();
    std::os::unix::fs::symlink(&real, tmp.path().join(".claude")).unwrap();
    let target = tmp.path().join(".claude/skills");
    ok(&home, tmp.path(), &["new", "demo"]);
    ok(&home, tmp.path(), &["seal", "demo"]);
    let file = home.join("registry.yml");
    let mut registry = skillctl::registry::Registry::from_yaml(&fs::read(&file).unwrap()).unwrap();
    registry.settings.global_link_targets = vec![skillctl::paths::HomePath::new(target).unwrap()];
    fs::write(&file, registry.to_yaml().unwrap()).unwrap();
    ok(&home, tmp.path(), &["global", "add", "demo"]);
    assert!(real.join("skills/demo").is_symlink());
    assert!(!real.join("skills").is_symlink());
    let out = ok(&home, tmp.path(), &["global", "list"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("demo@0.1.0\n  ") && text.contains(": ok"),
        "{text}"
    );
}
