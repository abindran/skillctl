//! Filesystem operations and a read-through, write-recording dry run.

use std::cell::RefCell;
use std::fs::{self, Metadata};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Object-safe filesystem boundary. Mutations never create missing parents implicitly.
pub trait Fs {
    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    fn symlink(&self, target: &Path, link: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()>;
}

/// Performs operations on the host filesystem.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFs;

impl Fs for RealFs {
    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata> {
        fs::symlink_metadata(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        fs::read_link(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let mut entries = fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?;
        entries.sort();
        Ok(entries)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        fs::create_dir(path)
    }

    fn symlink(&self, target: &Path, link: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link)
        }
        #[cfg(not(unix))]
        {
            let _ = (target, link);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "symlinks require Unix",
            ))
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir(path)
    }

    /// Sync a same-directory temporary file before atomically publishing it.
    /// Existing regular-file permissions are preserved; new files are private.
    /// On Unix the parent directory is synced after rename for durability.
    /// A directory-sync error can therefore occur after the new file is visible.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        write_atomic_before_commit(path, bytes, || Ok(()))
    }

    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(mode))
        }
        #[cfg(not(unix))]
        {
            let _ = (path, mode);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Unix modes require Unix",
            ))
        }
    }
}

// The hook allows tests to interrupt precisely after sync, before publication.
fn write_atomic_before_commit(
    path: &Path,
    bytes: &[u8],
    before_commit: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => temporary
            .as_file()
            .set_permissions(metadata.permissions())?,
        Ok(_) => (),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    temporary.as_file().sync_all()?;
    before_commit()?;
    temporary.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

/// A planned mutation, retained in call order without touching disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsAction {
    CreateDir { path: PathBuf },
    Symlink { target: PathBuf, link: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
    RemoveFile { path: PathBuf },
    RemoveDir { path: PathBuf },
    WriteAtomic { path: PathBuf, bytes: Vec<u8> },
    SetMode { path: PathBuf, mode: u32 },
}

/// Reads observe the real disk, NOT an overlay of planned mutations.
/// Writes are recorded without validation, so plans can refer to paths not yet created.
/// Command-level rendering of these actions is added with Context/dry-run wiring.
#[derive(Debug, Default)]
pub struct DryRunFs {
    actions: RefCell<Vec<FsAction>>,
}

impl DryRunFs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the plan without consuming it.
    pub fn actions(&self) -> Vec<FsAction> {
        self.actions.borrow().clone()
    }

    /// Drain the plan, leaving this service ready to record more actions.
    pub fn take_actions(&self) -> Vec<FsAction> {
        std::mem::take(&mut *self.actions.borrow_mut())
    }

    fn record(&self, action: FsAction) -> io::Result<()> {
        self.actions.borrow_mut().push(action);
        Ok(())
    }
}

