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

#[test]
fn human_stub_error_has_readable_command_hint_and_failure_status() {
    let output = run(&["global", "add", "demo"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("Error: Command 'global add' is not implemented\nHint: "));
    assert!(stderr.contains("skillctl --help"));
    assert!(!stderr.contains("Add {"));
}

#[test]
fn json_stub_error_is_one_document_on_stdout() {
    for args in [
        vec!["--json", "global", "add", "demo"],
        vec!["global", "add", "demo", "--json", "--quiet"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["error"], "not_implemented");
        assert_eq!(report["message"], "Command 'global add' is not implemented");
        assert_eq!(report["exit_code"], 1);
        assert!(report["hint"].as_str().unwrap().contains("skillctl --help"));
    }
}

#[test]
fn quiet_does_not_hide_human_errors() {
    let output = run(&["--quiet", "status"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains("Hint:"));
}

#[test]
fn config_stub_uses_nested_command_name() {
    let output = run(&["--json", "config", "get", "link_targets"]);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["message"], "Command 'config get' is not implemented");
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
