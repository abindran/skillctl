//! Binary-level coverage of reporting, streams, and process exit status.
use std::process::{Command, Output, Stdio};

fn run(args: &[&str]) -> Output {
    let temp = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .args(args)
        .env("SKILLCTL_HOME", temp.path().join("skills"))
        .stdin(Stdio::null())
        .output()
        .expect("run skillctl")
}

// `install demo` with no folder called demo is a Git URL that cannot be cloned:
// a real, offline, deterministic failure.
#[test]
fn human_error_has_readable_message_hint_and_failure_status() {
    let output = run(&["install", "demo"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("Error: clone demo"), "{stderr}");
    assert!(stderr.contains("\nHint: "), "{stderr}");
    assert!(!stderr.contains("Io {"), "{stderr}");
}

#[test]
fn json_error_is_one_document_on_stdout() {
    for args in [
        vec!["--json", "install", "demo"],
        vec!["install", "demo", "--json", "--quiet"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["error"], "io");
        assert!(
            report["message"]
                .as_str()
                .unwrap()
                .starts_with("clone demo")
        );
        assert_eq!(report["exit_code"], 1);
        assert!(!report["hint"].as_str().unwrap().is_empty());
    }
}

#[test]
fn quiet_does_not_hide_human_errors() {
    let output = run(&["--quiet", "edit", "demo"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains("Hint:"));
}

#[test]
fn config_get_reports_defaults_as_json() {
    let output = run(&["--json", "config", "get", "git_exclude"]);
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["key"], "git_exclude");
    assert_eq!(report["value"], true);
}

#[test]
fn clap_usage_errors_remain_text_and_exit_two() {
    let output = run(&["--json", "attach", "--project", "a", "--new", "b"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("cannot be used with")
    );
}

#[test]
fn help_and_version_still_succeed() {
    for flag in ["--help", "--version"] {
        let output = run(&[flag]);
        assert_eq!(output.status.code(), Some(0));
        assert!(!output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}
