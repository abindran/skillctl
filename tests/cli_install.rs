use skillctl::{
    services::fs::RealFs,
    store::meta::{Metadata, Source},
};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const DESCRIPTION: &str = "Use when reviewing a software change to provide consistent, actionable, and carefully verified engineering guidance.";

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .env("SKILLCTL_HOME", home)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}
fn ok(home: &Path, args: &[&str]) -> Output {
    let output = run(home, args);
    assert!(
        output.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.test"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn skill(dir: &Path, version: Option<&str>) {
    fs::create_dir_all(dir).unwrap();
    let name = dir.file_name().unwrap().to_str().unwrap();
    let metadata = version.map_or(String::new(), |v| format!("metadata:\n  version: '{v}'\n"));
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {DESCRIPTION}\n{metadata}---\nReview the change carefully.\n"),
    )
    .unwrap();
}

#[test]
fn installs_from_local_folders_single_multiple_and_subdir() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let src = temp.path().join("src");
    skill(&src.join("alpha"), Some("1.0.0"));
    skill(&src.join("nested/beta"), Some("2.0.0"));
    let source = src.to_str().unwrap();
    // Two skills, no terminal, no --all: the multi-select cannot be answered.
    let out = run(&home, &["install", source]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--all"));
    assert!(!home.join("store/alpha").exists());
    let out = ok(&home, &["install", source, "--all", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["installed"].as_array().unwrap().len(), 2);
    let alpha = home.join("store/alpha/1.0.0");
    assert!(alpha.join("alpha/SKILL.md").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(alpha.join("alpha/SKILL.md"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o444
        );
    }
    let meta = Metadata::load(&RealFs, &alpha.join("meta.json")).unwrap();
    assert_eq!(meta.source, Source::Local);
    assert_eq!(meta.version.to_string(), "1.0.0");
    assert!(home.join("store/beta/2.0.0/beta/SKILL.md").is_file());
    // Same content again: reported, exit 0, nothing rewritten.
    let out = ok(&home, &["install", source, "--path", "nested/beta"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Already installed beta@2.0.0"));
    // Same version, different bytes: refused.
    fs::write(
        src.join("nested/beta/SKILL.md"),
        fs::read_to_string(src.join("nested/beta/SKILL.md"))
            .unwrap()
            .replace("carefully", "very carefully"),
    )
    .unwrap();
    let out = run(&home, &["install", source, "--path", "nested/beta"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("different content"));
    let out = ok(&home, &["skills", "--json"]);
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    // An invalid --path is a usage error.
    assert_eq!(
        run(&home, &["install", source, "--path", "../escape"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run(&home, &["install", source, "--path", "missing"])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn version_flag_fills_missing_metadata_and_mismatches_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let unversioned = temp.path().join("src/gamma");
    skill(&unversioned, None);
    let out = run(&home, &["install", unversioned.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("O101"));
    ok(
        &home,
        &[
            "install",
            unversioned.to_str().unwrap(),
            "--version",
            "1.2.3",
        ],
    );
    let stored = home.join("store/gamma/1.2.3/gamma/SKILL.md");
    assert!(
        fs::read_to_string(&stored)
            .unwrap()
            .contains("version: 1.2.3")
    );
    // The source itself was never modified.
    assert!(
        !fs::read_to_string(unversioned.join("SKILL.md"))
            .unwrap()
            .contains("1.2.3")
    );
    let versioned = temp.path().join("other/delta");
    skill(&versioned, Some("1.0.0"));
    let out = run(
        &home,
        &["install", versioned.to_str().unwrap(), "--version", "2.0.0"],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not match"));
    ok(
        &home,
        &["install", versioned.to_str().unwrap(), "--version", "1.0.0"],
    );
    assert!(home.join("store/delta/1.0.0").is_dir());
}

#[test]
fn installs_from_a_git_tag_and_records_provenance() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    skill(&repo.join("skills/gamma"), Some("0.5.0"));
    fs::write(repo.join("README.md"), "not a skill\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "add gamma"]);
    git(&repo, &["tag", "v0.5.0"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]).trim().to_owned();
    // A second commit after the tag proves --ref selects the tag.
    fs::write(repo.join("README.md"), "changed later\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "later"]);
    let url = format!("file://{}", repo.canonicalize().unwrap().display());
    ok(&home, &["install", &url, "--ref", "v0.5.0"]);
    let version = home.join("store/gamma/0.5.0");
    assert!(version.join("gamma/SKILL.md").is_file());
    let meta = Metadata::load(&RealFs, &version.join("meta.json")).unwrap();
    assert_eq!(
        meta.source,
        Source::Git {
            url: url.clone(),
            reference: Some("v0.5.0".into()),
            commit: commit.clone(),
            subdir: Some("skills/gamma".into()),
        }
    );
    let out = ok(&home, &["install", &url, "--ref", "v0.5.0"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Already installed gamma@0.5.0"));
    // Unknown references fail cleanly without touching the store.
    let out = run(&home, &["install", &url, "--ref", "missing-tag"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read_dir(home.join("store/gamma")).unwrap().count(), 1);
}

#[test]
fn dry_run_writes_nothing_and_invalid_skills_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let src = temp.path().join("src/alpha");
    skill(&src, Some("1.0.0"));
    let out = ok(&home, &["--dry-run", "install", src.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Would install alpha@1.0.0"));
    assert!(!home.join("store/alpha").exists());
    let bad = temp.path().join("bad/omega");
    skill(&bad, Some("1.0.0"));
    fs::write(
        bad.join("SKILL.md"),
        fs::read_to_string(bad.join("SKILL.md"))
            .unwrap()
            .replace("software change", "<software> change"),
    )
    .unwrap();
    let out = run(&home, &["install", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("O104"));
    assert!(!home.join("store/omega").exists());
    // No SKILL.md anywhere is a usage error.
    let empty = temp.path().join("empty");
    fs::create_dir(&empty).unwrap();
    assert_eq!(
        run(&home, &["install", empty.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
}
