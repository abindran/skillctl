//! Advisory output around pins: description warnings and the index budget on
//! add/global add/attach, status --budget, and the hook add warning.
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
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

/// Seal a skill with the given description straight into the store.
fn seal(e: &Env, name: &str, description: &str) {
    ok(e, &["new", name]);
    fs::write(
        e.home.join(format!("drafts/{name}/SKILL.md")),
        format!("---\nname: {name}\ndescription: {description}\nmetadata:\n  version: '1.0.0'\n---\n# {name}\n"),
    )
    .unwrap();
    ok(e, &["seal", name]);
}

const GOOD: &str = r#"ALWAYS invoke this skill when the user asks to review a change, including requests like "review this diff" or "check my PR". Do not approve a change without this skill."#;
const WEAK: &str = "Helps with database migrations and more.";

#[test]
fn add_reports_description_warnings_and_the_budget_on_stderr_and_in_json() {
    let e = env();
    seal(&e, "review", GOOD);
    seal(&e, "migrate", WEAK);
    ok(&e, &["attach", "--new", "web", "--skills", "review"]);

    // A clean skill: no stderr note at all.
    let out = ok(&e, &["remove", "review"]);
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = ok(&e, &["add", "review"]);
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // A weak one: O140–O144 and the budget line on stderr; stdout unchanged.
    let out = ok(&e, &["add", "migrate"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    for rule in ["O140", "O141", "O142", "O144"] {
        assert!(stderr.contains(rule), "{stderr}");
    }
    assert!(stderr.contains("Index: 2 skill(s)"), "{stderr}");
    assert!(stderr.contains("/ 2000 tokens"), "{stderr}");
    assert!(
        String::from_utf8_lossy(&out.stdout).starts_with("Updated project pins"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );

    // JSON: nothing on stderr, the same data under advisory.
    ok(&e, &["remove", "migrate"]);
    let out = ok(&e, &["--json", "add", "migrate"]);
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json(&out);
    let rules: Vec<&str> = report["advisory"]["descriptions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["rule"].as_str().unwrap())
        .collect();
    assert_eq!(rules, ["O140", "O141", "O142", "O144"]);
    assert_eq!(
        report["advisory"]["index"]["skills"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(report["advisory"]["index"]["budget_tokens"], 2000);
    assert_eq!(report["advisory"]["index"]["over_budget"], false);

    // status --budget lists both and the total; plain status is unchanged.
    let out = ok(&e, &["status"]);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("Skill index"));
    let out = ok(&e, &["status", "--budget"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Skill index:"), "{text}");
    assert!(text.contains("review@1.0.0:"), "{text}");
    assert!(text.contains("migrate@1.0.0:"), "{text}");
    assert!(text.contains("/ 2000 tokens"), "{text}");
    // The default targets are both read by Cursor, which lists each skill twice.
    assert!(
        text.contains("Cursor reads .agents/skills and .claude/skills (2 folders), so it lists each skill 2 times"),
        "{text}"
    );
    let out = ok(&e, &["--json", "status", "--budget"]);
    let report = json(&out);
    assert_eq!(report["budget"]["skills"].as_array().unwrap().len(), 2);
    assert!(report["budget"]["tokens"].as_u64().unwrap() > 0);
    assert_eq!(report["budget"]["cursor_copies"], 2);
    // One Cursor-visible target: no note.
    ok(
        &e,
        &["--yes", "config", "set", "link_targets", ".claude/skills"],
    );
    let out = ok(&e, &["status", "--budget"]);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("Cursor reads"));
}

#[test]
fn global_add_attach_and_an_over_budget_listing_are_reported() {
    let e = env();
    seal(&e, "weak", WEAK);
    // A description just under the spec limit; several push the index over 1%.
    let long = format!(
        r#"ALWAYS invoke this skill when the user asks to do the thing, including requests like "do it" or "run it". Do not do it without this skill. {}"#,
        "Detail. ".repeat(100)
    );
    assert!(long.chars().count() <= 1024);
    for name in [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota",
    ] {
        seal(&e, name, &long);
    }
    let out = ok(&e, &["global", "add", "weak"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("O140"), "{stderr}");
    assert!(stderr.contains("Index: 1 skill(s)"), "{stderr}");
    // --skills takes one name per flag.
    let mut args = vec!["--json", "attach", "--new", "web"];
    for name in [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota",
    ] {
        args.extend(["--skills", name]);
    }
    let out = ok(&e, &args);
    let report = json(&out);
    // Nine long descriptions plus the global one: over the 2,000-token budget.
    assert_eq!(
        report["advisory"]["index"]["skills"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    assert_eq!(report["advisory"]["index"]["over_budget"], true);
    let out = ok(&e, &["status", "--budget"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("O143 warning: over budget"), "{text}");
}

#[test]
fn hook_add_on_an_activation_hook_warns_about_weak_pinned_descriptions() {
    let e = env();
    seal(&e, "weak", WEAK);
    ok(&e, &["attach", "--new", "web", "--skills", "weak"]);
    ok(&e, &["new", "--kind", "hook", "prompt-hook"]);
    fs::write(
        e.home.join("drafts/prompt-hook/HOOK.md"),
        "---\nname: prompt-hook\ndescription: Injects a skill check before every prompt so the agent decides explicitly whether a skill applies.\nmetadata:\n  version: '0.1.0'\nevents: [UserPromptSubmit]\nentry: hook.py\n---\n# prompt-hook\n",
    )
    .unwrap();
    ok(&e, &["seal", "prompt-hook"]);
    let out = ok(&e, &["hook", "add", "prompt-hook"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("1 pinned skill(s) fail the directive-description rules"),
        "{stderr}"
    );
    assert!(stderr.contains("weak"), "{stderr}");
    // A hook without UserPromptSubmit says nothing about descriptions.
    ok(&e, &["hook", "remove", "prompt-hook"]);
    ok(&e, &["new", "--kind", "hook", "logger"]);
    ok(&e, &["seal", "logger"]);
    let out = ok(&e, &["hook", "add", "logger"]);
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = ok(&e, &["--json", "hook", "add", "prompt-hook"]);
    assert!(out.stderr.is_empty());
    assert_eq!(
        json(&out)["advisory"]["descriptions"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}
