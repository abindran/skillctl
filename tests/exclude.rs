use skillctl::{
    linker::exclude::{rewrite, update},
    services::{
        fs::{DryRunFs, RealFs},
        git::{Git, GitCli},
    },
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn patterns() -> BTreeSet<String> {
    ["/.agents/skills/demo".into(), "/.claude/skills/demo".into()].into()
}
#[test]
fn insert_replace_remove_preserves_other_bytes_and_instances() {
    let a = Path::new("/work/a");
    let b = Path::new("/work/b");
    let initial = b"# user\n\\*.keep\n\xff\n";
    let first = rewrite(initial, a, &patterns()).unwrap();
    assert!(first.starts_with(initial));
    let second = rewrite(&first, b, &patterns()).unwrap();
    assert_eq!(rewrite(&second, a, &patterns()).unwrap(), second);
    let removed = rewrite(&second, a, &BTreeSet::new()).unwrap();
    assert_eq!(removed, rewrite(initial, b, &patterns()).unwrap());
    assert_eq!(rewrite(&removed, b, &BTreeSet::new()).unwrap(), initial);
}
#[test]
fn crlf_and_missing_final_newline() {
    let root = Path::new("/work/a");
    let bytes = rewrite(b"# user\r\nlast", root, &patterns()).unwrap();
    assert!(bytes.starts_with(b"# user\r\nlast\r\n# >>>"));
    assert_eq!(rewrite(&bytes, root, &patterns()).unwrap(), bytes);
}
#[test]
fn malformed_markers_and_injected_patterns_fail_closed() {
    let root = Path::new("/a");
    for bytes in [
        "# >>> skillctl /a >>>\n",
        "# <<< skillctl /a <<<\n",
        "# >>> skillctl /a >>>\n# >>> skillctl /b >>>\n# <<< skillctl /a <<<\n",
        "# >>> skillctl /a >>>\n# <<< skillctl /a <<<\n# >>> skillctl /a >>>\n",
    ] {
        assert!(rewrite(bytes.as_bytes(), root, &patterns()).is_err());
    }
    assert!(rewrite(b"", Path::new("/a\nb"), &patterns()).is_err());
    assert!(rewrite(b"", root, &["/a\n*".into()].into()).is_err());
}
fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}
#[test]
fn real_repo_hides_only_links_and_is_idempotent_and_removable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    let instance = root.join("nested [x]");
    let dir = instance.join(".agents/skills");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("demo"), "simulated link").unwrap();
    fs::write(dir.join("own"), "visible").unwrap();
    let link = dir.join("demo");
    let file = update(&RealFs, &GitCli::new(), &instance, [link.clone()])
        .unwrap()
        .unwrap();
    let status = git(&root, &["status", "--porcelain", "--untracked-files=all"]);
    assert!(!status.contains("demo"));
    assert!(status.contains("own"));
    let dry = DryRunFs::new();
    update(&dry, &GitCli::new(), &instance, [link]).unwrap();
    assert!(dry.actions().is_empty());
    update(&RealFs, &GitCli::new(), &instance, []).unwrap();
    assert!(!fs::read_to_string(file).unwrap().contains("skillctl"));
    assert!(git(&root, &["status", "--porcelain", "--untracked-files=all"]).contains("demo"));
}
#[test]
fn dry_run_and_non_repo_do_not_write() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let dry = DryRunFs::new();
    assert_eq!(update(&dry, &GitCli::new(), root, []).unwrap(), None);
    assert!(dry.actions().is_empty());
    git(root, &["init", "-q"]);
    let file = GitCli::new().exclude_file(root).unwrap().unwrap();
    let before = fs::read(&file).unwrap();
    update(&dry, &GitCli::new(), root, [root.join("skills/demo")]).unwrap();
    assert_eq!(dry.actions().len(), 1);
    assert_eq!(fs::read(file).unwrap(), before);
    assert!(
        update(
            &RealFs,
            &GitCli::new(),
            root,
            [PathBuf::from("/outside/demo")]
        )
        .is_err()
    );
}
#[test]
fn linked_worktrees_keep_each_others_blocks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "initial",
        ],
    );
    let worktree = temp.path().join("tree");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-qb",
            "other",
            worktree.to_str().unwrap(),
        ],
    );
    let root = root.canonicalize().unwrap();
    let worktree = worktree.canonicalize().unwrap();
    let file = update(&RealFs, &GitCli::new(), &root, [root.join("skills/a")])
        .unwrap()
        .unwrap();
    update(
        &RealFs,
        &GitCli::new(),
        &worktree,
        [worktree.join("skills/b")],
    )
    .unwrap();
    let both = fs::read_to_string(&file).unwrap();
    assert!(both.contains("/skills/a"));
    assert!(both.contains("/skills/b"));
    update(&RealFs, &GitCli::new(), &root, []).unwrap();
    let remaining = fs::read_to_string(file).unwrap();
    assert!(!remaining.contains("/skills/a"));
    assert!(remaining.contains("/skills/b"));
}
