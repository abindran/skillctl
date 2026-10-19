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
    artifact::Kind,
    error::Error,
    names::SkillName,
    paths::{Paths, RelPath},
    registry::{Instance, Pin, Project, Settings},
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

/// How pins map to links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// By artifact kind: skills into every `link_targets` folder, rules into
    /// `.cursor/rules/<name>.mdc`, commands into `.claude/commands/<name>.md`
    /// and `.cursor/commands/<name>.md`. Project instances use this.
    Kinds,
    /// Every pin as a folder link into every `link_targets` folder, whatever
    /// its kind. User-level folders (`~/.claude/skills`, `~/.claude/hooks`)
    /// use this.
    Folders,
}

/// One link to maintain: `<root>/<dir>/<name>` pointing at `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPlan {
    pub dir: RelPath,
    pub name: String,
    pub target: PathBuf,
}

impl LinkPlan {
    pub fn path(&self, root: &Path) -> PathBuf {
        root.join(self.dir.as_path()).join(&self.name)
    }
}

const RULES_DIR: &str = ".cursor/rules";
const COMMAND_DIRS: [&str; 2] = [".claude/commands", ".cursor/commands"];
const CLAUDE_AGENTS_DIR: &str = ".claude/agents";
const CURSOR_AGENTS_DIR: &str = ".cursor/agents";
/// The Cursor override an agent folder may carry beside `AGENT.md`.
pub const AGENT_CURSOR_OVERRIDE: &str = "cursor.md";

/// The links a set of pins needs. A pin whose kind cannot be read (version
/// missing from the store) is planned as a skill so its dangling links still
/// show up for doctor.
pub fn plans(
    fs: &dyn Fs,
    paths: &Paths,
    pins: &std::collections::BTreeMap<SkillName, Pin>,
    link_targets: &[RelPath],
    layout: Layout,
) -> Result<Vec<LinkPlan>, Error> {
    let mut plans = Vec::new();
    // A folder listed twice gets one link, not two outcomes.
    let link_targets: BTreeSet<&RelPath> = link_targets.iter().collect();
    for (name, pin) in pins {
        let dir = store::store_dir(paths, name, pin);
        let kind = match layout {
            Layout::Folders => Kind::Skill,
            Layout::Kinds => Kind::detect(fs, &dir)?.unwrap_or(Kind::Skill),
        };
        match kind {
            Kind::Skill => {
                for target in &link_targets {
                    plans.push(LinkPlan {
                        dir: (*target).clone(),
                        name: name.as_str().to_owned(),
                        target: dir.clone(),
                    });
                }
            }
            Kind::Rule => plans.push(LinkPlan {
                dir: RelPath::new(RULES_DIR)?,
                name: format!("{name}.mdc"),
                target: dir.join(Kind::Rule.document()),
            }),
            Kind::Command => {
                for commands in COMMAND_DIRS {
                    plans.push(LinkPlan {
                        dir: RelPath::new(commands)?,
                        name: format!("{name}.md"),
                        target: dir.join(Kind::Command.document()),
                    });
                }
            }
            Kind::Agent => {
                plans.push(LinkPlan {
                    dir: RelPath::new(CLAUDE_AGENTS_DIR)?,
                    name: format!("{name}.md"),
                    target: dir.join(Kind::Agent.document()),
                });
                // Cursor imports .claude/agents itself; a cursor.md override
                // exists only when the model or skill preload must differ.
                let cursor = dir.join(AGENT_CURSOR_OVERRIDE);
                if fs
                    .symlink_metadata(&cursor)
                    .is_ok_and(|info| info.is_file())
                {
                    plans.push(LinkPlan {
                        dir: RelPath::new(CURSOR_AGENTS_DIR)?,
                        name: format!("{name}.md"),
                        target: cursor,
                    });
                }
            }
            // Hooks are registered with `hook add`, never linked into a project.
            Kind::Hook => {}
        }
    }
    Ok(plans)
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
    layout: Layout,
) -> Result<ReconcileReport, Error> {
    let mut report = ReconcileReport::default();
    let mut planned_dirs = BTreeSet::new();
    let plans = plans(fs, paths, &project.skills, &settings.link_targets, layout)?;
    // Folders to visit: every configured skills folder (to clean links of
    // unpinned skills), every folder a plan needs, and obsolete folders.
    let mut active: BTreeSet<RelPath> = settings.link_targets.iter().cloned().collect();
    active.extend(plans.iter().map(|plan| plan.dir.clone()));
    if layout == Layout::Kinds {
        // Visit every kind folder even with no plan for it, so the links of
        // an unpinned rule or command are removed.
        for dir in [RULES_DIR, CLAUDE_AGENTS_DIR, CURSOR_AGENTS_DIR]
            .into_iter()
            .chain(COMMAND_DIRS)
        {
            active.insert(RelPath::new(dir)?);
        }
    }
    let obsolete: BTreeSet<_> = settings
        .previous_link_targets
        .iter()
        .filter(|target| !active.contains(*target))
        .cloned()
        .collect();
    for target in active.iter().chain(obsolete.iter()) {
        let dir = instance.path.join(target.as_path());
        let current = active.contains(target);
        let here: Vec<&LinkPlan> = plans.iter().filter(|plan| &plan.dir == target).collect();
        if !safe_directory(fs, &instance.path, target)? {
            if current {
                for plan in &here {
                    report.links.push(LinkOutcome {
                        path: dir.join(&plan.name),
                        target: Some(plan.target.clone()),
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
                    .is_some_and(|name| here.iter().any(|plan| plan.name == name));
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
        for plan in here {
            let path = dir.join(&plan.name);
            let want = plan.target.clone();
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
/// Lexical normalization of `.` and `..` without touching the filesystem, so
/// dangling links can still be classified.
pub(crate) fn normalize(path: &Path) -> PathBuf {
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
