//! Converge instance links on registry pins. Call under the exclusive registry lock.
//!
//! Only symlinks into the store or drafts are owned. Git exclude maintenance is
//! layered on this report by the exclude work item; this module never edits Git.
pub mod exclude;

use std::{
    collections::BTreeSet,
    io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::Serialize;

use crate::{
    error::Error,
    paths::{Paths, RelPath},
    registry::{Instance, Project, Settings},
    services::fs::Fs,
    store,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkStatus {
    Linked,
    Ok,
    Updated,
    Removed,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkOutcome {
    pub path: PathBuf,
    pub target: Option<PathBuf>,
    pub status: LinkStatus,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct ReconcileReport {
    pub links: Vec<LinkOutcome>,
}

impl ReconcileReport {
    pub fn has_conflicts(&self) -> bool {
        self.links
            .iter()
            .any(|link| link.status == LinkStatus::Conflict)
    }
}

/// Reconcile configured targets and clean managed links from obsolete targets.
///
/// Missing pinned content still gets a link: doctor reports missing versions.
/// Conflicts are collected, not errors; callers map them to exit code 4. I/O
/// failures stop the run, leaving intended registry pins repairable by relink.
/// Persist `instance.created_dirs` after reconciliation (also after partial I/O
/// failure). A dry-run caller should pass a cloned instance, not save it.
/// Target directory symlinks are conflicts, not traversed: they may lead outside
/// the instance to project-owned data. No directories are deleted here.
pub fn reconcile(
    fs: &dyn Fs,
    paths: &Paths,
    instance: &mut Instance,
    project: &Project,
    settings: &Settings,
) -> Result<ReconcileReport, Error> {
    let mut report = ReconcileReport::default();
    let mut planned_dirs = BTreeSet::new();
    let active: BTreeSet<_> = settings.link_targets.iter().cloned().collect();
    let obsolete: BTreeSet<_> = settings
        .previous_link_targets
        .iter()
        .filter(|target| !active.contains(*target))
        .cloned()
        .collect();
    for target in active.iter().chain(obsolete.iter()) {
        let dir = instance.path.join(target.as_path());
        let current = active.contains(target);
        if !safe_directory(fs, &instance.path, target)? {
            if current {
                for (name, pin) in &project.skills {
                    report.links.push(LinkOutcome {
                        path: dir.join(name.as_str()),
                        target: Some(store::store_dir(paths, name, pin)),
                        status: LinkStatus::Conflict,
                    });
                }
            }
            continue;
        }
        let mut entries = match fs.read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => vec![],
            Err(error) => return Err(io_error("List link target", &dir, error)),
        };
        entries.sort();
        for entry in entries {
            let wanted = current
                && entry
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| project.skills.keys().any(|skill| skill.as_str() == name));
            if !wanted && managed_target(fs, paths, &entry)?.is_some() {
                fs.remove_file(&entry)
                    .map_err(|error| io_error("Remove managed link", &entry, error))?;
                report.links.push(LinkOutcome {
                    path: entry,
                    target: None,
                    status: LinkStatus::Removed,
                });
            }
        }
        if !current {
            continue;
        }
        for (name, pin) in &project.skills {
            let path = dir.join(name.as_str());
            let want = store::store_dir(paths, name, pin);
            let status = match fs.symlink_metadata(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    ensure_dir(fs, instance, target, &mut planned_dirs)?;
                    fs.symlink(&want, &path)
                        .map_err(|error| io_error("Create skill link", &path, error))?;
                    LinkStatus::Linked
                }
                Err(error) => return Err(io_error("Inspect skill link", &path, error)),
                Ok(_) => match managed_target(fs, paths, &path)? {
                    Some(existing) if existing == want => LinkStatus::Ok,
                    Some(_) => {
                        replace_link(fs, &want, &path)?;
                        LinkStatus::Updated
                    }
                    None => LinkStatus::Conflict,
                },
            };
            report.links.push(LinkOutcome {
                path,
                target: Some(want),
                status,
            });
        }
    }
    Ok(report)
}

fn io_error(operation: &str, path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("{operation} {}", path.display()),
        source,
        hint: "Check folder permissions, then run skillctl relink to repair links.".into(),
    }
}

// Normalize without following links, so dangling links remain recognizable and
// a misleading store/../../outside prefix never grants ownership.
fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            _ => result.push(component.as_os_str()),
        }
    }
    result
}

fn managed_target(fs: &dyn Fs, paths: &Paths, link: &Path) -> Result<Option<PathBuf>, Error> {
    let metadata = match fs.symlink_metadata(link) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("Inspect link ownership", link, error)),
    };
    if !metadata.file_type().is_symlink() {
        return Ok(None);
    }
    let target = fs
        .read_link(link)
        .map_err(|error| io_error("Read skill link", link, error))?;
    let target = normalize(&link.parent().expect("link has a parent").join(target));
    let owned = [paths.store(), paths.drafts()].iter().any(|root| {
        let root = normalize(root);
        target != root && target.starts_with(&root)
    });
    Ok(owned.then_some(target))
}

// Reject symlinks and non-directories at every component before listing/writing.
fn safe_directory(fs: &dyn Fs, root: &Path, relative: &RelPath) -> Result<bool, Error> {
    let mut path = root.to_path_buf();
    for component in std::iter::once(None).chain(relative.as_path().components().map(Some)) {
        if let Some(component) = component {
            path.push(component.as_os_str());
        }
        match fs.symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(io_error("Inspect link directory", &path, error)),
        }
    }
    Ok(true)
}

fn ensure_dir(
    fs: &dyn Fs,
    instance: &mut Instance,
    target: &RelPath,
    planned: &mut BTreeSet<PathBuf>,
) -> Result<(), Error> {
    let mut relative = PathBuf::new();
    for component in target.as_path().components() {
        relative.push(component.as_os_str());
        let path = instance.path.join(&relative);
        if planned.contains(&path) {
            continue;
        }
        match fs.symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(io_error(
                    "Create link directory",
                    &path,
                    io::Error::new(io::ErrorKind::AlreadyExists, "Path is not a directory"),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs.create_dir(&path)
                    .map_err(|error| io_error("Create link directory", &path, error))?;
                let relative = RelPath::new(relative.clone())?;
                if !instance.created_dirs.contains(&relative) {
                    instance.created_dirs.push(relative);
                }
                planned.insert(path);
            }
            Err(error) => return Err(io_error("Inspect link directory", &path, error)),
        }
    }
    Ok(())
}

fn replace_link(fs: &dyn Fs, want: &Path, link: &Path) -> Result<(), Error> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = link.parent().expect("link has a parent");
    // Exclusive creation never clobbers a pre-existing temporary path. Unique
    // names also let dry runs plan multiple replacements without an overlay.
    for _ in 0..100 {
        let temporary = parent.join(format!(
            ".skillctl-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs.symlink(want, &temporary) {
            Ok(()) => {
                if let Err(error) = fs.rename(&temporary, link) {
                    let _ = fs.remove_file(&temporary);
                    return Err(io_error("Replace skill link", link, error));
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error("Stage replacement link", &temporary, error)),
        }
    }
    Err(io_error(
        "Stage replacement link",
        link,
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Could not reserve a temporary link",
        ),
    ))
}
