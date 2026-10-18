#![cfg(unix)]

use skillctl::{
    error::Error,
    paths::Paths,
    services::fs::{DryRunFs, Fs, FsAction, RealFs},
    store::{
        hash::content_hash,
        meta::Metadata,
        stage::{restore_write_mode, stage_version},
    },
};
use std::{
    cell::Cell,
    fs, io,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

struct Fixture {
    _temp: tempfile::TempDir,
    paths: Paths,
    source: PathBuf,
    metadata: Metadata,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(home.join("store")).unwrap();
        let paths = Paths::resolve(Some(&home)).unwrap();
        let source = temp.path().join("draft");
        fs::create_dir_all(source.join("scripts/empty")).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\nmetadata:\n  version: 1.0.0\n---\nBody\n",
        )
        .unwrap();
        fs::write(source.join("scripts/run"), "#!/bin/sh\ntrue\n").unwrap();
        fs::set_permissions(
            source.join("scripts/run"),
            fs::Permissions::from_mode(0o710),
        )
        .unwrap();
        let metadata = Metadata::from_json(
            &serde_json::to_vec(&serde_json::json!({
                "name": "demo", "version": "1.0.0", "source": {"kind": "local"},
                "content_hash": content_hash(&RealFs, &source).unwrap(),
                "installed_at": "2026-10-18T09:05:00Z", "skillctl_version": "0.1.0"
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            _temp: temp,
            paths,
            source,
            metadata,
        }
    }

    fn destination(&self) -> PathBuf {
        self.paths
            .version_dir(&self.metadata.name, &self.metadata.version)
    }

    fn assert_clean(&self) {
        assert!(!self.destination().exists());
        let parent = self.paths.skill_versions(&self.metadata.name);
        if parent.exists() {
            assert_eq!(fs::read_dir(parent).unwrap().count(), 0);
        }
        assert!(self.source.join("SKILL.md").exists());
    }
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
}

#[test]
fn publishes_complete_read_only_version_without_changing_source() {
    let fixture = Fixture::new();
    let original_mode = mode(&fixture.source.join("scripts/run"));
    let version =
        stage_version(&RealFs, &fixture.paths, &fixture.source, &fixture.metadata).unwrap();
    assert_eq!(version, fixture.destination());
    let skill = version.join("demo");
    for path in [
        &version,
        &skill,
        &skill.join("scripts"),
        &skill.join("scripts/empty"),
    ] {
        assert_eq!(mode(path), 0o555);
    }
    assert_eq!(mode(&skill.join("SKILL.md")), 0o444);
    assert_eq!(mode(&version.join("meta.json")), 0o444);
    assert_eq!(mode(&skill.join("scripts/run")), 0o555);
    assert_eq!(
        Metadata::load(&RealFs, &version.join("meta.json")).unwrap(),
        fixture.metadata
    );
    assert_eq!(
        content_hash(&RealFs, &skill).unwrap(),
        fixture.metadata.content_hash
    );
    assert_eq!(mode(&fixture.source.join("scripts/run")), original_mode);
    assert_eq!(
        fs::read_dir(fixture.paths.skill_versions(&fixture.metadata.name))
            .unwrap()
            .count(),
        1
    );
    restore_write_mode(&RealFs, &version).unwrap();
    for path in [
        &version,
        &skill,
        &skill.join("scripts/empty"),
        &skill.join("SKILL.md"),
        &version.join("meta.json"),
    ] {
        assert_eq!(mode(path), 0o755);
    }
    fs::remove_dir_all(version).unwrap();
}

#[test]
fn wrong_hash_rolls_back_without_publishing() {
    let mut fixture = Fixture::new();
    fixture.metadata.content_hash = "sha256:wrong".into();
    assert!(matches!(
        stage_version(&RealFs, &fixture.paths, &fixture.source, &fixture.metadata),
        Err(Error::Validation { .. })
    ));
    fixture.assert_clean();
}

#[test]
fn existing_versions_are_never_replaced_even_when_empty_or_symlinks() {
    for kind in ["directory", "file", "symlink"] {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.destination().parent().unwrap()).unwrap();
        match kind {
            "directory" => fs::create_dir(fixture.destination()).unwrap(),
            "file" => fs::write(fixture.destination(), "untouched").unwrap(),
            _ => symlink(&fixture.source, fixture.destination()).unwrap(),
        }
        let error =
            stage_version(&RealFs, &fixture.paths, &fixture.source, &fixture.metadata).unwrap_err();
        assert!(
            matches!(error, Error::Io { source, .. } if source.kind() == io::ErrorKind::AlreadyExists)
        );
        assert_eq!(
            fs::read_dir(fixture.destination().parent().unwrap())
                .unwrap()
                .count(),
            1
        );
        if kind == "file" {
            assert_eq!(
                fs::read_to_string(fixture.destination()).unwrap(),
                "untouched"
            );
        }
        if kind == "symlink" {
            assert_eq!(
                fs::read_link(fixture.destination()).unwrap(),
                fixture.source
            );
        }
    }
}

#[test]
fn refuses_source_symlinks_and_special_files_before_mutations() {
    use std::os::unix::net::UnixListener;
    for kind in [
        "file link",
        "directory link",
        "dangling link",
        "socket",
        "root link",
    ] {
        let fixture = Fixture::new();
        let entry = fixture.source.join("bad");
        let mut source = fixture.source.clone();
        let _socket = match kind {
            "socket" => Some(UnixListener::bind(&entry).unwrap()),
            "root link" => {
                source = fixture.source.with_file_name("source-link");
                symlink(&fixture.source, &source).unwrap();
                None
            }
            _ => {
                let target = match kind {
                    "file link" => fixture.source.join("SKILL.md"),
                    "directory link" => fixture.source.join("scripts"),
                    _ => fixture.source.join("missing"),
                };
                symlink(target, entry).unwrap();
                None
            }
        };
        let dry = DryRunFs::new();
        assert!(
            stage_version(&dry, &fixture.paths, &source, &fixture.metadata).is_err(),
            "{kind}"
        );
        assert!(dry.actions().is_empty());
        fixture.assert_clean();
    }
}

#[test]
fn refuses_symlinked_store_and_skill_parent() {
    for parent in [false, true] {
        let fixture = Fixture::new();
        let external = fixture.source.with_file_name("external");
        fs::create_dir(&external).unwrap();
        let link = if parent {
            fixture.paths.skill_versions(&fixture.metadata.name)
        } else {
            fs::remove_dir(fixture.paths.store()).unwrap();
            fixture.paths.store()
        };
        symlink(&external, link).unwrap();
        assert!(
            stage_version(&RealFs, &fixture.paths, &fixture.source, &fixture.metadata).is_err()
        );
        assert_eq!(fs::read_dir(external).unwrap().count(), 0);
    }
}

#[test]
fn dry_run_records_full_plan_without_creating_staged_paths() {
    let fixture = Fixture::new();
    let dry = DryRunFs::new();
    let destination =
        stage_version(&dry, &fixture.paths, &fixture.source, &fixture.metadata).unwrap();
    assert_eq!(destination, fixture.destination());
    assert!(
        !fixture
            .paths
            .skill_versions(&fixture.metadata.name)
            .exists()
    );
    let actions = dry.actions();
    assert!(
        matches!(actions.last(), Some(FsAction::SetMode { path, mode: 0o555 }) if path == &destination)
    );
    assert!(
        matches!(actions.get(actions.len() - 2), Some(FsAction::Rename { to, .. }) if to == &destination)
    );
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a, FsAction::WriteAtomic { .. }))
            .count(),
        3
    );
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a, FsAction::SetMode { mode: 0o555, .. }))
            .count(),
        5
    );
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a, FsAction::SetMode { mode: 0o444, .. }))
            .count(),
        2
    );
    assert!(actions.iter().any(|a| matches!(a, FsAction::CreateDir { path } if path.file_name().unwrap().to_string_lossy().starts_with(".staging-"))));
}

