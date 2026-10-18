#![cfg(unix)]

use skillctl::{
    linker::{LinkStatus, reconcile},
    paths::Paths,
    registry::{Instance, Project, Settings},
    services::fs::{DryRunFs, Fs, FsAction, RealFs},
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

struct Fixture {
    _temp: tempfile::TempDir,
    paths: Paths,
    instance: Instance,
    project: Project,
    settings: Settings,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let root = temp.path().join("instance");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&root).unwrap();
        let paths = Paths::resolve(Some(&home)).unwrap();
        let mut project = Project::default();
        project
            .skills
            .insert("demo".parse().unwrap(), "1.4.0".parse().unwrap());
        Self {
            _temp: temp,
            paths,
            instance: Instance {
                path: root,
                attached_at: chrono::DateTime::parse_from_rfc3339("2026-10-18T09:12:00Z")
                    .unwrap()
                    .into(),
                created_dirs: vec![],
            },
            project,
            settings: Settings::default(),
        }
    }

    fn run(&mut self, fs: &dyn Fs) -> skillctl::linker::ReconcileReport {
        reconcile(
            fs,
            &self.paths,
            &mut self.instance,
            &self.project,
            &self.settings,
        )
        .unwrap()
    }

    fn link(&self, target: &str) -> PathBuf {
        self.instance.path.join(target).join("demo")
    }
    fn desired(&self, version: &str) -> PathBuf {
        self.paths
            .store_dir(&"demo".parse().unwrap(), &version.parse().unwrap())
    }
}

#[test]
fn fresh_instance_creates_and_records_each_directory_once_and_absolute_links() {
    let mut f = Fixture::new();
    let report = f.run(&RealFs);
    assert_eq!(report.links.len(), 2);
    assert!(
        report
            .links
            .iter()
            .all(|link| link.status == LinkStatus::Linked)
    );
    for target in [".agents/skills", ".claude/skills"] {
        assert_eq!(fs::read_link(f.link(target)).unwrap(), f.desired("1.4.0"));
    }
    let dirs: Vec<_> = f
        .instance
        .created_dirs
        .iter()
        .map(|path| path.to_string())
        .collect();
    assert_eq!(
        dirs,
        [".agents", ".agents/skills", ".claude", ".claude/skills"]
    );
    let dry = DryRunFs::new();
    assert!(
        f.run(&dry)
            .links
            .iter()
            .all(|link| link.status == LinkStatus::Ok)
    );
    assert!(dry.actions().is_empty());
    assert_eq!(f.instance.created_dirs.len(), 4);
}

#[test]
fn bump_atomically_replaces_links_and_dangling_links_are_managed() {
    let mut f = Fixture::new();
    f.run(&RealFs); // Targets deliberately do not exist.
    f.project
        .skills
        .insert("demo".parse().unwrap(), "1.5.0".parse().unwrap());
    let dry = DryRunFs::new();
    assert!(
        f.run(&dry)
            .links
            .iter()
            .all(|link| link.status == LinkStatus::Updated)
    );
    let actions = dry.actions();
    assert_eq!(actions.len(), 4);
    for pair in actions.chunks_exact(2) {
        let FsAction::Symlink { target, link } = &pair[0] else {
            panic!("expected temporary symlink")
        };
        assert_eq!(target, &f.desired("1.5.0"));
        let FsAction::Rename { from, to } = &pair[1] else {
            panic!("expected atomic rename")
        };
        assert_eq!(link, from);
        assert_eq!(from.parent(), to.parent());
    }
    assert!(
        f.run(&RealFs)
            .links
            .iter()
            .all(|link| link.status == LinkStatus::Updated)
    );
    for target in [".agents/skills", ".claude/skills"] {
        assert_eq!(fs::read_link(f.link(target)).unwrap(), f.desired("1.5.0"));
        assert_eq!(
            fs::read_dir(f.link(target).parent().unwrap())
                .unwrap()
                .count(),
            1
        );
    }
}

#[test]
fn unpinned_managed_links_are_removed_but_own_files_and_foreign_links_remain() {
    let mut f = Fixture::new();
    f.run(&RealFs);
    let dir = f.instance.path.join(".agents/skills");
    fs::write(dir.join("notes"), "keep").unwrap();
    fs::create_dir(dir.join("personal")).unwrap();
    std::os::unix::fs::symlink("/somewhere/else", dir.join("foreign")).unwrap();
    f.project.skills.clear();
    let report = f.run(&RealFs);
    assert_eq!(report.links.len(), 2);
    assert!(
        report
            .links
            .iter()
            .all(|link| link.status == LinkStatus::Removed)
    );
    assert_eq!(fs::read(dir.join("notes")).unwrap(), b"keep");
    assert!(dir.join("personal").is_dir());
    assert_eq!(
        fs::read_link(dir.join("foreign")).unwrap(),
        Path::new("/somewhere/else")
    );
}

