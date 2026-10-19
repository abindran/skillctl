//! Health checks: registry versus disk, link health, store integrity, user-level
//! folders and the Git environment. `--fix` repairs what is safe to repair
//! (missing instances, link drift, writable store entries); everything else is
//! reported with the command that fixes it. Two advisory sections never affect
//! the exit status: telemetry (see [`super::telemetry`]) reports whether the
//! harnesses export token and cost data, and the MCP audit (see
//! [`super::mcp`]) lists the servers visible from this folder and which of
//! them the event log has never seen used.
use crate::{
    context::Context,
    error::Error,
    linker::{LinkStatus, normalize},
    names::SkillName,
    paths::HomePath,
    store::{self, hash::content_hash, meta::Metadata},
};
use semver::Version;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    pub check: &'static str,
    pub severity: &'static str,
    pub message: String,
    pub fix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    pub fixed: bool,
}

fn problem(
    check: &'static str,
    severity: &'static str,
    message: impl Into<String>,
    fix: impl Into<String>,
    path: Option<PathBuf>,
) -> Problem {
    Problem {
        check,
        severity,
        message: message.into(),
        fix: fix.into(),
        path,
        fixed: false,
    }
}

pub fn run(context: &mut Context<'_>, fix: bool) -> Result<(), Error> {
    let problems = if fix {
        super::pins::write_locked(context, |context| diagnose(context, true))?
    } else {
        super::init::ensure_home(context)?;
        super::read_locked(context, || diagnose(context, false))?
    };
    let telemetry = super::telemetry::inspect(
        |name| std::env::var(name).ok(),
        |binary| {
            std::env::var_os("PATH").is_some_and(|path| {
                std::env::split_paths(&path)
                    .any(|dir| std::fs::metadata(dir.join(binary)).is_ok_and(|info| info.is_file()))
            })
        },
    );
    // MCP servers visible from this folder, scored against the event log.
    let user_home = crate::paths::HomePath::new("~/.claude.json")?
        .expand()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let folder = super::attach::cwd()
        .ok()
        .and_then(|cwd| crate::paths::canonicalize(&cwd).ok());
    let mcp = super::mcp::audit(
        &user_home,
        folder.as_deref(),
        &context.home.home().join("log/events.jsonl"),
    )?;
    let fixed = problems.iter().filter(|item| item.fixed).count();
    let remaining = problems.len() - fixed;
    let problems_text = if problems.is_empty() {
        "No problems found.".to_owned()
    } else {
        let mut lines = problems
            .iter()
            .map(|item| {
                format!(
                    "  [{}]{} {}\n      fix: {}",
                    item.severity,
                    if item.fixed { " fixed:" } else { "" },
                    item.message,
                    item.fix
                )
            })
            .collect::<Vec<_>>();
        lines.push(format!(
            "{} problem(s), {fixed} fixed, {remaining} remaining",
            problems.len()
        ));
        lines.join("\n")
    };
    let text = [telemetry.text(), mcp.text(), problems_text]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    context
        .output
        .result(
            &serde_json::json!({
                "problems": problems,
                "fixed": fixed,
                "remaining": remaining,
                "telemetry": telemetry,
                "mcp": mcp,
                "dry_run": context.flags.dry_run,
            }),
            &text,
        )
        .map_err(super::output_error)?;
    if remaining > 0 {
        return Err(Error::Doctor {
            problems: remaining,
            hint: if fix {
                "Follow the fix suggestions above for the remaining problems.".into()
            } else {
                "Run skillctl doctor --fix to repair what can be repaired automatically.".into()
            },
        });
    }
    Ok(())
}

