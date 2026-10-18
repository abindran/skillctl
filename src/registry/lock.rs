//! Advisory locking for the entire registry operation, not just the save.
//!
//! Open the stable `registry.lock` file after home initialization, then run all
//! reads (and any subsequent writes/reconciliation) inside one scoped operation.
//! Never unlink or atomically replace the lock file: other processes may hold it.
//! Locks are released on return, error, panic unwinding, or process termination.
//! Dry runs must also lock while reading state; locking is coordination, not a
//! planned registry mutation. This module does not create the home directory.
use std::{
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use fd_lock::RwLock;

use crate::error::Error;

/// Maximum time spent waiting for another command to release the registry.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// An open handle to the stable registry lock file.
///
/// Separate commands must open separate handles. The callback API ensures the
/// lock outlives the complete read/modify/write operation, even on early errors.
#[derive(Debug)]
pub struct RegistryLock {
    inner: RwLock<File>,
    path: PathBuf,
}

impl RegistryLock {
    /// Open or create the lock file without truncating existing contents.
    /// The parent directory must already exist.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|source| Error::Io {
                operation: format!("Open registry lock {}", path.display()),
                source,
                hint: "Check the skillctl home directory and its permissions.".into(),
            })?;
        Ok(Self {
            inner: RwLock::new(file),
            path: path.to_owned(),
        })
    }

    /// Run a read-only operation under a shared lock, waiting up to 10 seconds.
    pub fn with_shared<T>(&self, operation: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
        self.with_shared_timeout(LOCK_TIMEOUT, operation)
    }

    /// Run a complete mutating operation under an exclusive lock.
    /// Acquire this before loading the registry, not only before saving it.
    pub fn with_exclusive<T>(
        &mut self,
        operation: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        self.with_exclusive_timeout(LOCK_TIMEOUT, operation)
    }

    /// Shared locking with a custom wait limit (zero makes one immediate attempt).
    pub fn with_shared_timeout<T>(
        &self,
        timeout: Duration,
        operation: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        let start = Instant::now();
        loop {
            match self.inner.try_read() {
                Ok(_guard) => return operation(),
                Err(error) => wait_or_fail(&self.path, "shared", start, timeout, error)?,
            }
        }
    }

    /// Exclusive locking with a custom wait limit, primarily for tests.
    pub fn with_exclusive_timeout<T>(
        &mut self,
        timeout: Duration,
        operation: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        let start = Instant::now();
        loop {
            match self.inner.try_write() {
                Ok(_guard) => return operation(),
                Err(error) => wait_or_fail(&self.path, "exclusive", start, timeout, error)?,
            }
        }
    }
}

fn wait_or_fail(
    path: &Path,
    mode: &str,
    start: Instant,
    timeout: Duration,
    error: io::Error,
) -> Result<(), Error> {
    let elapsed = start.elapsed();
    let source = match error.kind() {
        io::ErrorKind::WouldBlock if elapsed < timeout => {
            thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(elapsed)));
            return Ok(());
        }
        io::ErrorKind::Interrupted if elapsed < timeout => return Ok(()),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "Timed out after {timeout:?} waiting for a {mode} registry lock; another skillctl process may be running"
            ),
        ),
        _ => error,
    };
    Err(Error::Io {
        operation: format!("Acquire {mode} registry lock {}", path.display()),
        source,
        hint: "Wait for the other skillctl command to finish, then retry. Do not delete registry.lock.".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_readers_coexist_and_exclude_writers() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("registry.lock");
        let first = RegistryLock::open(&path).unwrap();
        let second = RegistryLock::open(&path).unwrap();
        let mut writer = RegistryLock::open(&path).unwrap();
        first.with_shared(|| second.with_shared(|| {
            let error = writer.with_exclusive_timeout::<()>(Duration::ZERO, || panic!("writer entered")).unwrap_err();
            assert!(matches!(error, Error::Io { source, .. } if source.kind() == io::ErrorKind::TimedOut));
            Ok(())
        })).unwrap();
        writer.with_exclusive(|| Ok(())).unwrap();
    }

    #[test]
    fn exclusive_lock_excludes_readers_and_writers_until_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("registry.lock");
        let mut owner = RegistryLock::open(&path).unwrap();
        let mut other = RegistryLock::open(&path).unwrap();
        owner
            .with_exclusive(|| {
                let start = Instant::now();
                let error = other
                    .with_shared_timeout(Duration::from_millis(60), || Ok(()))
                    .unwrap_err();
                assert!(start.elapsed() >= Duration::from_millis(60));
                assert_eq!(error.exit_code(), 1);
                assert!(error.to_string().contains("another skillctl process"));
                assert!(error.hint().contains("Do not delete"));
                assert!(
                    other
                        .with_exclusive_timeout(Duration::ZERO, || Ok(()))
                        .is_err()
                );
                Ok(())
            })
            .unwrap();
        other.with_shared(|| Ok(())).unwrap();
    }

    #[test]
    fn releases_on_error_and_panic_and_preserves_file_contents() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("registry.lock");
        std::fs::write(&path, b"existing").unwrap();
        let mut owner = RegistryLock::open(&path).unwrap();
        let mut other = RegistryLock::open(&path).unwrap();
        assert!(
            owner
                .with_exclusive::<()>(|| Err(Error::Registry {
                    message: "test failure".into(),
                    hint: "test".into(),
                }))
                .is_err()
        );
        other
            .with_exclusive_timeout(Duration::ZERO, || Ok(()))
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = owner.with_exclusive::<()>(|| panic!("test panic"));
        }));
        assert!(result.is_err());
        other
            .with_exclusive_timeout(Duration::ZERO, || Ok(()))
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"existing");
        assert_eq!(LOCK_TIMEOUT, Duration::from_secs(10));
    }

    #[test]
    fn waiting_reader_and_writer_acquire_after_release() {
        for shared in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("registry.lock");
            let mut owner = RegistryLock::open(&path).unwrap();
            let (contended_tx, contended_rx) = std::sync::mpsc::channel();
            let waiter = owner
                .with_exclusive(|| {
                    let waiter = std::thread::spawn(move || {
                        let mut lock = RegistryLock::open(&path).unwrap();
                        let error = if shared {
                            lock.with_shared_timeout(Duration::ZERO, || Ok(()))
                        } else {
                            lock.with_exclusive_timeout(Duration::ZERO, || Ok(()))
                        }
                        .unwrap_err();
                        assert!(matches!(error, Error::Io { source, .. }
                        if source.kind() == io::ErrorKind::TimedOut));
                        contended_tx.send(()).unwrap();
                        if shared {
                            lock.with_shared(|| Ok(42))
                        } else {
                            lock.with_exclusive(|| Ok(42))
                        }
                    });
                    contended_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                    Ok(waiter)
                })
                .unwrap();
            assert_eq!(waiter.join().unwrap().unwrap(), 42);
        }
    }

    #[test]
    fn non_contention_errors_fail_immediately() {
        let error = wait_or_fail(
            Path::new("registry.lock"),
            "exclusive",
            Instant::now(),
            LOCK_TIMEOUT,
            io::Error::new(io::ErrorKind::PermissionDenied, "test denial"),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Io { source, .. }
            if source.kind() == io::ErrorKind::PermissionDenied));
    }

    #[test]
    fn missing_parent_is_an_io_error_without_creating_directories() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing/registry.lock");
        let error = RegistryLock::open(&path).unwrap_err();
        assert!(
            matches!(error, Error::Io { source, .. } if source.kind() == io::ErrorKind::NotFound)
        );
        assert!(!path.parent().unwrap().exists());
    }
}