#[test]
fn own_folder_file_and_foreign_link_conflict_without_blocking_other_skills() {
    for kind in ["folder", "file", "link"] {
        let mut f = Fixture::new();
        let path = f.link(".agents/skills");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        match kind {
            "folder" => fs::create_dir(&path).unwrap(),
            "file" => fs::write(&path, "keep").unwrap(),
            _ => std::os::unix::fs::symlink("/foreign", &path).unwrap(),
        }
        let report = f.run(&RealFs);
        assert!(report.has_conflicts());
        assert_eq!(report.links[0].status, LinkStatus::Conflict);
        assert_eq!(report.links[1].status, LinkStatus::Linked);
        match kind {
            "folder" => assert!(path.is_dir()),
            "file" => assert_eq!(fs::read(&path).unwrap(), b"keep"),
            _ => assert_eq!(fs::read_link(&path).unwrap(), Path::new("/foreign")),
        }
    }
}

#[test]
fn obsolete_targets_remove_only_managed_links_and_do_not_remove_active_overlaps() {
    let mut f = Fixture::new();
    f.run(&RealFs);
    let dir = f.instance.path.join(".agents/skills");
    fs::write(dir.join("own"), "keep").unwrap();
    f.settings.previous_link_targets = f.settings.link_targets.clone();
    f.settings.link_targets = vec![".claude/skills".parse().unwrap()];
    let report = f.run(&RealFs);
    assert_eq!(
        report
            .links
            .iter()
            .filter(|l| l.status == LinkStatus::Removed)
            .count(),
        1
    );
    assert!(fs::symlink_metadata(dir.join("demo")).is_err());
    assert_eq!(fs::read(dir.join("own")).unwrap(), b"keep");
    assert_eq!(
        fs::read_link(f.link(".claude/skills")).unwrap(),
        f.desired("1.4.0")
    );
    assert_eq!(f.settings.previous_link_targets.len(), 2); // Cleared by relink --all, not here.
}

#[test]
fn draft_links_can_be_created_updated_and_removed() {
    let mut f = Fixture::new();
    f.project
        .skills
        .insert("demo".parse().unwrap(), "draft".parse().unwrap());
    f.run(&RealFs);
    assert_eq!(
        fs::read_link(f.link(".agents/skills")).unwrap(),
        f.paths.draft_dir(&"demo".parse().unwrap())
    );
    f.project
        .skills
        .insert("demo".parse().unwrap(), "1.4.0".parse().unwrap());
    assert!(
        f.run(&RealFs)
            .links
            .iter()
            .all(|l| l.status == LinkStatus::Updated)
    );
    f.project
        .skills
        .insert("demo".parse().unwrap(), "draft".parse().unwrap());
    f.run(&RealFs);
    f.project.skills.clear();
    assert!(
        f.run(&RealFs)
            .links
            .iter()
            .all(|l| l.status == LinkStatus::Removed)
    );
}

#[test]
fn dry_run_plans_fresh_shared_parents_once_and_does_not_write() {
    let mut f = Fixture::new();
    f.project
        .skills
        .insert("other".parse().unwrap(), "draft".parse().unwrap());
    f.settings
        .link_targets
        .push(".agents/extra".parse().unwrap());
    let dry = DryRunFs::new();
    let report = f.run(&dry);
    assert_eq!(report.links.len(), 6);
    assert_eq!(
        dry.actions()
            .iter()
            .filter(|a| matches!(a, FsAction::CreateDir { .. }))
            .count(),
        5
    );
    assert_eq!(fs::read_dir(&f.instance.path).unwrap().count(), 0);
    assert_eq!(f.instance.created_dirs.len(), 5);
}

#[test]
fn preexisting_directories_are_not_recorded_and_duplicate_targets_are_deduplicated() {
    let mut f = Fixture::new();
    fs::create_dir(f.instance.path.join(".agents")).unwrap();
    f.settings
        .link_targets
        .push(".agents/skills".parse().unwrap());
    let report = f.run(&RealFs);
    assert_eq!(report.links.len(), 2);
    assert!(
        !f.instance
            .created_dirs
            .contains(&".agents".parse().unwrap())
    );
    assert_eq!(f.instance.created_dirs.len(), 3);
}

