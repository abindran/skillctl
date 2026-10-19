//! `skillctl lint` on project instruction files: read-only, same report
//! formats and exit codes as validate.
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
        .output()
        .unwrap()
}

fn repo(root: &Path) {
    fs::create_dir_all(root).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(root)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn lint_reports_each_rule_with_a_fix_and_never_writes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let project = temp.path().join("project");
    repo(&project);

    // Nothing to lint: clean, exit 0, no home created as a side effect.
    let out = run(&home, &project, &["lint"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "0 error(s), 0 warning(s)"
    );

    // A lone CLAUDE.md: I304 with the migration hint.
    fs::write(
        project.join("CLAUDE.md"),
        "# Project\n\nRun cargo test before every commit.\n",
    )
    .unwrap();
    let before: Vec<_> = fs::read_dir(&project)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    let out = run(&home, &project, &["--json", "lint"]);
    assert_eq!(out.status.code(), Some(3));
    let findings: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(findings[0]["rule"], "I304");
    assert_eq!(findings[0]["severity"], "error");
    assert!(findings[0]["hint"].as_str().unwrap().contains("@AGENTS.md"));
    let after: Vec<_> = fs::read_dir(&project)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(before, after, "lint must not create or change files");

    // The fix applied by a person: AGENTS.md plus the import. A duplicated
    // line is the remaining warning, reported with its line number.
    fs::write(
        project.join("AGENTS.md"),
        "# Project\n\nRun cargo test before every commit.\n",
    )
    .unwrap();
    fs::write(
        project.join("CLAUDE.md"),
        "@AGENTS.md\n\nRun cargo test before every commit.\nUse /compact focus on the task.\n",
    )
    .unwrap();
    let out = run(&home, &project, &["lint"]);
    assert!(out.status.success(), "warnings alone pass");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("CLAUDE.md:3 I308 warning"), "{text}");
    assert!(text.contains("1 warning(s)"), "{text}");
    let out = run(&home, &project, &["lint", "--strict"]);
    assert_eq!(out.status.code(), Some(3));
    fs::write(
        project.join("CLAUDE.md"),
        "@AGENTS.md\n\nUse /compact focus on the task.\n",
    )
    .unwrap();
    let out = run(&home, &project, &["lint", "--format", "junit"]);
    assert!(out.status.success());
    let xml = String::from_utf8_lossy(&out.stdout);
    assert!(
        xml.contains("<testsuite name=\"skillctl\" tests=\"1\" failures=\"0\""),
        "{xml}"
    );

    // AGENTS.md only, shadowed by a CLAUDE.local.md: I309.
    fs::remove_file(project.join("CLAUDE.md")).unwrap();
    fs::write(
        project.join("CLAUDE.local.md"),
        "sandbox: http://localhost:9\n",
    )
    .unwrap();
    let out = run(&home, &project, &["--json", "lint"]);
    assert_eq!(out.status.code(), Some(3));
    let findings: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(findings[0]["rule"], "I309");
    assert!(
        findings[0]["message"]
            .as_str()
            .unwrap()
            .contains("CLAUDE.local.md")
    );

    // An explicit folder argument, and a parent CLAUDE.md inside the repo.
    fs::remove_file(project.join("CLAUDE.local.md")).unwrap();
    fs::write(project.join("CLAUDE.md"), "root notes only\n").unwrap();
    let nested = project.join("services/api");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("AGENTS.md"), "# api service\n").unwrap();
    let out = run(
        &home,
        temp.path(),
        &["--json", "lint", nested.to_str().unwrap()],
    );
    let findings: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rules: Vec<&str> = findings
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["rule"].as_str().unwrap())
        .collect();
    assert_eq!(rules, ["I309"]);
    assert!(
        findings[0]["message"]
            .as_str()
            .unwrap()
            .contains("project/CLAUDE.md")
            || findings[0]["message"]
                .as_str()
                .unwrap()
                .contains("CLAUDE.md"),
    );

    // Allow-listing through the registry suppresses an I rule.
    fs::remove_dir_all(project.join("services")).unwrap();
    assert!(run(&home, &project, &["init"]).status.success());
    assert!(
        run(
            &home,
            &project,
            &["config", "set", "validate.allow", "I304"]
        )
        .status
        .success()
    );
    let out = run(&home, &project, &["lint"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