impl Fs for DryRunFs {
    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata> {
        RealFs.symlink_metadata(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        RealFs.read_link(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        RealFs.read_dir(path)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        self.record(FsAction::CreateDir { path: path.into() })
    }

    fn symlink(&self, target: &Path, link: &Path) -> io::Result<()> {
        self.record(FsAction::Symlink {
            target: target.into(),
            link: link.into(),
        })
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.record(FsAction::Rename {
            from: from.into(),
            to: to.into(),
        })
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.record(FsAction::RemoveFile { path: path.into() })
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        self.record(FsAction::RemoveDir { path: path.into() })
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.record(FsAction::WriteAtomic {
            path: path.into(),
            bytes: bytes.into(),
        })
    }

    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        self.record(FsAction::SetMode {
            path: path.into(),
            mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_operations_through_trait_object() {
        let sandbox = tempfile::tempdir().unwrap();
        let service: &dyn Fs = &RealFs;
        let directory = sandbox.path().join("directory");
        service.create_dir(&directory).unwrap();
        assert_eq!(
            service.create_dir(&directory).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        let first = directory.join("first");
        let second = directory.join("second");
        service.write_atomic(&first, b"old").unwrap();
        service.write_atomic(&first, b"new").unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"new");
        service.rename(&first, &second).unwrap();
        assert_eq!(service.read_dir(&directory).unwrap(), vec![second.clone()]);
        assert!(service.remove_dir(&directory).is_err());
        service.remove_file(&second).unwrap();
        service.remove_dir(&directory).unwrap();
        assert!(!directory.exists());
    }

    #[test]
    fn failed_atomic_write_preserves_original_and_cleans_temporary() {
        let sandbox = tempfile::tempdir().unwrap();
        let path = sandbox.path().join("registry.yml");
        RealFs.write_atomic(&path, b"original").unwrap();
        let result = write_atomic_before_commit(&path, b"replacement", || {
            Err(io::Error::other("interrupted"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(RealFs.read_dir(sandbox.path()).unwrap(), vec![path]);
        let directory = sandbox.path().join("directory");
        fs::create_dir(&directory).unwrap();
        assert!(
            RealFs
                .write_atomic(&directory, b"cannot replace a directory")
                .is_err()
        );
        assert_eq!(RealFs.read_dir(sandbox.path()).unwrap().len(), 2);
        assert!(
            RealFs
                .write_atomic(&sandbox.path().join("missing/file"), b"data")
                .is_err()
        );
    }

    #[test]
    fn abrupt_process_exit_before_rename_keeps_original_intact() {
        let sandbox = tempfile::tempdir().unwrap();
        let path = sandbox.path().join("registry.yml");
        fs::write(&path, b"original").unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "services::fs::tests::crash_worker", "--ignored"])
            .env("SKILLCTL_TEST_CRASH_PATH", &path)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        // An abrupt exit may leave staging debris, but never partial destination bytes.
        let staged = RealFs
            .read_dir(sandbox.path())
            .unwrap()
            .into_iter()
            .find(|entry| entry != &path)
            .unwrap();
        assert_eq!(fs::read(staged).unwrap(), b"replacement");
        RealFs.write_atomic(&path, b"recovered").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"recovered");
    }

    #[test]
    #[ignore = "subprocess helper for crash simulation"]
    fn crash_worker() {
        let path =
            PathBuf::from(std::env::var_os("SKILLCTL_TEST_CRASH_PATH").expect("crash test path"));
        write_atomic_before_commit(&path, b"replacement", || std::process::exit(73)).unwrap();
    }

    #[test]
    fn dry_run_records_every_mutation_without_changing_disk() {
        let sandbox = tempfile::tempdir().unwrap();
        let file = sandbox.path().join("file");
        let directory = sandbox.path().join("directory");
        let link = sandbox.path().join("link");
        fs::write(&file, b"original").unwrap();
        fs::create_dir(&directory).unwrap();
        let before = fs::metadata(&file).unwrap().permissions();
        let dry = DryRunFs::new();
        let service: &dyn Fs = &dry;
        service.create_dir(&directory.join("nested")).unwrap();
        service.symlink(&file, &link).unwrap();
        service.rename(&file, &link).unwrap();
        service.remove_file(&file).unwrap();
        service.remove_dir(&directory).unwrap();
        service.write_atomic(&file, b"replacement").unwrap();
        service.set_mode(&file, 0o444).unwrap();
        assert_eq!(
            dry.actions(),
            vec![
                FsAction::CreateDir {
                    path: directory.join("nested")
                },
                FsAction::Symlink {
                    target: file.clone(),
                    link: link.clone()
                },
                FsAction::Rename {
                    from: file.clone(),
                    to: link.clone()
                },
                FsAction::RemoveFile { path: file.clone() },
                FsAction::RemoveDir {
                    path: directory.clone()
                },
                FsAction::WriteAtomic {
                    path: file.clone(),
                    bytes: b"replacement".to_vec()
                },
                FsAction::SetMode {
                    path: file.clone(),
                    mode: 0o444
                },
            ]
        );
        assert_eq!(
            service.read_dir(sandbox.path()).unwrap(),
            vec![directory.clone(), file.clone()]
        );
        assert_eq!(fs::read(&file).unwrap(), b"original");
        assert_eq!(fs::metadata(&file).unwrap().permissions(), before);
        assert!(directory.is_dir());
        assert!(!link.exists());
        assert!(service.symlink_metadata(&file).unwrap().is_file());
        assert_eq!(
            service.symlink_metadata(&link).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(service.read_dir(&link).is_err());
        assert!(service.read_link(&file).is_err());
        assert_eq!(dry.take_actions().len(), 7);
        assert!(dry.actions().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_modes_and_atomic_replacement() {
        use std::os::unix::fs::PermissionsExt;
        let sandbox = tempfile::tempdir().unwrap();
        let target = sandbox.path().join("target");
        let link = sandbox.path().join("link");
        let temporary_link = sandbox.path().join("temporary-link");
        RealFs.write_atomic(&target, b"data").unwrap();
        RealFs.set_mode(&target, 0o640).unwrap();
        RealFs.write_atomic(&target, b"updated").unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        RealFs.symlink(&target, &link).unwrap();
        assert!(
            RealFs
                .symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let dry = DryRunFs::new();
        assert_eq!(dry.read_link(&link).unwrap(), target);
        RealFs
            .symlink(Path::new("missing"), &temporary_link)
            .unwrap();
        RealFs.rename(&temporary_link, &link).unwrap();
        assert_eq!(RealFs.read_link(&link).unwrap(), Path::new("missing"));
        assert!(
            RealFs
                .symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        RealFs.remove_file(&link).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"updated");
        // Atomic writes replace a symlink itself, never its target.
        RealFs.symlink(&target, &link).unwrap();
        RealFs.write_atomic(&link, b"separate").unwrap();
        assert!(RealFs.symlink_metadata(&link).unwrap().is_file());
        assert_eq!(fs::read(&target).unwrap(), b"updated");
    }
}