#[test]
fn target_symlink_or_file_is_not_followed_or_overwritten() {
    for symlink in [false, true] {
        let mut f = Fixture::new();
        let outside = f._temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let obstacle = f.instance.path.join(".agents");
        if symlink {
            std::os::unix::fs::symlink(&outside, &obstacle).unwrap();
        } else {
            fs::write(&obstacle, "keep").unwrap();
        }
        let report = f.run(&RealFs);
        assert_eq!(report.links[0].status, LinkStatus::Conflict);
        assert_eq!(report.links[1].status, LinkStatus::Linked);
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }
}

#[test]
fn relative_owned_links_are_recognized_and_escaping_prefixes_are_foreign() {
    let mut f = Fixture::new();
    f.settings.link_targets = vec!["skills".parse().unwrap()];
    let dir = f.instance.path.join("skills");
    fs::create_dir(&dir).unwrap();
    std::os::unix::fs::symlink("../../home/store/demo/1.4.0/demo", dir.join("demo")).unwrap();
    assert_eq!(f.run(&RealFs).links[0].status, LinkStatus::Ok);
    fs::remove_file(dir.join("demo")).unwrap();
    let outside = f.paths.store().join("../../outside");
    std::os::unix::fs::symlink(&outside, dir.join("demo")).unwrap();
    assert_eq!(f.run(&RealFs).links[0].status, LinkStatus::Conflict);
    f.project.skills.clear();
    assert!(f.run(&RealFs).links.is_empty());
    assert_eq!(fs::read_link(dir.join("demo")).unwrap(), outside);
}

struct RenameFailure;
impl Fs for RenameFailure {
    fn symlink_metadata(&self, p: &Path) -> io::Result<fs::Metadata> {
        RealFs.symlink_metadata(p)
    }
    fn read_link(&self, p: &Path) -> io::Result<PathBuf> {
        RealFs.read_link(p)
    }
    fn read_dir(&self, p: &Path) -> io::Result<Vec<PathBuf>> {
        RealFs.read_dir(p)
    }
    fn create_dir(&self, p: &Path) -> io::Result<()> {
        RealFs.create_dir(p)
    }
    fn symlink(&self, t: &Path, p: &Path) -> io::Result<()> {
        RealFs.symlink(t, p)
    }
    fn rename(&self, _: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::other("injected rename failure"))
    }
    fn remove_file(&self, p: &Path) -> io::Result<()> {
        RealFs.remove_file(p)
    }
    fn remove_dir(&self, p: &Path) -> io::Result<()> {
        RealFs.remove_dir(p)
    }
    fn write_atomic(&self, p: &Path, bytes: &[u8]) -> io::Result<()> {
        RealFs.write_atomic(p, bytes)
    }
    fn set_mode(&self, p: &Path, mode: u32) -> io::Result<()> {
        RealFs.set_mode(p, mode)
    }
}

#[test]
fn failed_atomic_update_preserves_old_link_and_cleans_temporary_link() {
    let mut f = Fixture::new();
    f.run(&RealFs);
    f.project
        .skills
        .insert("demo".parse().unwrap(), "1.5.0".parse().unwrap());
    let error = reconcile(
        &RenameFailure,
        &f.paths,
        &mut f.instance,
        &f.project,
        &f.settings,
    )
    .unwrap_err();
    assert!(error.to_string().contains("injected rename failure"));
    assert_eq!(
        fs::read_link(f.link(".agents/skills")).unwrap(),
        f.desired("1.4.0")
    );
    assert_eq!(
        fs::read_dir(f.instance.path.join(".agents/skills"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn non_utf8_unpinned_managed_links_are_removed() {
    use std::os::unix::ffi::OsStrExt;
    let mut f = Fixture::new();
    f.run(&RealFs);
    let path = f
        .instance
        .path
        .join(".agents/skills")
        .join(std::ffi::OsStr::from_bytes(b"\xff"));
    // APFS refuses non-UTF-8 names (EILSEQ); only Linux filesystems can exercise this.
    if let Err(error) = std::os::unix::fs::symlink(f.desired("1.4.0"), &path) {
        eprintln!("skipping: filesystem rejects non-UTF-8 names: {error}");
        return;
    }
    assert!(
        f.run(&RealFs)
            .links
            .iter()
            .any(|l| l.path == path && l.status == LinkStatus::Removed)
    );
    assert!(fs::symlink_metadata(path).is_err());
}
