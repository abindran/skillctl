//! Hook artifacts through the CLI: scaffold, validate, seal, list, edit, install,
//! and the refusal to link a hook into skills folders.
use skillctl::{artifact::Kind, services::fs::RealFs, skill::Skill, store::meta::Metadata};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(home: &Path, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .env("HOME", home.parent().unwrap())
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
fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn hook_draft_is_scaffolded_validated_sealed_listed_and_edited() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();

    // Scaffold from the built-in hook template.
    let out = ok(&home, &work, &["--json", "new", "--kind", "hook", "logger"]);
    let report = json(&out);
    assert_eq!(report["kind"], "hook");
    let draft = home.join("drafts/logger");
    assert_eq!(report["path"], draft.to_str().unwrap());
    assert!(draft.join("HOOK.md").is_file());
    assert!(draft.join("hook.py").is_file());
    assert!(!draft.join("scripts").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(draft.join("hook.py"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
    }
    let script = fs::read_to_string(draft.join("hook.py")).unwrap();
    assert!(script.starts_with("#!/usr/bin/env python3"));
    assert!(script.contains("logger: a skillctl hook"));
    let hook = Skill::load_kind(&RealFs, &draft, Kind::Hook).unwrap();
    assert_eq!(hook.version().unwrap().unwrap().to_string(), "0.1.0");
    assert_eq!(hook.frontmatter.name.value().unwrap(), "logger");

    // The scaffold validates clean (hooks run K rules, not S/O/H).
    let out = ok(&home, &work, &["validate", "logger@draft"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "0 error(s), 0 warning(s)"
    );
    let out = ok(&home, &work, &["--json", "validate"]);
    assert_eq!(json(&out), serde_json::json!([]));

    // Seal records the kind in meta.json; the stored tree is read-only.
    ok(&home, &work, &["seal", "logger"]);
    let version = home.join("store/logger/0.1.0");
    assert!(!draft.exists());
    assert!(version.join("logger/HOOK.md").is_file());
    let meta = Metadata::load(&RealFs, &version.join("meta.json")).unwrap();
    assert_eq!(meta.kind, Kind::Hook);
    assert!(
        fs::read_to_string(version.join("meta.json"))
            .unwrap()
            .contains("\"kind\": \"hook\"")
    );

    // Listing shows the kind; doctor is happy with a hook in the store.
    let out = ok(&home, &work, &["skills"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "logger (hook)\n  0.1.0 — unused\n"
    );
    let out = ok(&home, &work, &["--json", "skills"]);
    assert_eq!(json(&out)[0]["kind"], "hook");
    let out = ok(&home, &work, &["--json", "doctor"]);
    assert_eq!(json(&out)["problems"], serde_json::json!([]));

    // Edit copies the version back into a draft with a patch bump and the
    // right document rewritten.
    ok(&home, &work, &["edit", "logger"]);
    let hook = Skill::load_kind(&RealFs, &draft, Kind::Hook).unwrap();
    assert_eq!(hook.version().unwrap().unwrap().to_string(), "0.1.1");
    assert!(draft.join("hook.py").is_file());
    ok(&home, &work, &["seal", "logger", "--bump", "minor"]);
    assert!(home.join("store/logger/0.2.0/logger/HOOK.md").is_file());
}

#[test]
fn hooks_cannot_be_linked_as_skills_until_registration_exists() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap()
            .success()
    );
    ok(&home, &repo, &["new", "--kind", "hook", "logger"]);
    ok(&home, &repo, &["seal", "logger"]);
    ok(&home, &repo, &["new", "demo"]);
    ok(&home, &repo, &["seal", "demo"]);
    ok(
        &home,
        &repo,
        &["attach", "--new", "web", "--skills", "demo"],
    );
    for args in [
        &["add", "logger"][..],
        &["global", "add", "logger"][..],
        &["attach", "--new", "api", "--skills", "logger"][..],
    ] {
        let out = run(&home, &repo, args);
        assert!(!out.status.success(), "{args:?} succeeded");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("is a hook, not a skill"),
            "{args:?}: {stderr}"
        );
    }
    assert!(!repo.join(".claude/skills/logger").exists());
    let out = ok(&home, &repo, &["--json", "global", "list"]);
    assert_eq!(json(&out), serde_json::json!([]));
}

#[test]
fn hooks_install_from_local_folders_alongside_skills() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let source = temp.path().join("org");
    let hook = source.join("hooks/logger");
    let skill = source.join("skills/review");
    fs::create_dir_all(&hook).unwrap();
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        hook.join("HOOK.md"),
        "---\nname: logger\ndescription: Appends one JSONL line per lifecycle event so sessions can be compared across tools.\nmetadata:\n  version: '1.2.0'\nevents: [SessionEnd, SubagentStop]\nentry: hook.py\n---\n# logger\n",
    )
    .unwrap();
    fs::write(
        hook.join("hook.py"),
        "#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(hook.join("hook.py"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: review\ndescription: Use when reviewing a change to apply the organisation's review checklist consistently.\nmetadata:\n  version: '2.0.0'\n---\n# review\n",
    )
    .unwrap();
    let out = ok(
        &home,
        &source,
        &["--json", "install", source.to_str().unwrap(), "--all"],
    );
    let installed = json(&out)["installed"].as_array().unwrap().clone();
    let names: Vec<&str> = installed
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["logger", "review"]);
    let meta = Metadata::load(&RealFs, &home.join("store/logger/1.2.0/meta.json")).unwrap();
    assert_eq!(meta.kind, Kind::Hook);
    let meta = Metadata::load(&RealFs, &home.join("store/review/2.0.0/meta.json")).unwrap();
    assert_eq!(meta.kind, Kind::Skill);

    // A hook that fails a structural rule is refused with its K finding.
    fs::remove_file(hook.join("hook.py")).unwrap();
    let out = run(
        &home,
        &source,
        &["install", hook.to_str().unwrap(), "--version", "1.3.0"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("K107"));

    // A source with neither document says so.
    let empty = temp.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let out = run(&home, &source, &["install", empty.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("No SKILL.md or HOOK.md found"));
}
