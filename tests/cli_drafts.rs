use skillctl::{
    services::fs::RealFs,
    skill::Skill,
    store::{
        hash::content_hash,
        meta::{Metadata, Source},
    },
};
use std::{
    path::Path,
    process::{Command, Output},
};

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .args(args)
        .output()
        .unwrap()
}
fn ok(home: &Path, args: &[&str]) -> Output {
    let output = run(home, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn new_then_seal_publishes_metadata_hash_and_read_only_version() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let output = ok(&home, &["new", "demo", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let draft = home.join("drafts/demo");
    assert_eq!(report["path"], draft.to_str().unwrap());
    assert!(draft.join("scripts").is_dir());
    assert!(draft.join("references").is_dir());
    let skill = Skill::load(&RealFs, &draft).unwrap();
    assert_eq!(skill.version().unwrap().unwrap().to_string(), "0.1.0");
    let output = ok(&home, &["seal", "demo", "--json"]);
    let version = home.join("store/demo/0.1.0");
    let stored = version.join("demo");
    assert!(!draft.exists());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["path"], version.to_str().unwrap());
    let meta = Metadata::load(&RealFs, &version.join("meta.json")).unwrap();
    assert_eq!(meta.source, Source::Local);
    assert_eq!(meta.content_hash, content_hash(&RealFs, &stored).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(stored.join("SKILL.md"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o444
        );
        assert_eq!(
            std::fs::metadata(&stored).unwrap().permissions().mode() & 0o777,
            0o555
        );
        skillctl::store::stage::restore_write_mode(&RealFs, &version).unwrap();
    }
}

#[test]
fn duplicate_drafts_invalid_skills_and_existing_versions_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    ok(&home, &["new", "demo"]);
    let file = home.join("drafts/demo/SKILL.md");
    let original = std::fs::read(&file).unwrap();
    assert!(!run(&home, &["new", "demo"]).status.success());
    assert_eq!(std::fs::read(&file).unwrap(), original);
    std::fs::write(&file, "---\nname: demo\ndescription: ''\n---\n").unwrap();
    assert_eq!(run(&home, &["seal", "demo"]).status.code(), Some(3));
    assert!(!home.join("store/demo").exists());
    std::fs::write(&file, &original).unwrap();
    ok(&home, &["seal", "demo"]);
    ok(&home, &["new", "demo"]);
    assert!(!run(&home, &["seal", "demo"]).status.success());
    assert_eq!(std::fs::read(&file).unwrap(), original);
    #[cfg(unix)]
    skillctl::store::stage::restore_write_mode(&RealFs, &home.join("store/demo/0.1.0")).unwrap();
}

#[test]
fn custom_template_preserves_fields_and_body_and_bumps_requested_component() {
    for (bump, expected) in [("major", "1.0.0"), ("minor", "0.2.0"), ("patch", "0.1.1")] {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("skills");
        ok(&home, &["init"]);
        std::fs::create_dir(home.join("templates/custom")).unwrap();
        std::fs::write(
            home.join("templates/custom/SKILL.md"),
            "---\nname: placeholder\nmetadata: {owner: team}\ncustom: true\n---\n# {{name}}\n",
        )
        .unwrap();
        ok(&home, &["new", "demo", "--from-template", "custom"]);
        ok(&home, &["seal", "demo", "--bump", bump]);
        let version = home.join(format!("store/demo/{expected}"));
        let skill = Skill::load(&RealFs, &version.join("demo")).unwrap();
        assert_eq!(skill.version().unwrap().unwrap().to_string(), expected);
        assert_eq!(skill.frontmatter.metadata.value().unwrap()["owner"], "team");
        assert_eq!(skill.body, "# demo\n");
        assert!(skill.frontmatter.extra.contains_key("custom"));
        #[cfg(unix)]
        skillctl::store::stage::restore_write_mode(&RealFs, &version).unwrap();
    }
}

#[test]
fn dry_runs_change_neither_missing_home_nor_existing_draft() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    ok(&home, &["--dry-run", "new", "demo"]);
    assert!(!home.exists());
    ok(&home, &["new", "demo"]);
    let file = home.join("drafts/demo/SKILL.md");
    let original = std::fs::read(&file).unwrap();
    let output = ok(
        &home,
        &["--dry-run", "--json", "seal", "demo", "--bump", "major"],
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["dry_run"], true);
    assert!(
        report["path"]
            .as_str()
            .unwrap()
            .ends_with("store/demo/1.0.0")
    );
    assert_eq!(std::fs::read(&file).unwrap(), original);
    assert!(!home.join("store/demo").exists());
}

#[test]
fn traversal_symlinks_missing_version_and_bump_collision_do_not_destroy_drafts() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    ok(&home, &["init"]);
    assert!(
        !run(&home, &["new", "demo", "--from-template", "../default"])
            .status
            .success()
    );
    assert!(!home.join("drafts/demo").exists());
    ok(&home, &["new", "demo"]);
    let file = home.join("drafts/demo/SKILL.md");
    let original = std::fs::read(&file).unwrap();
    std::fs::create_dir_all(home.join("store/demo/1.0.0")).unwrap();
    assert!(
        !run(&home, &["seal", "demo", "--bump", "major"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&file).unwrap(), original);
    std::fs::write(&file, "---\nname: demo\ndescription: Valid\n---\n").unwrap();
    assert_eq!(run(&home, &["seal", "demo"]).status.code(), Some(3));
    #[cfg(unix)]
    {
        std::fs::write(&file, &original).unwrap();
        let external = temp.path().join("outside");
        std::fs::write(&external, "untouched").unwrap();
        std::os::unix::fs::symlink(&external, home.join("drafts/demo/link")).unwrap();
        assert!(!run(&home, &["seal", "demo"]).status.success());
        assert_eq!(std::fs::read_to_string(external).unwrap(), "untouched");
        assert!(file.exists());
    }
}

#[test]
fn concurrent_new_and_seal_never_replace_an_existing_version() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    ok(&home, &["init"]);
    let spawn = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_skillctl"))
            .env("SKILLCTL_HOME", &home)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut first = spawn(&["new", "demo"]);
    let mut second = spawn(&["new", "demo"]);
    assert_ne!(
        first.wait().unwrap().success(),
        second.wait().unwrap().success()
    );
    let mut first = spawn(&["seal", "demo"]);
    let mut second = spawn(&["seal", "demo"]);
    assert_ne!(
        first.wait().unwrap().success(),
        second.wait().unwrap().success()
    );
    assert!(home.join("store/demo/0.1.0/meta.json").exists());
    #[cfg(unix)]
    skillctl::store::stage::restore_write_mode(&RealFs, &home.join("store/demo/0.1.0")).unwrap();
}
#[test]
#[cfg(unix)]
fn seal_removes_drafts_containing_read_only_folders() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    ok(&home, &["new", "demo"]);
    let draft = home.join("drafts/demo");
    let script = draft.join("scripts/run.sh");
    std::fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o555)).unwrap();
    let text = std::fs::read_to_string(draft.join("SKILL.md"))
        .unwrap()
        .replace(
            "## Instructions",
            "Run `scripts/run.sh`.\n\n## Instructions",
        );
    std::fs::write(draft.join("SKILL.md"), text).unwrap();
    std::fs::set_permissions(
        draft.join("scripts"),
        std::fs::Permissions::from_mode(0o555),
    )
    .unwrap();
    ok(&home, &["seal", "demo"]);
    assert!(!draft.exists());
    assert!(home.join("store/demo/0.1.0/demo/scripts/run.sh").is_file());
}

#[test]
fn drafts_are_listed_with_their_kind_until_sealed() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let out = ok(&home, &["--json", "drafts"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "[]");
    ok(&home, &["new", "review"]);
    ok(&home, &["new", "--kind", "hook", "logger"]);
    ok(&home, &["new", "--kind", "agent", "explore"]);
    std::fs::create_dir_all(home.join("drafts/empty")).unwrap();
    std::fs::write(home.join("drafts/not a name"), "").unwrap();
    let out = ok(&home, &["--json", "drafts"]);
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let summary: Vec<(String, Option<String>)> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["name"].as_str().unwrap().to_owned(),
                row["kind"].as_str().map(str::to_owned),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("empty".to_owned(), None),
            ("explore".to_owned(), Some("agent".to_owned())),
            ("logger".to_owned(), Some("hook".to_owned())),
            ("review".to_owned(), Some("skill".to_owned())),
        ]
    );
    assert!(rows[3]["path"].as_str().unwrap().ends_with("drafts/review"));
    let out = ok(&home, &["drafts"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("review (skill) "), "{text}");
    assert!(text.contains("empty (no root document) "), "{text}");
    ok(&home, &["seal", "review"]);
    let out = ok(&home, &["--json", "drafts"]);
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 3);
}
