//! Milestone-2 workflow on isolated real Git repositories.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn run(home: &Path, dir: &Path, args: &[&str]) {
    let o = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .arg("--home")
        .arg(home)
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}
fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
#[derive(Debug, PartialEq, Eq)]
enum Entry {
    File(Vec<u8>, u32),
    Dir(u32),
    Link(PathBuf),
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    use std::os::unix::fs::PermissionsExt;
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Entry>) {
        for item in fs::read_dir(path).unwrap() {
            let p = item.unwrap().path();
            let m = fs::symlink_metadata(&p).unwrap();
            let key = p.strip_prefix(root).unwrap().to_path_buf();
            let value = if m.file_type().is_symlink() {
                Entry::Link(fs::read_link(&p).unwrap())
            } else if m.is_dir() {
                visit(root, &p, out);
                Entry::Dir(m.permissions().mode())
            } else {
                Entry::File(fs::read(&p).unwrap(), m.permissions().mode())
            };
            out.insert(key, value);
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}
#[test]
fn full_git_clean_repair_and_detach_workflow_with_zero_write_dry_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    for root in [&a, &b] {
        git(root, &["init", "-q"]);
        fs::write(root.join("README.md"), "committed project data\n").unwrap();
        git(root, &["add", "README.md"]);
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "initial",
            ],
        );
    }
    run(&home, &a, &["new", "demo"]);
    run(&home, &a, &["seal", "demo"]);
    run(&home, &a, &["new", "other"]);
    run(&home, &a, &["seal", "other"]);
    let registry_file = home.join("registry.yml");
    let mut registry =
        skillctl::registry::Registry::from_yaml(&fs::read(&registry_file).unwrap()).unwrap();
    registry.settings.global_link_targets =
        vec![skillctl::paths::HomePath::new(tmp.path().join("personal/skills")).unwrap()];
    fs::write(&registry_file, registry.to_yaml().unwrap()).unwrap();
    let before = snapshot(tmp.path());
    run(
        &home,
        &a,
        &["--dry-run", "attach", "--new", "web", "--skills", "demo"],
    );
    assert_eq!(before, snapshot(tmp.path()));
    run(&home, &a, &["attach", "--new", "web", "--skills", "demo"]);
    run(&home, &b, &["attach", "--project", "web"]);
    for args in [
        vec!["--dry-run", "add", "other"],
        vec!["--dry-run", "remove", "demo"],
        vec!["--dry-run", "relink", "--all"],
        vec!["--dry-run", "detach"],
        vec!["--dry-run", "global", "add", "other"],
        vec!["--dry-run", "new", "preview"],
    ] {
        let before = snapshot(tmp.path());
        run(&home, &a, &args);
        assert_eq!(before, snapshot(tmp.path()), "{args:?}");
    }
    run(&home, &a, &["add", "other"]);
    assert!(b.join(".claude/skills/other").is_symlink());
    // Intentional destructive operation confined to this test's temporary repo.
    git(&b, &["clean", "-fdx"]);
    assert!(!b.join(".agents").exists());
    run(&home, &b, &["relink"]);
    assert!(b.join(".claude/skills/demo").is_symlink());
    for root in [&a, &b] {
        run(&home, root, &["detach"]);
        assert!(!root.join(".agents").exists());
        assert!(!root.join(".claude").exists());
        assert_eq!(
            fs::read_to_string(root.join("README.md")).unwrap(),
            "committed project data\n"
        );
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        assert!(status.stdout.is_empty());
    }
}
