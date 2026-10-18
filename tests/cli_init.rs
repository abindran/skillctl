//! Initialization through the public executable, with isolated homes for every test.
use std::path::Path;
use std::process::{Command, Output, Stdio};

use skillctl::registry::Registry;
use skillctl::services::fs::RealFs;

fn invoke(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn assert_layout(home: &Path) {
    for path in ["store", "drafts", "templates/default"] {
        assert!(home.join(path).is_dir(), "{path}");
    }
    assert!(home.join("registry.lock").is_file());
    assert_eq!(
        Registry::load(&RealFs, &home.join("registry.yml")).unwrap(),
        Registry::default()
    );
    let template = std::fs::read_to_string(home.join("templates/default/SKILL.md")).unwrap();
    let rendered = template.replace("{{name}}", "demo");
    let frontmatter = rendered.split("---").nth(1).unwrap();
    let fields: serde_yaml_ng::Value = serde_yaml_ng::from_str(frontmatter).unwrap();
    assert_eq!(fields["name"].as_str(), Some("demo"));
    assert_eq!(fields["metadata"]["version"].as_str(), Some("0.1.0"));
    assert!(fields["description"].as_str().unwrap().len() >= 80);
}

#[test]
fn init_creates_nested_home_and_second_run_changes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("nested/skills");
    let first = invoke(&home, &["--json", "init"]);
    assert!(first.status.success(), "{:?}", first);
    assert!(first.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert!(!report["created"].as_array().unwrap().is_empty());
    assert_layout(&home);
    let files = [
        "registry.yml",
        "templates/default/SKILL.md",
        "registry.lock",
    ];
    let before: Vec<_> = files
        .iter()
        .map(|p| {
            let path = home.join(p);
            (
                std::fs::read(&path).unwrap(),
                std::fs::metadata(path).unwrap().modified().unwrap(),
            )
        })
        .collect();
    let second = invoke(&home, &["--json", "--quiet", "init"]);
    assert!(second.status.success());
    let report: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(report["created"], serde_json::json!([]));
    for (path, (bytes, modified)) in files.iter().zip(before) {
        assert_eq!(std::fs::read(home.join(path)).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(home.join(path))
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
    }
    assert!(!home.join("registry.yml.bak-1").exists());
}

#[test]
fn partial_layout_is_completed_without_touching_existing_files() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    std::fs::create_dir_all(home.join("templates/default")).unwrap();
    // Even malformed state must not be silently reset by init.
    std::fs::write(home.join("registry.yml"), "custom or unsupported registry").unwrap();
    std::fs::write(home.join("templates/default/SKILL.md"), "custom template").unwrap();
    let output = invoke(&home, &["init"]);
    assert!(output.status.success());
    assert!(home.join("store").is_dir());
    assert!(home.join("drafts").is_dir());
    assert_eq!(
        std::fs::read_to_string(home.join("registry.yml")).unwrap(),
        "custom or unsupported registry"
    );
    assert_eq!(
        std::fs::read_to_string(home.join("templates/default/SKILL.md")).unwrap(),
        "custom template"
    );
}

#[test]
fn dry_run_lists_paths_without_creating_even_the_lock() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("nested/skills");
    for args in [
        vec!["--dry-run", "init"],
        vec!["--dry-run", "--json", "init"],
    ] {
        let output = invoke(&home, &args);
        assert!(output.status.success(), "{:?}", output);
        assert!(!temp.path().join("nested").exists());
        assert!(String::from_utf8_lossy(&output.stdout).contains("SKILL.md"));
    }
}

#[test]
fn other_commands_initialize_only_a_missing_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let output = invoke(&home, &["--json", "status"]);
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["attached"], false);
    assert_layout(&home);
    std::fs::remove_file(home.join("templates/default/SKILL.md")).unwrap();
    let output = invoke(&home, &["status"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(!home.join("templates/default/SKILL.md").exists());
}

#[test]
fn filesystem_conflicts_fail_clearly_without_overwriting() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(home.join("store"), "not a directory").unwrap();
    let output = invoke(&home, &["--json", "init"]);
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["error"], "io");
    assert!(report["message"].as_str().unwrap().contains("store"));
    assert_eq!(
        std::fs::read_to_string(home.join("store")).unwrap(),
        "not a directory"
    );
}

#[test]
fn simultaneous_initializers_converge_on_one_complete_layout() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("missing/skills");
    let mut children: Vec<_> = (0..6)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_skillctl"))
                .env("SKILLCTL_HOME", &home)
                .args(["--quiet", "init"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children.drain(..) {
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{:?}", output);
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    assert_layout(&home);
    assert!(!home.join("registry.yml.bak-1").exists());
}

#[cfg(unix)]
#[test]
fn existing_symlinks_are_never_replaced() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let actual = temp.path().join("actual");
    std::fs::create_dir(&actual).unwrap();
    symlink(&actual, &home).unwrap();
    let template_dir = actual.join("templates/default");
    std::fs::create_dir_all(&template_dir).unwrap();
    let missing = temp.path().join("user-template");
    symlink(&missing, template_dir.join("SKILL.md")).unwrap();
    let output = invoke(&home, &["init"]);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        std::fs::read_link(template_dir.join("SKILL.md")).unwrap(),
        missing
    );
    assert!(!missing.exists());
    assert_eq!(std::fs::read_link(&home).unwrap(), actual);
}
