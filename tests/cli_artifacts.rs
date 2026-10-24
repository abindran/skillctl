//! Rule and command artifacts through the CLI: scaffold, seal, link by kind
//! into .cursor/rules and the two commands folders, status/doctor/exclude
//! coverage, removal, the global refusal, validation rules, and a hook's
//! companions pinned into every project.
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Env {
    _temp: tempfile::TempDir,
    home: PathBuf,
    user_home: PathBuf,
    repo: PathBuf,
}

fn env() -> Env {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let user_home = temp.path().join("userhome");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&user_home).unwrap();
    fs::create_dir(&repo).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap()
            .success()
    );
    Env {
        _temp: temp,
        home,
        user_home,
        repo,
    }
}

fn run(e: &Env, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &e.home)
        .env("HOME", &e.user_home)
        .current_dir(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}
fn ok(e: &Env, dir: &Path, args: &[&str]) -> Output {
    let out = run(e, dir, args);
    assert!(
        out.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap()
}
fn write_draft(e: &Env, name: &str, document: &str, text: &str) {
    fs::write(e.home.join(format!("drafts/{name}/{document}")), text).unwrap();
}

const RULE: &str = "---\nname: style\ndescription: Attach for every change so the agent checks the available skills before acting.\nalwaysApply: true\nmetadata:\n  version: '1.0.0'\n---\n\nCheck the skills before acting.\n";
const COMMAND: &str = "---\nname: checkpoint\ndescription: Write the open task, decisions and file list to a scratch note and say what to run next.\nargument-hint: \"[task]\"\nmetadata:\n  version: '1.0.0'\n---\n\nWrite .claude/scratch/$ARGUMENTS.md with the open task, decisions and files touched.\n";

#[test]
fn rules_and_commands_are_scaffolded_sealed_and_linked_by_kind() {
    let e = env();
    // Scaffolds exist for both kinds and validate clean.
    ok(&e, &e.repo, &["new", "--kind", "rule", "style"]);
    assert!(e.home.join("drafts/style/RULE.mdc").is_file());
    ok(&e, &e.repo, &["validate", "style@draft"]);
    write_draft(&e, "style", "RULE.mdc", RULE);
    ok(&e, &e.repo, &["seal", "style"]);
    ok(&e, &e.repo, &["new", "--kind", "command", "checkpoint"]);
    assert!(e.home.join("drafts/checkpoint/COMMAND.md").is_file());
    write_draft(&e, "checkpoint", "COMMAND.md", COMMAND);
    ok(&e, &e.repo, &["seal", "checkpoint"]);
    ok(&e, &e.repo, &["new", "demo"]);
    ok(&e, &e.repo, &["seal", "demo"]);
    let out = ok(&e, &e.repo, &["skills"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("style (rule)"), "{text}");
    assert!(text.contains("checkpoint (command)"), "{text}");

    // Attach with all three: each kind lands where its harness reads it.
    let out = ok(
        &e,
        &e.repo,
        &[
            "--json",
            "attach",
            "--new",
            "web",
            "--skills",
            "demo",
            "--skills",
            "style",
            "--skills",
            "checkpoint",
        ],
    );
    let links: Vec<String> = json(&out)["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            l["path"]
                .as_str()
                .unwrap()
                .strip_prefix(&format!(
                    "{}/",
                    fs::canonicalize(&e.repo).unwrap().display()
                ))
                .unwrap()
                .to_owned()
        })
        .collect();
    let mut sorted = links.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            ".claude/commands/checkpoint.md",
            ".claude/skills/demo",
            ".cursor/commands/checkpoint.md",
            ".cursor/rules/style.mdc",
        ]
    );
    let rule_link = e.repo.join(".cursor/rules/style.mdc");
    assert!(rule_link.is_symlink());
    assert_eq!(
        fs::canonicalize(&rule_link).unwrap(),
        fs::canonicalize(e.home.join("store/style/1.0.0/style/RULE.mdc")).unwrap()
    );
    assert!(
        fs::read_to_string(&rule_link)
            .unwrap()
            .contains("alwaysApply: true")
    );
    assert_eq!(
        fs::canonicalize(e.repo.join(".cursor/commands/checkpoint.md")).unwrap(),
        fs::canonicalize(e.home.join("store/checkpoint/1.0.0/checkpoint/COMMAND.md")).unwrap()
    );
    // Hidden from Git, visible to status, clean for doctor.
    let exclude = fs::read_to_string(e.repo.join(".git/info/exclude")).unwrap();
    assert!(exclude.contains(".cursor/rules/style.mdc"), "{exclude}");
    assert!(
        exclude.contains(".claude/commands/checkpoint.md"),
        "{exclude}"
    );
    let out = ok(&e, &e.repo, &["status"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("style@1.0.0 .cursor/rules: ok"), "{text}");
    assert!(
        text.contains("checkpoint@1.0.0 .claude/commands: ok"),
        "{text}"
    );
    assert!(
        text.contains("checkpoint@1.0.0 .cursor/commands: ok"),
        "{text}"
    );
    let out = ok(&e, &e.repo, &["--json", "doctor"]);
    assert_eq!(json(&out)["problems"], serde_json::json!([]));
    // A user's own rule file in the same folder is never touched.
    fs::write(
        e.repo.join(".cursor/rules/mine.mdc"),
        "---\ndescription: mine\n---\nkeep\n",
    )
    .unwrap();
    ok(&e, &e.repo, &["relink"]);
    assert_eq!(
        fs::read_to_string(e.repo.join(".cursor/rules/mine.mdc")).unwrap(),
        "---\ndescription: mine\n---\nkeep\n"
    );
    // Removing a pin removes exactly its links.
    ok(&e, &e.repo, &["remove", "checkpoint"]);
    assert!(!e.repo.join(".claude/commands/checkpoint.md").exists());
    assert!(!e.repo.join(".cursor/commands/checkpoint.md").exists());
    assert!(rule_link.is_symlink());
    // A broken rule link is a doctor problem that --fix repairs.
    fs::remove_file(&rule_link).unwrap();
    let out = run(&e, &e.repo, &["--json", "doctor"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json(&out)["problems"][0]["check"], "link-missing");
    ok(&e, &e.repo, &["doctor", "--fix"]);
    assert!(rule_link.is_symlink());
    // Rules and commands are project-level: global add refuses them.
    let out = run(&e, &e.repo, &["global", "add", "style"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("project-level"));
}

#[test]
fn rule_and_command_rules_fire_on_their_shapes() {
    let e = env();
    ok(&e, &e.repo, &["init"]);
    let rule = e.home.join("drafts/style");
    fs::create_dir_all(&rule).unwrap();
    let cases: Vec<(&str, String, Vec<&str>)> = vec![
        ("clean", RULE.to_owned(), vec![]),
        ("missing", String::new(), vec!["K201"]),
        (
            "fields",
            "---\nname: other\ndescription: ''\nalwaysApply: maybe\nglobs: 3\n---\n\n".to_owned(),
            vec!["K202", "K203", "K204", "K205", "K205", "K206"],
        ),
        (
            "budget",
            RULE.replace("Check the skills before acting.\n", &"line\n".repeat(101)),
            vec!["K207"],
        ),
    ];
    for (label, text, expect) in cases {
        if text.is_empty() {
            let _ = fs::remove_file(rule.join("RULE.mdc"));
            // An empty folder is reported as a skill missing SKILL.md.
            let out = run(&e, &e.repo, &["--json", "validate", rule.to_str().unwrap()]);
            assert_eq!(json(&out)[0]["rule"], "S001", "{label}");
            // A hook-less, skill-less folder with an unparsable RULE.mdc is K201.
            fs::write(rule.join("RULE.mdc"), "not frontmatter").unwrap();
            let out = run(&e, &e.repo, &["--json", "validate", rule.to_str().unwrap()]);
            assert_eq!(json(&out)[0]["rule"], expect[0], "{label}");
            continue;
        }
        fs::write(rule.join("RULE.mdc"), &text).unwrap();
        let out = run(&e, &e.repo, &["--json", "validate", rule.to_str().unwrap()]);
        let got: Vec<&str> = json(&out)
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["rule"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
            .leak()
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(got, expect, "{label}");
    }
    let command = e.home.join("drafts/checkpoint");
    fs::create_dir_all(&command).unwrap();
    fs::write(command.join("COMMAND.md"), COMMAND).unwrap();
    let out = run(
        &e,
        &e.repo,
        &["--json", "validate", command.to_str().unwrap()],
    );
    assert_eq!(json(&out), serde_json::json!([]));
    fs::write(
        command.join("COMMAND.md"),
        "---\nname: nope\ndescription: ''\n---\n\n",
    )
    .unwrap();
    let out = run(
        &e,
        &e.repo,
        &["--json", "validate", command.to_str().unwrap()],
    );
    let got: Vec<String> = json(&out)
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["rule"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(got, ["K302", "K303", "K304", "K305"]);
}

#[test]
fn hook_companions_are_pinned_into_every_project_and_removed_with_the_hook() {
    let e = env();
    ok(&e, &e.repo, &["new", "--kind", "rule", "style"]);
    write_draft(&e, "style", "RULE.mdc", RULE);
    ok(&e, &e.repo, &["seal", "style"]);
    ok(&e, &e.repo, &["new", "demo"]);
    ok(&e, &e.repo, &["seal", "demo"]);
    ok(&e, &e.repo, &["attach", "--new", "web", "--skills", "demo"]);
    ok(&e, &e.repo, &["new", "--kind", "hook", "prompt-hook"]);
    write_draft(
        &e,
        "prompt-hook",
        "HOOK.md",
        "---\nname: prompt-hook\ndescription: Injects a skill check before every prompt so the agent decides explicitly whether a skill applies.\nmetadata:\n  version: '0.1.0'\nevents: [UserPromptSubmit]\nentry: hook.py\ncompanions: [style]\n---\n# prompt-hook\n",
    );
    ok(&e, &e.repo, &["seal", "prompt-hook"]);
    let out = ok(&e, &e.repo, &["--json", "hook", "add", "prompt-hook"]);
    let report = json(&out);
    assert_eq!(report["companions"][0]["name"], "style");
    assert!(e.repo.join(".cursor/rules/style.mdc").is_symlink());
    let registry = fs::read_to_string(e.home.join("registry.yml")).unwrap();
    assert!(registry.contains("style: 1.0.0"), "{registry}");
    // Idempotent: a second add pins nothing new.
    let out = ok(&e, &e.repo, &["--json", "hook", "add", "prompt-hook"]);
    assert_eq!(json(&out)["companions"], serde_json::json!([]));
    // Removing the hook unpins the companion and its link goes with it.
    ok(&e, &e.repo, &["hook", "remove", "prompt-hook"]);
    assert!(!e.repo.join(".cursor/rules/style.mdc").exists());
    assert!(
        !fs::read_to_string(e.home.join("registry.yml"))
            .unwrap()
            .contains("style:")
    );
    // A companion missing from the store is a clear refusal.
    skillctl::store::stage::restore_write_mode(
        &skillctl::services::fs::RealFs,
        &e.home.join("store/style/1.0.0"),
    )
    .unwrap();
    fs::remove_dir_all(e.home.join("store/style")).unwrap();
    let out = run(&e, &e.repo, &["hook", "add", "prompt-hook"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Companion 'style' is not in the store"));
}
