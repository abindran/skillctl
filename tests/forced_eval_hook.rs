//! The bundled skill-forced-eval hook: validates and installs, shortlists by
//! phrase, path and keywords over the skills the agent sees, and stays silent
//! in Cursor, without skills, and for prompts that match nothing.
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn hook_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("hooks/skill-forced-eval")
}

fn python() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("python3"))
        .find(|path| path.is_file())
}

fn skill(root: &Path, name: &str, description: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\nmetadata:\n  version: '1.0.0'\n---\n# {name}\n"),
    )
    .unwrap();
}

fn run(cwd: &Path, user_home: &Path, prompt: &str, cursor: bool) -> (String, String) {
    let mut command = Command::new(hook_dir().join("hook.py"));
    command
        .env("HOME", user_home)
        .env("SKILLCTL_FORCED_EVAL_DEBUG", "1")
        .env_remove("CURSOR_VERSION")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if cursor {
        command.env("CURSOR_VERSION", "1.2.3");
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"session_id": "s", "cwd": cwd, "hook_event_name": "UserPromptSubmit", "prompt": prompt})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn forced_eval_hook_validates_and_installs() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("HOME", temp.path())
        .args(["install"])
        .arg(hook_dir())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", &home)
        .env("HOME", temp.path())
        .args(["validate", "skill-forced-eval@1.0.0"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "0 error(s), 0 warning(s)"
    );
}

#[test]
fn shortlist_matches_phrases_paths_and_keywords_and_stays_silent_otherwise() {
    if python().is_none() {
        eprintln!("python3 not on PATH; skipping");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let user_home = temp.path().join("home");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&user_home).unwrap();

    // No skills visible: silent.
    let (out, err) = run(
        &project,
        &user_home,
        "add a column to the orders table",
        false,
    );
    assert_eq!(out, "");
    assert!(err.contains("no skills visible"), "{err}");

    skill(
        &project.join(".claude/skills"),
        "db-migrations",
        r#"ALWAYS invoke this skill when the user asks to create, modify or roll back a database migration or schema change, including indirect phrasing like "add a column" or "rename this field". Do not write migration files directly without this skill."#,
    );
    skill(
        &project.join(".agents/skills"),
        "ember-client",
        r#"ALWAYS invoke this skill when the user asks to change any .hbs template or .scss style under jsapps/, including requests like "fix this template". Do not edit jsapps/ directly without this skill."#,
    );
    skill(
        &user_home.join(".claude/skills"),
        "release-notes",
        r#"ALWAYS invoke this skill when the user asks to write or update release notes or a changelog entry, including "what changed since the last tag". Do not write release notes without this skill."#,
    );

    // A quoted trigger phrase: one candidate, scored above the rest.
    let (out, _) = run(
        &project,
        &user_home,
        "please add a column to the orders table",
        false,
    );
    assert_eq!(
        out,
        "MANDATORY SKILL CHECK. Candidates: db-migrations\nFor each: [name] - YES/NO - [reason]. Call Skill(name) for every YES before any other tool.\n"
    );
    // A path mention reaches a skill through its paths, not its words.
    let (out, err) = run(
        &project,
        &user_home,
        "the dropdown in jsapps/zohoone/app/templates/orgs.hbs is misaligned",
        false,
    );
    assert!(out.contains("Candidates: ember-client"), "{out}\n{err}");
    assert!(err.contains("path "), "{err}");
    // Plurals and verb forms count as the same keyword.
    let (out, err) = run(
        &project,
        &user_home,
        "we need two new columns on the schemas for migrating orders",
        false,
    );
    assert!(out.contains("db-migrations"), "{out}\n{err}");
    // Shared keywords alone need at least two.
    let (out, _) = run(
        &project,
        &user_home,
        "draft the changelog entry and release notes for 2.1",
        false,
    );
    assert!(out.contains("release-notes"), "{out}");
    let (out, err) = run(
        &project,
        &user_home,
        "what is a migration in general terms?",
        false,
    );
    assert_eq!(out, "", "one keyword is not enough: {err}");
    // A prompt that matches nothing is silent.
    let (out, err) = run(
        &project,
        &user_home,
        "explain the difference between a Vec and a VecDeque",
        false,
    );
    assert_eq!(out, "");
    assert!(err.contains("no candidate among 3 skill(s)"), "{err}");
    // Cursor: silent before anything is read.
    let (out, err) = run(
        &project,
        &user_home,
        "add a column to the orders table",
        true,
    );
    assert_eq!(out, "");
    assert_eq!(err, "");
    // Malformed input exits 0 silently.
    let mut child = Command::new(hook_dir().join("hook.py"))
        .env("HOME", &user_home)
        .current_dir(&project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"garbage").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
}
