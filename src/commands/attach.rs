use crate::{
    context::Context,
    error::Error,
    linker::{self, LinkStatus},
    names::{ProjectName, SkillName},
    registry::{Instance, Pin, Project, Registry, lock::RegistryLock},
    store,
};
use chrono::Utc;
use std::{collections::BTreeMap, path::PathBuf};

pub(super) fn usage(message: impl Into<String>) -> Error {
    Error::Usage {
        message: message.into(),
        hint: "Run skillctl --help for explicit non-interactive options.".into(),
    }
}
pub(super) fn cwd() -> Result<PathBuf, Error> {
    std::env::current_dir().map_err(|source| Error::Io {
        operation: "Read current directory".into(),
        source,
        hint: "Run from an accessible directory.".into(),
    })
}
pub(super) fn now() -> chrono::DateTime<Utc> {
    std::time::SystemTime::now().into()
}
/// Resolve `name[@pin]` to a linkable skill. Hooks live in the same store but
/// are registered with a harness (`hook add`) rather than linked into skills
/// folders, so they are refused here.
pub(super) fn skill_pin(context: &Context<'_>, value: &str) -> Result<(SkillName, Pin), Error> {
    let (name, version) = value
        .split_once('@')
        .map_or((value, None), |(n, v)| (n, Some(v)));
    let name: SkillName = name.parse()?;
    let requested = version.map(str::parse).transpose()?;
    let pin = store::resolve_pin(
        context.fs.as_ref(),
        &context.home,
        &name,
        requested.as_ref(),
    )?;
    let dir = store::store_dir(&context.home, &name, &pin);
    if crate::artifact::Kind::of(context.fs.as_ref(), &dir)? == crate::artifact::Kind::Hook {
        return Err(usage(format!(
            "'{name}' is a hook, not a skill; register it with skillctl hook add {name}"
        )));
    }
    Ok((name, pin))
}

/// Persist desired state before linking, and created-dir bookkeeping even if
/// reconciliation fails midway. A dry run operates on an in-memory registry.
pub(super) fn reconcile_instance(
    context: &Context<'_>,
    registry: &mut Registry,
    name: &ProjectName,
    index: usize,
) -> Result<linker::ReconcileReport, Error> {
    let mut project = registry.projects[name].clone();
    if registry.settings.global_in_instances {
        project.skills.extend(registry.global.skills.clone());
    }
    let instance = &mut registry
        .projects
        .get_mut(name)
        .expect("known project")
        .instances[index];
    let result = linker::reconcile(
        context.fs.as_ref(),
        &context.home,
        instance,
        &project,
        &registry.settings,
    );
    let report = match result {
        Ok(report) => report,
        Err(error) => {
            if !context.flags.dry_run {
                registry.save(context.fs.as_ref(), &context.home.registry())?;
            }
            return Err(error);
        }
    };
    let path = instance.path.clone();
    if !context.flags.dry_run {
        registry.save(context.fs.as_ref(), &context.home.registry())?;
    }
    if registry.settings.git_exclude {
        linker::exclude::update(
            context.fs.as_ref(),
            context.git.as_ref(),
            &path,
            report
                .links
                .iter()
                .filter(|l| {
                    matches!(
                        l.status,
                        LinkStatus::Linked | LinkStatus::Updated | LinkStatus::Ok
                    )
                })
                .map(|l| l.path.clone()),
        )?;
    }
    Ok(report)
}

pub fn run(
    context: &mut Context<'_>,
    project: Option<&ProjectName>,
    new: Option<&ProjectName>,
    skills: &[String],
    top_level: bool,
    here: bool,
) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    if context.flags.dry_run {
        attach(context, project, new, skills, top_level, here)
    } else {
        RegistryLock::open(&context.home.registry_lock())?
            .with_exclusive(|| attach(context, project, new, skills, top_level, here))
    }
}

