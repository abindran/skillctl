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
fn ok(home: &Path, dir: &Path, args: &[&str]) {
    let o = run(home, dir, args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}
#[test]
fn pins_propagate_to_all_instances_remove_and_repair() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    for n in ["demo", "other"] {
        ok(&home, &a, &["new", n]);
        ok(&home, &a, &["seal", n]);
    }
    ok(&home, &a, &["attach", "--new", "web", "--skills", "demo"]);
    ok(&home, &b, &["attach", "--project", "web"]);
    let before = fs::read(home.join("registry.yml")).unwrap();
    ok(&home, &a, &["--dry-run", "add", "other"]);
    assert_eq!(before, fs::read(home.join("registry.yml")).unwrap());
    assert!(!b.join(".claude/skills/other").exists());
    ok(&home, &a, &["add", "other"]);
    for root in [&a, &b] {
        assert!(root.join(".claude/skills/other").is_symlink());
    }
    fs::remove_file(b.join(".claude/skills/demo")).unwrap();
    ok(&home, &b, &["relink"]);
    assert!(b.join(".claude/skills/demo").is_symlink());
    ok(&home, &a, &["remove", "other"]);
    for root in [&a, &b] {
        assert!(!root.join(".claude/skills/other").exists());
    }
    ok(&home, tmp.path(), &["relink", "--all"]);
    assert_eq!(
        run(&home, tmp.path(), &["add", "demo"]).status.code(),
        Some(2)
    );
}
#[test]
fn concurrent_adds_keep_both_pins() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path();
    for n in ["one", "two"] {
        ok(&home, root, &["new", n]);
        ok(&home, root, &["seal", n]);
    }
    ok(&home, root, &["attach", "--new", "web", "--skills", "one"]);
    ok(&home, root, &["remove", "one"]);
    let spawn = |n| {
        Command::new(env!("CARGO_BIN_EXE_skillctl"))
            .arg("--home")
            .arg(&home)
            .args(["add", n])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut a = spawn("one");
    let mut b = spawn("two");
    assert!(a.wait().unwrap().success());
    assert!(b.wait().unwrap().success());
    let registry =
        skillctl::registry::Registry::from_yaml(&fs::read(home.join("registry.yml")).unwrap())
            .unwrap();
    assert_eq!(registry.projects["web"].skills.len(), 2);
}
