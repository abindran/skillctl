use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

struct Env {
    _temp: tempfile::TempDir,
    home: std::path::PathBuf,
    user_home: std::path::PathBuf,
    root: std::path::PathBuf,
}

fn env() -> Env {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let user_home = temp.path().join("userhome");
    let root = temp.path().join("repo");
    fs::create_dir_all(&user_home).unwrap();
    fs::create_dir(&root).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&root)
            .status()
            .unwrap()
            .success()
    );
    Env {
        _temp: temp,
        home,
        user_home,
        root,
    }
}

fn run(env: &Env, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &env.home)
        .env("HOME", &env.user_home)
        .current_dir(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}
fn ok(env: &Env, dir: &Path, args: &[&str]) -> Output {
    let out = run(env, dir, args);
    assert!(
        out.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn checks(env: &Env, dir: &Path, args: &[&str]) -> (Option<i32>, Vec<String>, Vec<String>) {
    let mut full = vec!["--json"];
    full.extend_from_slice(args);
    let out = run(env, dir, &full);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let problems = report["problems"].as_array().unwrap();
    let all = problems
        .iter()
        .map(|p| p["check"].as_str().unwrap().to_owned())
        .collect();
    let fixed = problems
        .iter()
        .filter(|p| p["fixed"] == true)
        .map(|p| p["check"].as_str().unwrap().to_owned())
        .collect();
    (out.status.code(), all, fixed)
}

#[test]
fn doctor_reports_and_fixes_link_drift_missing_instances_and_store_permissions() {
    let e = env();
    ok(&e, &e.root, &["new", "demo"]);
    ok(&e, &e.root, &["seal", "demo"]);
    ok(&e, &e.root, &["attach", "--new", "web", "--skills", "demo"]);
    let out = ok(&e, &e.root, &["doctor"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("No problems found."));

    // A deleted link is reported, exits 1, and --fix restores it.
    let link = e.root.join(".agents/skills/demo");
    fs::remove_file(&link).unwrap();
    let (code, all, _) = checks(&e, &e.root, &["doctor"]);
    assert_eq!(code, Some(1));
    assert_eq!(all, ["link-missing"]);
    let out = run(&e, &e.root, &["--json", "doctor"]);
    let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"], "doctor");
    let (code, _, fixed) = checks(&e, &e.root, &["doctor", "--fix"]);
    assert_eq!(code, Some(0));
    assert_eq!(fixed, ["link-missing"]);
    assert!(link.is_symlink());

    // A vanished instance folder is removed from the registry by --fix.
    let second = e.root.parent().unwrap().join("second");
    fs::create_dir(&second).unwrap();
    ok(&e, &second, &["attach", "--project", "web"]);
    fs::remove_dir_all(&second).unwrap();
    let (code, all, _) = checks(&e, &e.root, &["doctor"]);
    assert_eq!(code, Some(1));
    assert_eq!(all, ["instance-missing"]);
    let (code, _, fixed) = checks(&e, &e.root, &["doctor", "--fix"]);
    assert_eq!(code, Some(0));
    assert_eq!(fixed, ["instance-missing"]);
    let registry =
        skillctl::registry::Registry::from_yaml(&fs::read(e.home.join("registry.yml")).unwrap())
            .unwrap();
    assert_eq!(registry.projects["web"].instances.len(), 1);

    // Writable store entries are fixed; tampered content is only reported.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let stored = e.home.join("store/demo/0.1.0/demo/SKILL.md");
        fs::set_permissions(&stored, fs::Permissions::from_mode(0o644)).unwrap();
        let (code, all, _) = checks(&e, &e.root, &["doctor"]);
        assert_eq!(code, Some(1));
        assert_eq!(all, ["store-writable"]);
        let (code, _, fixed) = checks(&e, &e.root, &["doctor", "--fix"]);
        assert_eq!(code, Some(0));
        assert_eq!(fixed, ["store-writable"]);
        assert_eq!(
            fs::metadata(&stored).unwrap().permissions().mode() & 0o777,
            0o444
        );
        fs::set_permissions(&stored, fs::Permissions::from_mode(0o644)).unwrap();
        let mut text = fs::read_to_string(&stored).unwrap();
        text.push_str("\ntampered\n");
        fs::write(&stored, text).unwrap();
        let (code, all, fixed) = checks(&e, &e.root, &["doctor", "--fix"]);
        assert_eq!(code, Some(1));
        assert!(all.contains(&"hash-mismatch".to_owned()), "{all:?}");
        assert_eq!(fixed, ["store-writable"]);
    }
}

#[test]
fn doctor_reports_missing_pins_and_shadowing_personal_copies() {
    let e = env();
    ok(&e, &e.root, &["new", "demo"]);
    ok(&e, &e.root, &["seal", "demo"]);
    ok(&e, &e.root, &["attach", "--new", "web", "--skills", "demo"]);
    // A personal copy in the user-level folder shadows the project skill.
    fs::create_dir_all(e.user_home.join(".claude/skills/demo")).unwrap();
    let (code, all, _) = checks(&e, &e.root, &["doctor"]);
    assert_eq!(code, Some(1));
    assert_eq!(all, ["shadowed"]);
    fs::remove_dir_all(e.user_home.join(".claude/skills/demo")).unwrap();
    // A pin whose version is not installed.
    let file = e.home.join("registry.yml");
    let text = fs::read_to_string(&file)
        .unwrap()
        .replace("demo: 0.1.0", "demo: 9.9.9");
    fs::write(&file, text).unwrap();
    let (code, all, _) = checks(&e, &e.root, &["doctor"]);
    assert_eq!(code, Some(1));
    assert!(all.contains(&"pin-missing".to_owned()), "{all:?}");
    assert!(all.contains(&"link-wrong".to_owned()), "{all:?}");
    // Dry runs report without touching anything.
    let before = fs::read(&file).unwrap();
    let (_, _, fixed) = checks(&e, &e.root, &["--dry-run", "doctor", "--fix"]);
    assert!(!fixed.is_empty());
    assert_eq!(fs::read(&file).unwrap(), before);
}
