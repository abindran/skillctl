use skillctl::{registry::Registry, services::fs::RealFs};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const DESCRIPTION: &str = "Use when reviewing code changes to provide consistent, actionable and carefully verified engineering guidance.";
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
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn skill(dir: &Path, description: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("SKILL.md"), format!("---\nname: demo\ndescription: {description}\nmetadata:\n  version: '1.0.0'\n---\nReview the change carefully.\n")).unwrap();
}
#[test]
fn paths_drafts_versions_and_all_targets_have_deterministic_reports() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    ok(&home, &["init"]);
    let draft = home.join("drafts/demo");
    skill(&draft, DESCRIPTION);
    for target in [draft.to_str().unwrap(), "demo@draft"] {
        let output = ok(&home, &["validate", target, "--strict", "--json"]);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::json!([])
        );
    }
    ok(&home, &["seal", "demo"]);
    ok(&home, &["validate", "demo@1.0.0", "--strict"]);
    ok(&home, &["validate", "demo", "--strict"]);
    skill(&draft, DESCRIPTION);
    let output = ok(&home, &["validate", "--format", "junit"]);
    let report =
        quick_junit::Report::deserialize_from_str(std::str::from_utf8(&output.stdout).unwrap())
            .unwrap();
    assert_eq!(report.tests, 2);
    assert_eq!(report.failures, 0);
    skillctl::store::stage::restore_write_mode(&RealFs, &home.join("store/demo/1.0.0")).unwrap();
}
#[test]
fn warning_success_strict_failure_and_error_json_are_single_documents() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    ok(&home, &["new", "demo"]);
    skill(&home.join("drafts/demo"), "Use when reviewing.");
    let output = ok(&home, &["validate", "demo@draft", "--format", "json"]);
    let findings: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        findings
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["rule"] == "O102")
    );
    let output = run(&home, &["validate", "demo@draft", "--strict", "--json"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let findings: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        findings
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["severity"] == "error")
    );
    fs::write(home.join("drafts/demo/SKILL.md"), "invalid").unwrap();
    let output = run(&home, &["validate", "demo@draft", "--json"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let findings: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(findings.as_array().unwrap().len(), 1);
    assert_eq!(findings[0]["rule"], "S002");
}
#[test]
fn junit_has_one_case_per_skill_and_failure_per_error_with_safe_xml() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    ok(&home, &["init"]);
    let dir = temp.path().join("a&b").join("demo");
    skill(&dir, &format!("{DESCRIPTION} <bad>"));
    let output = run(
        &home,
        &["validate", dir.to_str().unwrap(), "--format", "junit"],
    );
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let xml = std::str::from_utf8(&output.stdout).unwrap();
    let report = quick_junit::Report::deserialize_from_str(xml).unwrap();
    assert_eq!(report.tests, 1);
    assert_eq!(report.failures, 1);
    assert!(xml.contains("a&amp;b"));
    assert!(xml.contains("type=\"O104\""));
    assert_eq!(xml.matches("<failure ").count(), 1);
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: demo\ndescription: ''\n---\nBody",
    )
    .unwrap();
    let output = run(
        &home,
        &["validate", dir.to_str().unwrap(), "--format", "junit"],
    );
    let xml = String::from_utf8(output.stdout).unwrap();
    assert_eq!(xml.matches("<testcase ").count(), 1);
    assert!(xml.matches("<failure ").count() >= 2);
}
#[test]
fn global_allow_list_is_honored_by_cli_and_seal_preserves_invalid_drafts() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    ok(&home, &["new", "demo"]);
    let mut registry = Registry::load(&RealFs, &home.join("registry.yml")).unwrap();
    registry.settings.validate.allow = vec!["O102".into()];
    registry.save(&RealFs, &home.join("registry.yml")).unwrap();
    ok(&home, &["validate", "demo@draft", "--strict"]);
    let path = home.join("drafts/demo/SKILL.md");
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("Describe what", "<Describe> what");
    fs::write(&path, &text).unwrap();
    let output = run(&home, &["seal", "demo"]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    assert!(!home.join("store/demo/0.1.0").exists());
    registry.settings.validate.allow.push("O104".into());
    registry.save(&RealFs, &home.join("registry.yml")).unwrap();
    ok(&home, &["seal", "demo"]);
    skillctl::store::stage::restore_write_mode(&RealFs, &home.join("store/demo/0.1.0")).unwrap();
}
#[test]
fn dry_run_quiet_explicit_format_and_missing_reference_tool() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let dir = temp.path().join("demo");
    skill(&dir, DESCRIPTION);
    let output = ok(
        &home,
        &[
            "validate",
            dir.to_str().unwrap(),
            "--dry-run",
            "--quiet",
            "--json",
        ],
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([])
    );
    assert!(!home.exists());
    let output = ok(
        &home,
        &[
            "validate",
            dir.to_str().unwrap(),
            "--quiet",
            "--format",
            "json",
        ],
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([])
    );
    let output = ok(&home, &["validate", dir.to_str().unwrap(), "--quiet"]);
    assert!(output.stdout.is_empty());
    let output = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("PATH", temp.path())
        .args(["validate", dir.to_str().unwrap(), "--with-reference"])
        .output()
        .unwrap();
    assert!(output.status.success());
}
#[test]
#[cfg(unix)]
fn reference_verdict_is_merged_without_disclosing_its_output() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let dir = temp.path().join("demo");
    skill(&dir, DESCRIPTION);
    let executable = temp.path().join("skills-ref");
    fs::write(
        &executable,
        "#!/bin/sh\necho secret-should-not-leak\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("PATH", temp.path())
        .args([
            "validate",
            dir.to_str().unwrap(),
            "--with-reference",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let findings: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(findings[0]["rule"], "R001");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("secret-should-not-leak"));
}
#[test]
fn validate_works_in_a_home_without_a_registry() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    fs::create_dir(&home).unwrap();
    let dir = temp.path().join("demo");
    skill(&dir, DESCRIPTION);
    ok(&home, &["validate", dir.to_str().unwrap(), "--strict"]);
    assert!(!home.join("registry.yml").exists());
}
