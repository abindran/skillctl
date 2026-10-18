//! Context construction boundaries in the real executable.
use std::process::{Command, Stdio};

#[test]
fn explicit_home_overrides_environment_without_creating_state() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("explicit");
    let environment = temp.path().join("environment");
    for dry_run in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_skillctl"));
        command
            .env("SKILLCTL_HOME", &environment)
            .arg("--home")
            .arg(&home)
            .arg("--json");
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.arg("status").stdin(Stdio::null()).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["error"], "not_implemented");
        assert!(!home.exists());
        assert!(!environment.exists());
    }
}

#[test]
fn relative_home_and_environment_home_are_accepted_without_initialization() {
    let temp = tempfile::tempdir().unwrap();
    for explicit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_skillctl"));
        command
            .current_dir(temp.path())
            .env("SKILLCTL_HOME", "skills");
        if explicit {
            command.args(["--home", "skills"]);
        }
        let output = command
            .args(["--json", "--quiet", "status"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["error"], "not_implemented");
        assert!(!temp.path().join("skills").exists());
    }
}