fn attach(
    context: &mut Context<'_>,
    chosen: Option<&ProjectName>,
    new: Option<&ProjectName>,
    skills: &[String],
    top_level: bool,
    here: bool,
) -> Result<(), Error> {
    let mut registry = super::load_registry(context)?;
    let mut path = crate::paths::canonicalize(&cwd()?)?;
    if let Some(root) = context.git.toplevel(&path)? {
        if root != path && !here {
            context.prompt.set_answer_flag("--top-level or --here");
            if top_level
                || context.prompt.confirm(
                    &format!(
                        "Inside Git worktree {}. Attach its top level instead?",
                        root.display()
                    ),
                    true,
                )?
            {
                path = crate::paths::canonicalize(&root)?;
            }
        }
    }
    // Refuse nesting with a usage error before touching the registry.
    for (other_name, other) in &registry.projects {
        for instance in &other.instances {
            if instance.path != path
                && (path.starts_with(&instance.path) || instance.path.starts_with(&path))
            {
                return Err(usage(format!(
                    "{} is nested with instance {} of project '{other_name}'",
                    path.display(),
                    instance.path.display()
                )));
            }
        }
    }
    let existing = registry.projects.iter().find_map(|(name, p)| {
        p.instances
            .iter()
            .position(|i| i.path == path)
            .map(|index| (name.clone(), index))
    });
    let (name, create) = if let Some(name) = new {
        (name.clone(), true)
    } else if let Some(name) = chosen {
        (name.clone(), false)
    } else {
        let mut choices: Vec<_> = registry
            .projects
            .iter()
            .map(|(name, p)| {
                format!(
                    "{name} ({} instances, {} skills)",
                    p.instances.len(),
                    p.skills.len()
                )
            })
            .collect();
        choices.push("Create a new project".into());
        context.prompt.set_answer_flag("--project or --new");
        let selected = context.prompt.select("Choose a project", &choices, 0)?;
        if selected == choices.len() - 1 {
            context.prompt.set_answer_flag("--new");
            let value = context.prompt.input("Project name", &|v| {
                v.parse::<ProjectName>()
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })?;
            (value.parse()?, true)
        } else {
            (
                registry
                    .projects
                    .keys()
                    .nth(selected)
                    .ok_or_else(|| usage("Invalid project choice"))?
                    .clone(),
                false,
            )
        }
    };
    if create && registry.projects.contains_key(&name) {
        return Err(usage(format!(
            "Project '{name}' already exists; use --project"
        )));
    }
    if !create && !registry.projects.contains_key(&name) {
        return Err(usage(format!("Unknown project '{name}'; use --new")));
    }
    if !create && !skills.is_empty() {
        return Err(usage(
            "--skills is only supported when creating a project with --new",
        ));
    }
    let mut pins = BTreeMap::new();
    if create {
        if skills.is_empty() {
            let available = store::list_skills(context.fs.as_ref(), &context.home)?
                .into_iter()
                .filter_map(|n| {
                    store::latest_stable(context.fs.as_ref(), &context.home, &n)
                        .ok()
                        .map(|v| (n, Pin::Version(v)))
                })
                .collect::<Vec<_>>();
            if !available.is_empty() {
                let items = available
                    .iter()
                    .map(|(n, p)| format!("{n}@{p}"))
                    .collect::<Vec<_>>();
                context.prompt.set_answer_flag("--skills");
                for index in context.prompt.multi_select(
                    "Select skills",
                    &items,
                    &vec![true; items.len()],
                )? {
                    let (n, p) = available
                        .get(index)
                        .ok_or_else(|| usage("Invalid skill choice"))?;
                    pins.insert(n.clone(), p.clone());
                }
            }
        } else {
            for value in skills {
                let (n, p) = skill_pin(context, value)?;
                pins.insert(n, p);
            }
        }
    }
    if let Some(skill) = pins
        .keys()
        .find(|k| registry.global.skills.contains_key(*k))
    {
        return Err(usage(format!("Skill '{skill}' is pinned globally")));
    }
    // Retain recorded folders when moving an instance between projects.
    let instance = if let Some((old, index)) = existing {
        registry
            .projects
            .get_mut(&old)
            .expect("existing project")
            .instances
            .remove(index)
    } else {
        Instance {
            path: path.clone(),
            attached_at: now(),
            created_dirs: vec![],
        }
    };
    if create {
        registry.projects.insert(
            name.clone(),
            Project {
                skills: pins,
                instances: vec![],
            },
        );
    }
    let target = registry.projects.get_mut(&name).expect("chosen project");
    target.instances.push(instance);
    let index = target.instances.len() - 1;
    registry.validate()?;
    if !context.flags.dry_run {
        registry.save(context.fs.as_ref(), &context.home.registry())?;
    }
    let report = reconcile_instance(context, &mut registry, &name, index)?;
    let advisory = if create && !registry.projects[&name].skills.is_empty() {
        let checked = registry.projects[&name].skills.clone();
        Some(super::advisory::report(
            context,
            &checked,
            &super::pins::visible(&registry, &name),
        )?)
    } else {
        None
    };
    let text = format!(
        "{}Attached {} to {name}\n{}",
        if context.flags.dry_run {
            "Would have "
        } else {
            ""
        },
        path.display(),
        report
            .links
            .iter()
            .map(|l| format!("  {:?} {}", l.status, l.path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );
    context.output.result(&serde_json::json!({"project":name,"instance":path,"links":report.links,"advisory":advisory,"dry_run":context.flags.dry_run}), &text).map_err(super::output_error)?;
    conflicts(report.has_conflicts())
}
pub(super) fn conflicts(has: bool) -> Result<(), Error> {
    if has {
        Err(Error::Conflicts {
            summary: "Project-owned paths were left untouched".into(),
            hint: "Move conflicting files yourself, then run skillctl relink.".into(),
        })
    } else {
        Ok(())
    }
}
