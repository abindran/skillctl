//! Cross-process tests using the test executable as a controlled lock holder.
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

use skillctl::{
    registry::{Project, Registry, lock::RegistryLock},
    services::fs::RealFs,
};

// Ensure a failing test cannot leave a process holding a lock indefinitely.
struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn worker(root: &Path, name: &str) -> Worker {
    Worker(Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "lock_worker", "--nocapture"])
        .env("SKILLCTL_LOCK_TEST_ROOT", root)
        .env("SKILLCTL_LOCK_TEST_NAME", name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn().unwrap())
}

fn wait_until_locked(worker: &mut Worker) {
    let mut output = BufReader::new(worker.0.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        assert_ne!(output.read_line(&mut line).unwrap(), 0, "worker exited before locking");
        if line.trim() == "LOCKED" { break; }
        line.clear();
    }
}

#[test]
fn concurrent_writers_preserve_both_registry_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    Registry::default().save(&RealFs, &root.join("registry.yml")).unwrap();
    let mut first = worker(root, "first");
    wait_until_locked(&mut first);
    // Start the second writer while the first still holds the exclusive lock.
    let mut second = worker(root, "second");
    first.0.stdin.take().unwrap().write_all(b"continue\n").unwrap();
    assert!(first.0.wait().unwrap().success());
    assert!(second.0.wait().unwrap().success());
    let registry = Registry::load(&RealFs, &root.join("registry.yml")).unwrap();
    assert!(registry.projects.contains_key(&"first".parse().unwrap()));
    assert!(registry.projects.contains_key(&"second".parse().unwrap()));
}

#[test]
fn process_termination_releases_lock_without_deleting_lock_file() {
    let temp = tempfile::tempdir().unwrap();
    let mut child = worker(temp.path(), "first");
    wait_until_locked(&mut child);
    let path = temp.path().join("registry.lock");
    let mut lock = RegistryLock::open(&path).unwrap();
    assert!(lock.with_exclusive_timeout(Duration::ZERO, || Ok(())).is_err());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    lock.with_exclusive_timeout(Duration::from_secs(1), || Ok(())).unwrap();
    assert!(path.exists());
}

#[test]
#[ignore = "subprocess helper invoked by the parent tests"]
fn lock_worker() {
    let root = std::env::var_os("SKILLCTL_LOCK_TEST_ROOT").expect("helper root");
    let root = Path::new(&root);
    let name = std::env::var("SKILLCTL_LOCK_TEST_NAME").unwrap();
    let mut lock = RegistryLock::open(&root.join("registry.lock")).unwrap();
    lock.with_exclusive(|| {
        if name == "first" {
            println!("LOCKED");
            std::io::stdout().flush().unwrap();
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).unwrap();
        }
        let mut registry = Registry::load(&RealFs, &root.join("registry.yml"))?;
        registry.projects.insert(name.parse().unwrap(), Project::default());
        registry.save(&RealFs, &root.join("registry.yml"))
    }).unwrap();
}
