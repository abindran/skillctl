use skillctl::registry::Registry;
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
fn registry(home: &Path) -> Registry {
    Registry::from_yaml(&fs::read(home.join("registry.yml")).unwrap()).unwrap()
}
fn text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).trim_end().to_owned()
}

#[test]
fn config_get_and_set_cover_every_key_and_relink_cleans_dropped_targets() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join("repo");
    fs::create_dir(&root).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&root)
            .status()
            .unwrap()
            .success()
    );
    ok(&home, &root, &["init"]);
    assert_eq!(
        text(&ok(&home, &root, &["config", "get", "link_targets"])),
        ".agents/skills,.claude/skills"
    );
    let out = ok(&home, &root, &["config", "get", "link_targets", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        value["value"],
        serde_json::json!([".agents/skills", ".claude/skills"])
    );
    assert_eq!(
        run(&home, &root, &["config", "get", "nope"]).status.code(),
        Some(2)
    );
    assert_eq!(
        run(&home, &root, &["config", "set", "git_exclude", "maybe"])
            .status
            .code(),
        Some(2)
    );
    ok(&home, &root, &["new", "demo"]);
    ok(&home, &root, &["seal", "demo"]);
    ok(
        &home,
        &root,
        &["attach", "--new", "web", "--skills", "demo"],
    );
    assert!(root.join(".agents/skills/demo").is_symlink());

    // Without a terminal and without --yes the setting changes but links stay.
    let out = ok(
        &home,
        &root,
        &[
            "config",
            "set",
            "link_targets",
            ".claude/skills,.zcode/skills",
        ],
    );
    assert!(text(&out).contains("relink --all"));
    let reg = registry(&home);
    assert_eq!(
        reg.settings
            .link_targets
            .iter()
            .map(|t| t.as_path().display().to_string())
            .collect::<Vec<_>>(),
        [".claude/skills", ".zcode/skills"]
    );
    assert_eq!(
        reg.settings
            .previous_link_targets
            .iter()
            .map(|t| t.as_path().display().to_string())
            .collect::<Vec<_>>(),
        [".agents/skills"]
    );
    assert!(root.join(".agents/skills/demo").is_symlink());
    assert!(!root.join(".zcode/skills/demo").exists());

    // With --yes the change is applied everywhere and the history is cleared.
    let out = ok(
        &home,
        &root,
        &["--yes", "config", "set", "link_targets", ".claude/skills"],
    );
    assert!(text(&out).contains("Removed"));
    assert!(!root.join(".agents/skills/demo").exists());
    assert!(root.join(".claude/skills/demo").is_symlink());
    assert!(registry(&home).settings.previous_link_targets.is_empty());
    // The folders skillctl created for the dropped targets go too, once empty,
    // and are forgotten; the folder a current target lives in stays.
    assert!(!root.join(".agents").exists());
    assert!(!root.join(".zcode").exists());
    assert!(root.join(".claude/skills").is_dir());
    let created: Vec<String> = registry(&home).projects.values().next().unwrap().instances[0]
        .created_dirs
        .iter()
        .map(|dir| dir.as_path().display().to_string())
        .collect();
    assert!(
        created
            .iter()
            .all(|dir| !dir.starts_with(".agents") && !dir.starts_with(".zcode")),
        "{created:?}"
    );

    ok(&home, &root, &["config", "set", "git_exclude", "false"]);
    assert_eq!(
        text(&ok(&home, &root, &["config", "get", "git_exclude"])),
        "false"
    );
    ok(
        &home,
        &root,
        &["config", "set", "validate.allow", "O102, H201"],
    );
    assert_eq!(
        text(&ok(&home, &root, &["config", "get", "validate.allow"])),
        "O102,H201"
    );
    ok(
        &home,
        &root,
        &["--yes", "config", "set", "global_in_instances", "true"],
    );
    assert!(registry(&home).settings.global_in_instances);
    let personal = temp.path().join("personal/skills");
    ok(
        &home,
        &root,
        &[
            "--yes",
            "config",
            "set",
            "global_link_targets",
            personal.to_str().unwrap(),
        ],
    );
    assert_eq!(
        text(&ok(&home, &root, &["config", "get", "global_link_targets"])),
        personal.to_str().unwrap()
    );
    // Dry runs change nothing.
    let before = fs::read(home.join("registry.yml")).unwrap();
    ok(
        &home,
        &root,
        &["--dry-run", "config", "set", "git_exclude", "true"],
    );
    assert_eq!(fs::read(home.join("registry.yml")).unwrap(), before);
}

#[test]
fn completions_render_a_script_without_touching_the_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    for (shell, marker) in [("zsh", "#compdef skillctl"), ("bash", "complete")] {
        let out = ok(&home, temp.path(), &["completions", shell]);
        assert!(text(&out).contains(marker), "{shell}");
    }
    assert!(!home.exists());
}