fn diagnose(context: &Context<'_>, fix: bool) -> Result<Vec<Problem>, Error> {
    let fs = context.fs.as_ref();
    let home = &context.home;
    let mut registry = super::load_registry(context)?;
    let mut problems = Vec::new();
    let mut registry_changed = false;

    // Git environment.
    match context.git.version() {
        Err(_) => problems.push(problem(
            "git-missing",
            "error",
            "git was not found on PATH",
            "Install Git 2.31 or newer and make sure it is on PATH",
            None,
        )),
        Ok(version) if version < Version::new(2, 31, 0) => problems.push(problem(
            "git-old",
            "error",
            format!("git {version} is older than 2.31, which skillctl needs for exclude files"),
            "Upgrade Git to 2.31 or newer",
            None,
        )),
        Ok(_) => {}
    }

    // Instances that vanished or changed type.
    for (name, project) in registry.projects.iter_mut() {
        let mut kept = Vec::new();
        for instance in std::mem::take(&mut project.instances) {
            match fs.symlink_metadata(&instance.path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let mut item = problem(
                        "instance-missing",
                        "error",
                        format!(
                            "Instance {} of project '{name}' no longer exists",
                            instance.path.display()
                        ),
                        "skillctl doctor --fix removes it from the registry",
                        Some(instance.path.clone()),
                    );
                    if fix {
                        item.fixed = true;
                        registry_changed = true;
                    } else {
                        kept.push(instance);
                    }
                    problems.push(item);
                }
                Err(source) => return Err(inspect_error(&instance.path, source)),
                Ok(info) if !info.is_dir() => {
                    problems.push(problem(
                        "instance-replaced",
                        "warning",
                        format!(
                            "Instance {} of project '{name}' is no longer a plain folder",
                            instance.path.display()
                        ),
                        "Run skillctl attach from the real folder, or skillctl detach there",
                        Some(instance.path.clone()),
                    ));
                    kept.push(instance);
                }
                Ok(_) => kept.push(instance),
            }
        }
        project.instances = kept;
    }

    // Pins whose content is missing from the store.
    let scopes = registry
        .projects
        .iter()
        .map(|(name, project)| (format!("project '{name}'"), &project.skills))
        .chain([
            ("the global skills".to_owned(), &registry.global.skills),
            ("the global hooks".to_owned(), &registry.global.hooks),
        ]);
    for (scope, pins) in scopes {
        for (skill, pin) in pins {
            let dir = store::store_dir(home, skill, pin);
            if !is_dir(fs, &dir)? {
                problems.push(problem(
                    "pin-missing",
                    "error",
                    format!(
                        "{skill}@{pin} pinned by {scope} is missing from {}",
                        dir.display()
                    ),
                    format!(
                        "skillctl install <source> to restore {skill}@{pin}, or unpin it with skillctl remove / global remove"
                    ),
                    Some(dir),
                ));
            }
        }
    }

    // Link health in every existing instance and user-level folder.
    let mut checks: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    for (name, project) in &registry.projects {
        let mut pins = project.skills.clone();
        if registry.settings.global_in_instances {
            pins.extend(registry.global.skills.clone());
        }
        for instance in &project.instances {
            if !is_dir(fs, &instance.path)? {
                continue;
            }
            for (skill, pin) in &pins {
                let want = store::store_dir(home, skill, pin);
                for target in &registry.settings.link_targets {
                    checks.push((
                        instance.path.join(target.as_path()).join(skill.as_str()),
                        want.clone(),
                        format!("project '{name}'"),
                    ));
                }
            }
        }
    }
    for (skill, pin) in &registry.global.skills {
        let want = store::store_dir(home, skill, pin);
        for target in &registry.settings.global_link_targets {
            checks.push((
                target.expand()?.join(skill.as_str()),
                want.clone(),
                "the global skills".to_owned(),
            ));
        }
    }
    let hook_target = crate::harness::hook_link_target().expand()?;
    for (hook, pin) in &registry.global.hooks {
        checks.push((
            hook_target.join(hook.as_str()),
            store::store_dir(home, hook, pin),
            "the global hooks".to_owned(),
        ));
    }
    let mut link_problem = false;
    for (path, want, scope) in checks {
        let state = super::status::health(context, &path, &want)?;
        if state == "ok" {
            continue;
        }
        link_problem = true;
        let (check, severity, fix_hint) = match state {
            "missing" => ("link-missing", "error", "skillctl relink --all"),
            "wrong" => ("link-wrong", "error", "skillctl relink --all"),
            "dangling" => (
                "link-dangling",
                "error",
                "Reinstall the pinned version, then skillctl relink --all",
            ),
            _ => (
                "link-conflict",
                "warning",
                "Move the project-owned entry aside, then skillctl relink --all",
            ),
        };
        problems.push(problem(
            check,
            severity,
            format!("{} is {state} ({scope})", path.display()),
            fix_hint,
            Some(path),
        ));
    }

    // Store integrity: metadata, content hash and read-only permissions.
    for skill in store::list_skills(fs, home)? {
        for version in store::list_versions(fs, home, &skill)? {
            let version_dir = home.version_dir(&skill, &version);
            match Metadata::load(fs, &version_dir.join("meta.json")) {
                Err(error) => problems.push(problem(
                    "meta-invalid",
                    "error",
                    format!("{skill}@{version}: {error}"),
                    "Reinstall the version with skillctl install, or unpin it and run skillctl gc",
                    Some(version_dir.clone()),
                )),
                Ok(meta) => match content_hash(fs, &home.store_dir(&skill, &version)) {
                    Ok(hash) if hash == meta.content_hash => {}
                    Ok(_) => problems.push(problem(
                        "hash-mismatch",
                        "error",
                        format!(
                            "{skill}@{version} differs from its recorded content hash; it was modified after installation"
                        ),
                        "Reinstall it from its source, or unpin it and run skillctl gc",
                        Some(version_dir.clone()),
                    )),
                    Err(error) => problems.push(problem(
                        "hash-unreadable",
                        "error",
                        format!("{skill}@{version}: {error}"),
                        "Reinstall the version with skillctl install",
                        Some(version_dir.clone()),
                    )),
                },
            }
            let mut writable = Vec::new();
            collect_writable(fs, &version_dir, &mut writable)?;
            if !writable.is_empty() {
                let mut item = problem(
                    "store-writable",
                    "warning",
                    format!(
                        "{} entries of {skill}@{version} are writable",
                        writable.len()
                    ),
                    "skillctl doctor --fix restores read-only permissions",
                    Some(version_dir.clone()),
                );
                if fix {
                    for (path, mode) in &writable {
                        fs.set_mode(path, *mode)
                            .map_err(|source| inspect_error(path, source))?;
                    }
                    item.fixed = true;
                }
                problems.push(item);
            }
        }
    }

    // Personal copies that shadow project skills, and global target layout.
    let project_skills: BTreeSet<&SkillName> = registry
        .projects
        .values()
        .flat_map(|project| project.skills.keys())
        .collect();
    for folder in ["~/.claude/skills", "~/.agents/skills", "~/.cursor/skills"] {
        let Ok(dir) = HomePath::new(folder).and_then(|path| path.expand()) else {
            continue;
        };
        let dir = match fs.symlink_metadata(&dir) {
            Ok(info) if info.file_type().is_symlink() => crate::paths::canonicalize(&dir)?,
            Ok(info) if info.is_dir() => dir,
            _ => continue,
        };
        for skill in &project_skills {
            let entry = dir.join(skill.as_str());
            match fs.symlink_metadata(&entry) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(inspect_error(&entry, source)),
                Ok(info) => {
                    let managed = info.file_type().is_symlink()
                        && fs.read_link(&entry).is_ok_and(|target| {
                            let target =
                                normalize(&entry.parent().expect("entry parent").join(target));
                            [home.store(), home.drafts()].iter().any(|root| {
                                let root = normalize(root);
                                target != root && target.starts_with(&root)
                            })
                        });
                    if !managed {
                        problems.push(problem(
                            "shadowed",
                            "warning",
                            format!(
                                "{} is a personal copy of project skill '{skill}' and is loaded in every folder",
                                entry.display()
                            ),
                            "Remove or rename the personal copy, or make it a global skill with skillctl global add",
                            Some(entry),
                        ));
                    }
                }
            }
        }
    }
    for target in &registry.settings.global_link_targets {
        let path = target.expand()?;
        match fs.symlink_metadata(&path) {
            Ok(info) if info.file_type().is_symlink() => problems.push(problem(
                "global-target-symlink",
                "warning",
                format!(
                    "{} is itself a symlink; some harnesses mishandle that layout",
                    path.display()
                ),
                "Replace the folder link with a real folder; skillctl links each skill inside the resolved folder",
                Some(path),
            )),
            Ok(_) => {
                if let Some(parent) = path.parent() {
                    if context.git.toplevel(parent)?.is_some() {
                        problems.push(problem(
                            "global-target-in-repo",
                            "warning",
                            format!(
                                "{} lies inside a Git work tree, so global skill links show up there",
                                path.display()
                            ),
                            "Ignore the links in that repository or move the folder out of it",
                            Some(path),
                        ));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(inspect_error(&path, source)),
        }
    }

    // Hook registration: every pinned hook's events are in the harness settings
    // exactly once, and not also in Cursor's own hooks file.
    if !registry.global.hooks.is_empty() {
        let settings_path = crate::harness::claude_settings().expand()?;
        match crate::harness::claude::load(fs, &settings_path) {
            Err(Error::Usage { message, .. }) => problems.push(problem(
                "settings-invalid",
                "error",
                message,
                "Fix the JSON by hand; skillctl never rewrites a settings file it cannot parse",
                Some(settings_path.clone()),
            )),
            Err(error) => return Err(error),
            Ok(settings) => {
                let cursor_path = crate::harness::cursor_hooks().expand()?;
                let cursor = match fs.read(&cursor_path) {
                    Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes).ok(),
                    Err(_) => None,
                };
                for (hook, pin) in &registry.global.hooks {
                    let dir = store::store_dir(home, hook, pin);
                    let Ok(declared) = super::hook::declared(context, &dir) else {
                        continue;
                    };
                    let registered = crate::harness::claude::registered_events(&settings, hook);
                    let missing: Vec<&str> = declared
                        .registrations
                        .iter()
                        .map(|registration| registration.event.as_str())
                        .filter(|event| !registered.contains(*event))
                        .collect();
                    if !missing.is_empty() {
                        problems.push(problem(
                            "hook-unregistered",
                            "error",
                            format!(
                                "Hook '{hook}' is pinned but not registered for {} in {}",
                                missing.join(", "),
                                settings_path.display()
                            ),
                            format!("skillctl hook add {hook}"),
                            Some(settings_path.clone()),
                        ));
                    }
                    let duplicated = crate::harness::claude::duplicated_events(&settings, hook);
                    if !duplicated.is_empty() {
                        problems.push(problem(
                            "hook-duplicate",
                            "warning",
                            format!(
                                "Hook '{hook}' is registered more than once for {}; it runs twice per event",
                                duplicated.join(", ")
                            ),
                            format!(
                                "skillctl hook remove {hook} && skillctl hook add {hook} leaves one entry per event"
                            ),
                            Some(settings_path.clone()),
                        ));
                    }
                    if cursor
                        .as_ref()
                        .is_some_and(|value| crate::harness::claude::mentions(value, hook))
                    {
                        problems.push(problem(
                            "hook-duplicate",
                            "warning",
                            format!(
                                "Hook '{hook}' also appears in {}; Cursor reads the Claude settings too, so it runs twice there",
                                cursor_path.display()
                            ),
                            format!("Remove the {hook} entries from {}", cursor_path.display()),
                            Some(cursor_path.clone()),
                        ));
                    }
                }
            }
        }
    }

    // Repositories that would check links out as plain files.
    for project in registry.projects.values() {
        for instance in &project.instances {
            if !is_dir(fs, &instance.path)? {
                continue;
            }
            if context.git.config_bool(&instance.path, "core.symlinks")? == Some(false) {
                problems.push(problem(
                    "core-symlinks",
                    "error",
                    format!(
                        "core.symlinks is false in {}; Git treats links there as files",
                        instance.path.display()
                    ),
                    format!(
                        "git -C {} config core.symlinks true",
                        instance.path.display()
                    ),
                    Some(instance.path.clone()),
                ));
            }
        }
    }

    if fix {
        if registry_changed && !context.flags.dry_run {
            registry.save(fs, &home.registry())?;
        }
        if link_problem {
            let reports = super::pins::reconcile_everything(context, &mut registry)?;
            let conflicts: BTreeSet<PathBuf> = reports
                .iter()
                .flat_map(|report| report.links.iter())
                .filter(|link| link.status == LinkStatus::Conflict)
                .map(|link| link.path.clone())
                .collect();
            for item in problems
                .iter_mut()
                .filter(|item| item.check.starts_with("link-") && item.check != "link-dangling")
            {
                if item
                    .path
                    .as_ref()
                    .is_some_and(|path| !conflicts.contains(path))
                {
                    item.fixed = true;
                }
            }
        }
    }
    Ok(problems)
}

fn collect_writable(
    fs: &dyn crate::services::fs::Fs,
    dir: &Path,
    out: &mut Vec<(PathBuf, u32)>,
) -> Result<(), Error> {
    #[cfg(unix)]
    fn writable(info: &std::fs::Metadata) -> (bool, bool) {
        use std::os::unix::fs::PermissionsExt;
        let mode = info.permissions().mode();
        (mode & 0o222 != 0, mode & 0o111 != 0)
    }
    #[cfg(not(unix))]
    fn writable(_info: &std::fs::Metadata) -> (bool, bool) {
        (false, false)
    }
    let info = fs
        .symlink_metadata(dir)
        .map_err(|source| inspect_error(dir, source))?;
    if writable(&info).0 {
        out.push((dir.to_path_buf(), 0o555));
    }
    for entry in fs
        .read_dir(dir)
        .map_err(|source| inspect_error(dir, source))?
    {
        let info = fs
            .symlink_metadata(&entry)
            .map_err(|source| inspect_error(&entry, source))?;
        if info.file_type().is_symlink() {
            continue;
        } else if info.is_dir() {
            collect_writable(fs, &entry, out)?;
        } else if info.is_file() {
            let (is_writable, executable) = writable(&info);
            if is_writable {
                out.push((entry, if executable { 0o555 } else { 0o444 }));
            }
        }
    }
    Ok(())
}

fn is_dir(fs: &dyn crate::services::fs::Fs, path: &Path) -> Result<bool, Error> {
    match fs.symlink_metadata(path) {
        Ok(info) => Ok(info.is_dir()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(inspect_error(path, source)),
    }
}

fn inspect_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Inspect {}", path.display()),
        source,
        hint: "Check the path and its permissions.".into(),
    }
}
