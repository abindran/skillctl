use skillctl::{
    registry::{Instance, Pin, Project, Registry},
    services::fs::RealFs,
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

#[test]
fn empty_text_and_json_snapshots_and_dry_run_do_not_create_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    for (command, expected) in [
        ("skills", "No installed skills.\n"),
        ("projects", "No projects.\n"),
    ] {
        let output = run(&home, &["--dry-run", command]);
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
        assert!(!home.exists());
        let output = run(&home, &["--dry-run", "--quiet", "--json", command]);
        assert!(output.status.success());
        assert_eq!(output.stdout, b"[]\n");
        assert!(!home.exists());
    }
}

#[test]
fn populated_text_and_json_are_deterministic_and_show_usage_and_missing_instances() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    assert!(run(&home, &["init"]).status.success());
    for path in [
        "store/demo/1.0.0",
        "store/demo/2.0.0",
        "store/global-demo/1.0.0",
    ] {
        std::fs::create_dir_all(home.join(path)).unwrap();
    }
    let live = std::fs::canonicalize(temp.path()).unwrap().join("live");
    std::fs::create_dir(&live).unwrap();
    let missing = live.with_file_name("missing");
    let mut registry = Registry::default();
    let mut project = Project::default();
    project.skills.insert(
        "demo".parse().unwrap(),
        Pin::Version("1.0.0".parse().unwrap()),
    );
    project.instances = [live.clone(), missing.clone()]
        .into_iter()
        .map(|path| Instance {
            path,
            attached_at: "2026-10-18T09:12:00Z".parse().unwrap(),
            created_dirs: vec![],
        })
        .collect();
    registry.projects.insert("web".parse().unwrap(), project);
    registry.global.skills.insert(
        "global-demo".parse().unwrap(),
        Pin::Version("1.0.0".parse().unwrap()),
    );
    registry.save(&RealFs, &home.join("registry.yml")).unwrap();
    let before = std::fs::read(home.join("registry.yml")).unwrap();
    let output = run(&home, &["skills"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "demo\n  1.0.0 — web\n  2.0.0 — unused\nglobal-demo\n  1.0.0 — global\n"
    );
    let output = run(&home, &["--json", "skills"]);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json,
        serde_json::json!([
            {"name":"demo", "kind":"skill", "versions":[{"version":"1.0.0","projects":["web"],"global":false},{"version":"2.0.0","projects":[],"global":false}]},
            {"name":"global-demo", "kind":"skill", "versions":[{"version":"1.0.0","projects":[],"global":true}]}
        ])
    );
    let output = run(&home, &["projects"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "web (1 skills, 2 instances)\n  {}\n  {} [missing]\n",
            live.display(),
            missing.display()
        )
    );
    let output = run(&home, &["--json", "projects"]);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json,
        serde_json::json!([{"name":"web","skills":{"demo":"1.0.0"},"instances":[
        {"path":live,"attached_at":"2026-10-18T09:12:00Z","created_dirs":[]},
        {"path":missing,"attached_at":"2026-10-18T09:12:00Z","created_dirs":[]}
    ],"missing_instances":[missing]}])
    );
    assert_eq!(std::fs::read(home.join("registry.yml")).unwrap(), before);
    for command in ["skills", "projects"] {
        let output = run(&home, &["--quiet", command]);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
    }
}