#[test]
fn permission_restoration_preflights_symlinks_and_dry_runs() {
    let fixture = Fixture::new();
    let version =
        stage_version(&RealFs, &fixture.paths, &fixture.source, &fixture.metadata).unwrap();
    let dry = DryRunFs::new();
    restore_write_mode(&dry, &version).unwrap();
    assert!(
        dry.actions()
            .iter()
            .all(|a| matches!(a, FsAction::SetMode { mode: 0o755, .. }))
    );
    assert_eq!(mode(&version), 0o555);
    RealFs.set_mode(&version, 0o755).unwrap();
    symlink(&fixture.source, version.join("foreign")).unwrap();
    let dry = DryRunFs::new();
    assert!(restore_write_mode(&dry, &version).is_err());
    assert!(dry.actions().is_empty());
    assert!(restore_write_mode(&RealFs, &version).is_err());
    assert_eq!(mode(&version.join("meta.json")), 0o444);
    fs::remove_file(version.join("foreign")).unwrap();
    restore_write_mode(&RealFs, &version).unwrap();
}

/// Fail once at a chosen mutation, then allow rollback to use the real FS.
struct FailFs {
    point: &'static str,
    failed: Cell<bool>,
}

impl FailFs {
    fn fail(&self, point: &str) -> io::Result<()> {
        if self.point == point && !self.failed.replace(true) {
            Err(io::Error::other(format!("injected {point} failure")))
        } else {
            Ok(())
        }
    }
}

