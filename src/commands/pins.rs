//! Project pin changes and convergence share one write-locked workflow.
use super::attach;
use crate::{
    context::Context,
    error::Error,
    linker::ReconcileReport,
    names::{ProjectName, SkillName},
    registry::{Pin, Registry, lock::RegistryLock, resolve::InstanceResolver},
};
use std::collections::BTreeMap;

pub(super) fn selected(
    registry: &Registry,
    named: Option<&ProjectName>,
) -> Result<ProjectName, Error> {
    if let Some(name) = named {
        if !registry.projects.contains_key(name) {
            return Err(attach::usage(format!("Unknown project '{name}'")));
        }
        return Ok(name.clone());
    }
    InstanceResolver::new(registry)?
        .resolve(&attach::cwd()?)?
        .map(|r| r.project_name.clone())
        .ok_or_else(|| attach::usage("Not attached; run skillctl attach or pass --project"))
}
pub(super) fn write_locked<T>(
    context: &mut Context<'_>,
    operation: impl FnOnce(&mut Context<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    super::init::ensure_home(context)?;
    if context.flags.dry_run {
        operation(context)
    } else {
        RegistryLock::open(&context.home.registry_lock())?.with_exclusive(|| operation(context))
    }
}
pub(super) fn reconcile_projects(
    context: &Context<'_>,
    registry: &mut Registry,
    names: &[ProjectName],
) -> Result<Vec<ReconcileReport>, Error> {
    let mut reports = Vec::new();
    for name in names {
        for index in 0..registry.projects[name].instances.len() {
            reports.push(attach::reconcile_instance(context, registry, name, index)?);
        }
    }
    Ok(reports)
}
fn finish(
    context: &mut Context<'_>,
    reports: &[ReconcileReport],
    action: &str,
    advisory: Option<serde_json::Value>,
) -> Result<(), Error> {
    let mut text = action.to_owned();
    for report in reports {
        for link in &report.links {
            text.push_str(&format!("\n  {:?} {}", link.status, link.path.display()));
        }
    }
    context.output.result(&serde_json::json!({"action":action,"instances":reports,"advisory":advisory,"dry_run":context.flags.dry_run}), &text).map_err(super::output_error)?;
    attach::conflicts(reports.iter().any(ReconcileReport::has_conflicts))
}

/// Everything an agent in this project sees: its pins plus the global skills.
pub(super) fn visible(registry: &Registry, project: &ProjectName) -> BTreeMap<SkillName, Pin> {
    let mut visible = registry.projects[project].skills.clone();
    visible.extend(registry.global.skills.clone());
    visible
}
pub fn add(
    context: &mut Context<'_>,
    skill: &str,
    project: Option<&ProjectName>,
    draft: bool,
) -> Result<(), Error> {
    write_locked(context, |context| {
        let mut registry = super::load_registry(context)?;
        let name = selected(&registry, project)?;
        let value = if draft {
            if skill.contains('@') {
                return Err(attach::usage("--draft cannot be combined with @version"));
            }
            format!("{skill}@draft")
        } else {
            skill.to_owned()
        };
        let (skill, pin) = attach::skill_pin(context, &value)?;
        if registry.global.skills.contains_key(&skill) {
            return Err(attach::usage(format!("Skill '{skill}' is pinned globally")));
        }
        registry
            .projects
            .get_mut(&name)
            .expect("selected")
            .skills
            .insert(skill.clone(), pin.clone());
        if !context.flags.dry_run {
            registry.save(context.fs.as_ref(), &context.home.registry())?;
        }
        let reports = reconcile_projects(context, &mut registry, std::slice::from_ref(&name))?;
        let checked = BTreeMap::from([(skill, pin)]);
        let advisory = super::advisory::report(context, &checked, &visible(&registry, &name))?;
        finish(context, &reports, "Updated project pins", Some(advisory))
    })
}
pub fn remove(
    context: &mut Context<'_>,
    skill: &SkillName,
    project: Option<&ProjectName>,
) -> Result<(), Error> {
    write_locked(context, |context| {
        let mut registry = super::load_registry(context)?;
        let name = selected(&registry, project)?;
        registry
            .projects
            .get_mut(&name)
            .expect("selected")
            .skills
            .remove(skill);
        if !context.flags.dry_run {
            registry.save(context.fs.as_ref(), &context.home.registry())?;
        }
        let reports = reconcile_projects(context, &mut registry, &[name])?;
        finish(context, &reports, "Removed project pin", None)
    })
}
/// `relink --all`: globals first, then every instance. Obsolete-target history is
/// cleared only after a conflict-free pass. Shared with doctor --fix and config.
pub(super) fn reconcile_everything(
    context: &Context<'_>,
    registry: &mut Registry,
) -> Result<Vec<ReconcileReport>, Error> {
    let mut reports = super::global::reconcile(context, registry)?;
    reports.extend(super::hook::reconcile(context, registry)?);
    let names = registry.projects.keys().cloned().collect::<Vec<_>>();
    reports.extend(reconcile_projects(context, registry, &names)?);
    if !reports.iter().any(ReconcileReport::has_conflicts) {
        prune_retired_dirs(context, registry);
        registry.settings.previous_link_targets.clear();
        if !context.flags.dry_run {
            registry.save(context.fs.as_ref(), &context.home.registry())?;
        }
    }
    Ok(reports)
}
/// After a dropped link target's links are gone, remove the folders skillctl
/// created for it (the target and its ancestors, deepest first) when they are
/// empty, and forget them. A folder that a current target lives in, or that
/// holds anything else, is kept.
fn prune_retired_dirs(context: &Context<'_>, registry: &mut Registry) {
    let retired = registry.settings.previous_link_targets.clone();
    if retired.is_empty() || context.flags.dry_run {
        return;
    }
    let current = registry.settings.link_targets.clone();
    for project in registry.projects.values_mut() {
        for instance in &mut project.instances {
            let mut candidates: Vec<crate::paths::RelPath> = instance
                .created_dirs
                .iter()
                .filter(|dir| {
                    let dir = dir.as_path();
                    retired
                        .iter()
                        .any(|target| target.as_path().starts_with(dir))
                        && !current
                            .iter()
                            .any(|target| target.as_path().starts_with(dir))
                })
                .cloned()
                .collect();
            candidates.sort_by_key(|dir| std::cmp::Reverse(dir.as_path().components().count()));
            for dir in candidates {
                let path = instance.path.join(dir.as_path());
                let is_dir = context
                    .fs
                    .symlink_metadata(&path)
                    .is_ok_and(|info| info.is_dir());
                // remove_dir only succeeds on an empty folder.
                if !is_dir || context.fs.remove_dir(&path).is_ok() {
                    instance.created_dirs.retain(|kept| kept != &dir);
                }
            }
        }
    }
}

pub fn relink(
    context: &mut Context<'_>,
    all: bool,
    project: Option<&ProjectName>,
) -> Result<(), Error> {
    write_locked(context, |context| {
        let mut registry = super::load_registry(context)?;
        let reports = if all {
            reconcile_everything(context, &mut registry)?
        } else if let Some(project) = project {
            let name = selected(&registry, Some(project))?;
            reconcile_projects(context, &mut registry, &[name])?
        } else {
            let resolver = InstanceResolver::new(&registry)?;
            let found = resolver
                .resolve(&attach::cwd()?)?
                .ok_or_else(|| attach::usage("Not attached; run skillctl attach first"))?;
            let name = found.project_name.clone();
            let index = found
                .project
                .instances
                .iter()
                .position(|i| i.path == found.instance.path)
                .expect("resolved");
            vec![attach::reconcile_instance(
                context,
                &mut registry,
                &name,
                index,
            )?]
        };
        finish(context, &reports, "Reconciled links", None)
    })
}
