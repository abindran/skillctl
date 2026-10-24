use skillctl::{services::fs::RealFs, skill::Skill};
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
    let output = run(home, dir, args);
    assert!(
        output.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn edit_copies_a_version_into_a_writable_draft_with_a_patch_bump() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let cwd = temp.path();
    ok(&home, cwd, &["new", "demo"]);
    let draft = home.join("drafts/demo");
    fs::write(draft.join("scripts/run.sh"), "#!/bin/sh\necho hi\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            draft.join("scripts/run.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let text = fs::read_to_string(draft.join("SKILL.md")).unwrap().replace(
        "## Instructions",
        "Run `scripts/run.sh`.\n\n## Instructions",
    );
    fs::write(draft.join("SKILL.md"), text).unwrap();
    ok(&home, cwd, &["seal", "demo"]);
    assert!(!draft.exists());
    // Dry run plans the draft without creating it.
    let out = ok(&home, cwd, &["--dry-run", "edit", "demo"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Would create draft demo"));
    assert!(!draft.exists());
    let out = ok(&home, cwd, &["edit", "demo", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["path"], draft.to_str().unwrap());
    let skill = Skill::load(&RealFs, &draft).unwrap();
    assert_eq!(skill.version().unwrap().unwrap().to_string(), "0.1.1");
    assert!(draft.join("scripts/run.sh").is_file());
    assert!(draft.join("references").is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&draft.join("SKILL.md")), 0o644);
        assert_eq!(mode(&draft.join("scripts/run.sh")), 0o755);
        assert_eq!(mode(&draft.join("scripts")), 0o755);
        // The store copy is untouched and still read-only.
        assert_eq!(mode(&home.join("store/demo/0.1.0/demo/SKILL.md")), 0o444);
    }
    // An existing draft is only replaced with --force.
    fs::write(draft.join("notes.md"), "work in progress").unwrap();
    assert_eq!(run(&home, cwd, &["edit", "demo"]).status.code(), Some(2));
    assert!(draft.join("notes.md").exists());
    ok(&home, cwd, &["edit", "demo@0.1.0", "--force"]);
    assert!(!draft.join("notes.md").exists());
    // Unknown versions and draft pins are rejected.
    assert_eq!(
        run(&home, cwd, &["edit", "demo@9.9.9", "--force"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        run(&home, cwd, &["edit", "demo@draft", "--force"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(run(&home, cwd, &["edit", "nope"]).status.code(), Some(1));
}

#[test]
fn edited_draft_can_be_linked_with_add_draft_and_sealed_as_the_next_version() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    ok(&home, &project, &["new", "demo"]);
    ok(&home, &project, &["seal", "demo"]);
    ok(
        &home,
        &project,
        &["attach", "--new", "web", "--skills", "demo"],
    );
    ok(&home, &project, &["edit", "demo"]);
    ok(&home, &project, &["add", "demo", "--draft"]);
    let link = project.join(".claude/skills/demo");
    assert_eq!(fs::read_link(&link).unwrap(), home.join("drafts/demo"));
    let out = ok(&home, &project, &["status"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("demo@draft"));
    ok(&home, &project, &["seal", "demo"]);
    assert!(home.join("store/demo/0.1.1/demo/SKILL.md").is_file());
    ok(&home, &project, &["add", "demo@0.1.1"]);
    assert_eq!(
        fs::read_link(&link).unwrap(),
        home.join("store/demo/0.1.1/demo")
    );
}
