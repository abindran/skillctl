use std::io;

use crate::context::{Context, Flags};
use crate::paths::Paths;
use crate::services::fs::{DryRunFs, Fs, FsAction};
use crate::services::prompt::ScriptedPrompter;

use super::init::{DEFAULT_HOOK_SCRIPT, DEFAULT_TEMPLATE, initialize};

#[test]
fn injected_dry_run_records_the_layout_without_disk_writes_or_prompts() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("skills");
    let fs = DryRunFs::new();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let context = Context::new(
        Paths::resolve(Some(&home)).unwrap(),
        Box::new(BorrowedFs(&fs)),
        Box::new(ScriptedPrompter::new([])),
        &mut stdout,
        &mut stderr,
        Flags {
            dry_run: true,
            ..Flags::default()
        },
    );
    let report = initialize(&context).unwrap();
    // home, store, drafts, templates, templates/default, templates/hook,
    // registry.yml, and three template files.
    assert_eq!(report.created.len(), 10);
    assert!(report.dry_run);
    assert!(!home.exists());
    let actions = fs.actions();
    // Each template file is a write followed by a mode change.
    assert_eq!(actions.len(), 13);
    assert!(matches!(&actions[0], FsAction::CreateDir { path } if path == &home));
    assert!(
        matches!(&actions[5], FsAction::CreateDir { path } if path == &home.join("templates/hook"))
    );
    assert!(matches!(&actions[6], FsAction::WriteAtomic { path, bytes }
        if path == &home.join("registry.yml") && crate::registry::Registry::from_yaml(bytes).unwrap() == crate::registry::Registry::default()));
    assert!(matches!(&actions[7], FsAction::WriteAtomic { path, bytes }
        if path == &home.join("templates/default/SKILL.md") && bytes == DEFAULT_TEMPLATE.as_bytes()));
    assert!(matches!(&actions[11], FsAction::WriteAtomic { path, bytes }
        if path == &home.join("templates/hook/hook.py") && bytes == DEFAULT_HOOK_SCRIPT.as_bytes()));
    assert!(
        matches!(&actions[12], FsAction::SetMode { path, mode: 0o755 }
        if path == &home.join("templates/hook/hook.py"))
    );
    drop(context);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

// Borrow a recording service while Context owns its object-safe wrapper.
struct BorrowedFs<'a>(&'a DryRunFs);

impl Fs for BorrowedFs<'_> {
    fn symlink_metadata(&self, path: &std::path::Path) -> io::Result<std::fs::Metadata> {
        self.0.symlink_metadata(path)
    }
    fn read_link(&self, path: &std::path::Path) -> io::Result<std::path::PathBuf> {
        self.0.read_link(path)
    }
    fn read_dir(&self, path: &std::path::Path) -> io::Result<Vec<std::path::PathBuf>> {
        self.0.read_dir(path)
    }
    fn create_dir(&self, path: &std::path::Path) -> io::Result<()> {
        self.0.create_dir(path)
    }
    fn symlink(&self, target: &std::path::Path, link: &std::path::Path) -> io::Result<()> {
        self.0.symlink(target, link)
    }
    fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> io::Result<()> {
        self.0.rename(from, to)
    }
    fn remove_file(&self, path: &std::path::Path) -> io::Result<()> {
        self.0.remove_file(path)
    }
    fn remove_dir(&self, path: &std::path::Path) -> io::Result<()> {
        self.0.remove_dir(path)
    }
    fn write_atomic(&self, path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
        self.0.write_atomic(path, bytes)
    }
    fn set_mode(&self, path: &std::path::Path, mode: u32) -> io::Result<()> {
        self.0.set_mode(path, mode)
    }
}