impl Fs for FailFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        RealFs.read(path)
    }
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        RealFs.symlink_metadata(path)
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        RealFs.read_link(path)
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        RealFs.read_dir(path)
    }
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        if path.file_name().is_some_and(|n| n == "scripts") {
            self.fail("directory")?;
        }
        RealFs.create_dir(path)
    }
    fn symlink(&self, target: &Path, link: &Path) -> io::Result<()> {
        RealFs.symlink(target, link)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        // Model macOS even on Linux: read-only directory roots cannot be renamed.
        if mode(from) & 0o200 == 0 {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        self.fail("rename")?;
        RealFs.rename(from, to)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        RealFs.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        RealFs.remove_dir(path)
    }
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.fail(if path.file_name().unwrap() == "meta.json" {
            "metadata"
        } else {
            "copy"
        })?;
        RealFs.write_atomic(path, bytes)?;
        self.fail("after-write")
    }
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        if mode == 0o555 && path.file_name().is_some_and(|n| n == "demo") {
            self.fail("permissions")?;
        }
        if mode == 0o555 && path.file_name().is_some_and(|n| n == "1.0.0") {
            self.fail("published-permissions")?;
        }
        RealFs.set_mode(path, mode)
    }
}

#[test]
fn publishes_with_macos_rename_permissions() {
    let fixture = Fixture::new();
    let fs = FailFs {
        point: "none",
        failed: Cell::new(false),
    };
    let version = stage_version(&fs, &fixture.paths, &fixture.source, &fixture.metadata).unwrap();
    assert_eq!(version, fixture.destination());
    assert_eq!(mode(&version), 0o555);
    restore_write_mode(&RealFs, &version).unwrap();
}

#[test]
fn failures_at_each_stage_roll_back_even_after_sealing_or_committed_write() {
    for point in [
        "directory",
        "copy",
        "metadata",
        "after-write",
        "permissions",
        "rename",
        "published-permissions",
    ] {
        let fixture = Fixture::new();
        let failing = FailFs {
            point,
            failed: Cell::new(false),
        };
        let error = stage_version(&failing, &fixture.paths, &fixture.source, &fixture.metadata)
            .unwrap_err();
        assert!(error.to_string().contains("injected"), "{point}: {error}");
        assert!(failing.failed.get(), "{point}");
        fixture.assert_clean();
    }
}
