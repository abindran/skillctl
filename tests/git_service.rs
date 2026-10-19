use std::path::Path;
use std::process::Command;

use skillctl::cli::Cli;
use skillctl::context::Context;
use skillctl::error::Error;
use skillctl::services::git::{Git, GitCli};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "--quiet"]);
}

#[test]
fn normal_repository_and_subfolder_return_absolute_paths_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo with spaces ");
    repo(&root);
    let sub = root.join("nested/subfolder");
    std::fs::create_dir_all(&sub).unwrap();
    let root = root.canonicalize().unwrap();
    let service = GitCli::new();
    for dir in [&root, &sub] {
        assert_eq!(service.toplevel(dir).unwrap(), Some(root.clone()));
        assert_eq!(
            service.exclude_file(dir).unwrap(),
            Some(root.join(".git/info/exclude"))
        );
    }
    std::fs::remove_file(root.join(".git/info/exclude")).unwrap();
    assert_eq!(
        service.exclude_file(&sub).unwrap(),
        Some(root.join(".git/info/exclude"))
    );
    assert!(!root.join(".git/info/exclude").exists());
}

#[test]
fn linked_worktree_uses_shared_exclude_file_not_dot_git_directory() {
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("main");
    repo(&main);
    git(
        &main,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    let worktree = temp.path().join("linked worktree");
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            worktree.to_str().unwrap(),
        ],
    );
    assert!(worktree.join(".git").is_file());
    let service = GitCli::new();
    assert_eq!(
        service.toplevel(&worktree).unwrap(),
        Some(worktree.canonicalize().unwrap())
    );
    assert_eq!(
        service.exclude_file(&worktree).unwrap(),
        service.exclude_file(&main).unwrap()
    );
}

#[test]
fn non_repository_and_bare_repository_return_none() {
    let temp = tempfile::tempdir().unwrap();
    let service = GitCli::new();
    assert_eq!(service.toplevel(temp.path()).unwrap(), None);
    assert_eq!(service.exclude_file(temp.path()).unwrap(), None);
    let bare = temp.path().join("bare.git");
    std::fs::create_dir(&bare).unwrap();
    git(&bare, &["init", "--bare", "--quiet"]);
    assert_eq!(service.toplevel(&bare).unwrap(), None);
    assert_eq!(service.exclude_file(&bare).unwrap(), None);
}

#[test]
fn invalid_directory_and_missing_executable_are_actionable_errors() {
    let temp = tempfile::tempdir().unwrap();
    let error = GitCli::new()
        .toplevel(&temp.path().join("missing"))
        .unwrap_err();
    assert_eq!(error.exit_code(), 1);
    assert!(error.to_string().contains("missing"));
    let service = GitCli::with_program(temp.path().join("no-such-git"));
    for error in [
        service.version().unwrap_err(),
        service.toplevel(temp.path()).unwrap_err(),
        service.exclude_file(temp.path()).unwrap_err(),
    ] {
        assert!(
            matches!(&error, Error::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
        );
        assert!(error.hint().contains("Install Git"));
    }
}

#[test]
fn installed_git_version_is_parsed() {
    let version = GitCli::new().version().unwrap();
    assert!(version >= semver::Version::new(2, 31, 0));
}

struct FakeGit;

impl Git for FakeGit {
    fn toplevel(&self, _dir: &Path) -> Result<Option<std::path::PathBuf>, Error> {
        Ok(Some(std::path::PathBuf::from("/injected/repository")))
    }
    fn exclude_file(&self, _dir: &Path) -> Result<Option<std::path::PathBuf>, Error> {
        Ok(None)
    }
    fn version(&self) -> Result<semver::Version, Error> {
        Ok(semver::Version::new(2, 31, 0))
    }
}

#[test]
fn context_allows_fake_git_and_build_does_not_run_git_or_create_home() {
    use clap::Parser;
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("missing-home");
    let cli = Cli::try_parse_from([
        "skillctl",
        "--home",
        home.to_str().unwrap(),
        "--dry-run",
        "status",
    ])
    .unwrap();
    let mut context = Context::from_cli(&cli).unwrap();
    assert!(!home.exists());
    context.git = Box::new(FakeGit);
    assert_eq!(
        context.git.toplevel(temp.path()).unwrap(),
        Some("/injected/repository".into())
    );
    assert_eq!(
        context.git.version().unwrap(),
        semver::Version::new(2, 31, 0)
    );
    assert!(!home.exists());
}

#[cfg(unix)]
#[test]
fn non_utf8_repository_path_is_preserved() {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp
        .path()
        .join(std::ffi::OsString::from_vec(b"repo-\xff".to_vec()));
    // APFS refuses non-UTF-8 names (EILSEQ); only Linux filesystems can exercise this.
    if let Err(error) = std::fs::create_dir_all(&root) {
        eprintln!("skipping: filesystem rejects non-UTF-8 names: {error}");
        return;
    }
    repo(&root);
    assert_eq!(
        GitCli::new().toplevel(&root).unwrap(),
        Some(root.canonicalize().unwrap())
    );
}

#[cfg(unix)]
#[test]
fn unexpected_command_failures_are_not_non_repository_results() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("failing-git");
    std::fs::write(
        &executable,
        "#!/bin/sh\necho 'fatal: detected dubious ownership' >&2\nexit 128\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    // On Linux a sibling test thread may fork while the script is still open
    // for writing, and the child holds that descriptor until it execs; running
    // the script in that window fails with ETXTBSY. Retry briefly rather than
    // fail on a race that is unrelated to what this test checks.
    let git = GitCli::with_program(executable);
    let mut error = git.toplevel(temp.path()).unwrap_err();
    for _ in 0..50 {
        let busy = matches!(
            &error,
            skillctl::error::Error::Io { source, .. }
                if source.kind() == std::io::ErrorKind::ExecutableFileBusy
        );
        if !busy {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        error = git.toplevel(temp.path()).unwrap_err();
    }
    assert!(error.to_string().contains("dubious ownership"), "{error:?}");
    assert_eq!(error.exit_code(), 1);
}
