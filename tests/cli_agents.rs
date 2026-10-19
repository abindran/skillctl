//! Agent artifacts: scaffold, validate (H220–H227), seal, link into
//! .claude/agents, derive a Cursor override that links into .cursor/agents,
//! and the four bundled agents validating clean.
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

fn run(e: &Env, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &e.home)
        .env("HOME", &e.user_home)
        .current_dir(&e.repo)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}
fn ok(e: &Env, args: &[&str]) -> Output {
    let out = run(e, args);
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
fn rules(out: &Output) -> Vec<String> {
    json(out)
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["rule"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn agents_are_scaffolded_validated_linked_and_derived_for_cursor() {
    let e = env();
    // The scaffold is a valid agent apart from the Claude alias warning.
    ok(&e, &["new", "--kind", "agent", "explorer"]);
    let draft = e.home.join("drafts/explorer");
    assert!(draft.join("AGENT.md").is_file());
    let out = run(&e, &["--json", "validate", "explorer@draft"]);
    assert_eq!(rules(&out), ["H222"]);
    ok(&e, &["seal", "explorer"]);
    ok(&e, &["attach", "--new", "web", "--skills", "explorer"]);
    let link = e.repo.join(".claude/agents/explorer.md");
    assert!(link.is_symlink());
    assert_eq!(
        fs::canonicalize(&link).unwrap(),
        fs::canonicalize(e.home.join("store/explorer/0.1.0/explorer/AGENT.md")).unwrap()
    );
    assert!(!e.repo.join(".cursor/agents").exists());
    let out = ok(&e, &["status"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("explorer@0.1.0 .claude/agents: ok"));

    // derive needs a draft; after edit it writes cursor.md, which links to
    // .cursor/agents and clears H222.
    let out = run(
        &e,
        &["agent", "derive", "explorer", "--model", "gpt-5-mini"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("run skillctl edit explorer"));
    ok(&e, &["edit", "explorer"]);
    let out = ok(
        &e,
        &[
            "--json",
            "agent",
            "derive",
            "explorer",
            "--model",
            "gpt-5-mini",
        ],
    );
    assert_eq!(json(&out)["model"], "gpt-5-mini");
    let cursor = fs::read_to_string(draft.join("cursor.md")).unwrap();
    assert!(cursor.contains("model: gpt-5-mini"), "{cursor}");
    assert!(cursor.contains("readonly: true"), "{cursor}");
    assert!(
        cursor.contains("You investigate; you never edit."),
        "{cursor}"
    );
    let out = run(&e, &["--json", "validate", "explorer@draft"]);
    assert_eq!(rules(&out), Vec::<String>::new());
    ok(&e, &["seal", "explorer"]);
    ok(&e, &["add", "explorer@0.1.1"]);
    let cursor_link = e.repo.join(".cursor/agents/explorer.md");
    assert!(cursor_link.is_symlink());
    assert_eq!(
        fs::canonicalize(&cursor_link).unwrap(),
        fs::canonicalize(e.home.join("store/explorer/0.1.1/explorer/cursor.md")).unwrap()
    );
    let exclude = fs::read_to_string(e.repo.join(".git/info/exclude")).unwrap();
    assert!(exclude.contains(".cursor/agents/explorer.md"), "{exclude}");
    // Removing the pin removes both links.
    ok(&e, &["remove", "explorer"]);
    assert!(!link.exists());
    assert!(!cursor_link.exists());
}

#[test]
fn agent_rules_fire_on_their_shapes() {
    let e = env();
    ok(&e, &["init"]);
    let dir = e.home.join("drafts/helper");
    fs::create_dir_all(&dir).unwrap();
    let write = |text: &str| fs::write(dir.join("AGENT.md"), text).unwrap();
    let target = dir.to_str().unwrap().to_owned();
    write("not frontmatter");
    let out = run(&e, &["--json", "validate", &target]);
    assert_eq!(rules(&out), ["H220"]);
    // Scoped tools without readonly, Claude alias without override, bad types.
    write(
        "---\nname: Other:Name\ndescription: ''\ntools: 3\nreadonly: maybe\nskills: 4\nmaxTurns: 0\nmodel: 7\n---\n\n",
    );
    let out = run(&e, &["--json", "validate", &target]);
    let mut got = rules(&out);
    got.sort();
    let mut expect: Vec<String> = [
        "H221", "H223", "H223", "H223", "H224", "H225", "H225", "H225", "H225", "H225",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    expect.sort();
    assert_eq!(got, expect);
    // Case-insensitive name match, H222 only, then cleared by a matching
    // override; a stale override is H226; bad schema.json is H227.
    write(
        "---\nname: Helper\ndescription: Use proactively to look things up without editing.\ntools: Read\nreadonly: true\nmodel: haiku\nmetadata:\n  version: '1.0.0'\n---\n\nLook things up.\n",
    );
    let out = run(&e, &["--json", "validate", &target]);
    assert_eq!(rules(&out), ["H222"]);
    fs::write(
        dir.join("cursor.md"),
        "---\nname: Helper\ndescription: Use proactively to look things up without editing.\nmodel: gpt-5-mini\nreadonly: true\n---\n\nLook things up.\n",
    )
    .unwrap();
    let out = run(&e, &["--json", "validate", &target]);
    assert_eq!(rules(&out), Vec::<String>::new());
    fs::write(
        dir.join("cursor.md"),
        "---\nname: Helper\ndescription: x\nmodel: gpt-5-mini\n---\n\nSomething else entirely.\n",
    )
    .unwrap();
    let out = run(&e, &["--json", "validate", &target]);
    assert_eq!(rules(&out), ["H226"]);
    fs::remove_file(dir.join("cursor.md")).unwrap();
    fs::write(dir.join("schema.json"), "[1, 2]").unwrap();
    let out = run(&e, &["--json", "validate", &target]);
    assert_eq!(rules(&out), ["H222", "H227"]);
}

#[test]
fn bundled_agents_validate_and_install() {
    let e = env();
    let agents = Path::new(env!("CARGO_MANIFEST_DIR")).join("agents");
    let out = ok(
        &e,
        &["--json", "install", agents.to_str().unwrap(), "--all"],
    );
    let names: Vec<String> = json(&out)["installed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        names,
        ["explore", "log-digester", "repo-explorer", "reviewer"]
    );
    for name in names {
        let out = run(&e, &["--json", "validate", &format!("{name}@1.0.0")]);
        // Only the Claude alias warning remains until a cursor.md is derived.
        assert_eq!(rules(&out), ["H222"], "{name}");
    }
    ok(&e, &["attach", "--new", "web", "--skills", "explore"]);
    let text = fs::read_to_string(e.repo.join(".claude/agents/explore.md")).unwrap();
    assert!(text.starts_with("---\nname: Explore\n"));
}
