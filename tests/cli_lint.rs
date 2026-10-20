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

#[test]
fn lint_reports_budget_and_procedure_rules_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let project = temp.path().join("project");
    repo(&project);
    let mut agents = String::from(
        "# Project\n\nRun cargo test before every commit.\n\n## Release process\n\n1. bump\n2. tag\n3. push\n4. wait for CI\n5. publish\n",
    );
    agents.push_str(&"fact\n".repeat(200));
    fs::write(project.join("AGENTS.md"), agents).unwrap();
    fs::write(project.join("CLAUDE.md"), "@AGENTS.md\n").unwrap();
    let rules = project.join(".cursor/rules");
    fs::create_dir_all(&rules).unwrap();
    fs::write(
        rules.join("style.mdc"),
        format!("---\nalwaysApply: true\n---\n{}", "x\n".repeat(150)),
    )
    .unwrap();
    let out = run(&home, &project, &["--json", "lint"]);
    assert!(out.status.success(), "warnings alone pass");
    let findings: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rules: Vec<(&str, Option<u64>)> = findings
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["rule"].as_str().unwrap(), f["line"].as_u64()))
        .collect();
    assert_eq!(rules, [("I301", None), ("I303", Some(5)), ("I302", None)]);
    assert!(
        findings
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["severity"] == "warning")
    );
    assert!(
        findings[1]["message"]
            .as_str()
            .unwrap()
            .contains("Release process")
    );
    let out = run(&home, &project, &["lint", "--strict"]);
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn lint_files_lists_what_lint_reads_present_or_not() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let project = temp.path().join("project");
    repo(&project);

    let out = run(&home, &project, &["--json", "lint", "--files"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let files: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files = files.as_array().unwrap();
    assert!(files.iter().all(|file| file["present"] == false));
    assert_eq!(files[0]["name"], "AGENTS.md");
    let out = run(&home, &project, &["lint", "--files"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Nothing to lint here"));

    fs::write(project.join("AGENTS.md"), "# Facts\nOne.\n").unwrap();
    let out = run(&home, &project, &["--json", "lint", "--files"]);
    let files: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(files[0]["present"], true);
    assert_eq!(files[0]["lines"], 2);
    assert!(
        files[0]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule == "I301")
    );
}
