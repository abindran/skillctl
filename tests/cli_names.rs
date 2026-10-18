//! Name validation at the public CLI boundary.
use std::process::{Command, Stdio};

#[test]
fn invalid_plain_names_are_usage_errors() {
    for args in [
        vec!["new", "Invalid"],
        vec!["remove", "a--b"],
        vec!["global", "remove", "a_b"],
        vec!["seal", "bad.name", "--bump", "patch"],
        vec!["attach", "--new", "Bad"],
        vec!["attach", "--project", "bad.name"],
        vec!["add", "demo", "--project", "Bad"],
        vec!["remove", "demo", "--project", "Bad"],
        vec!["relink", "--project", "Bad"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_skillctl"))
            .args(&args)
            .env_remove("SKILLCTL_HOME")
            .stdin(Stdio::null())
            .output()
            .expect("run skillctl");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("Invalid"), "{args:?}: {stderr}");
        assert!(stderr.contains("name"), "{args:?}: {stderr}");
    }
}

#[test]
fn valid_names_and_versioned_arguments_still_reach_command_stubs() {
    for args in [
        vec!["new", "org-code-review"],
        vec!["remove", "org-code-review"],
        vec!["global", "remove", "demo"],
        vec!["seal", "demo", "--bump", "patch"],
        vec!["attach", "--new", "web-app"],
        vec!["attach", "--project", "web-app", "--skills", "a,b@1.2.0"],
        vec!["add", "demo@1.2.0", "--project", "web-app"],
        vec!["edit", "demo@1.2.0"],
        vec!["global", "add", "demo@1.2.0"],
        vec!["relink", "--project", "web-app"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_skillctl"))
            .args(&args)
            .env_remove("SKILLCTL_HOME")
            .stdin(Stdio::null())
            .output()
            .expect("run skillctl");
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("not implemented"),
            "{args:?}"
        );
    }
}
