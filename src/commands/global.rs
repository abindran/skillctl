use crate::{
    cli::GlobalCommand,
    context::Context,
    error::Error,
    linker::{self, ReconcileReport},
    paths::RelPath,
    registry::{Instance, Project, Registry, Settings},
    store,
};
use std::path::PathBuf;

/// Reuse the ownership/convergence engine for user-level folders without Git.
/// Resolve the target directory itself when it is a symlink; links remain per-skill.
pub(super) fn reconcile(
    context: &Context<'_>,
    registry: &Registry,
) -> Result<Vec<ReconcileReport>, Error> {
    let project = Project {
        skills: registry.global.skills.clone(),
        instances: vec![],
    };
    let mut reports = Vec::new();
    for target in &registry.settings.global_link_targets {
        let target = target.expand()?;
        let target = if std::fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink())
        {
            crate::paths::canonicalize(&target)?
        } else {
            target
        };
        let mut root = target.as_path();
        while !root.is_dir() {
            root = root
                .parent()
                .ok_or_else(|| super::attach::usage("Global target has no existing parent"))?;
        }
        // Ensure a nonempty relative target even when the target already exists.
        let root = if root == target {
            root.parent()
                .ok_or_else(|| super::attach::usage("Global target cannot be /"))?
        } else {
            root
        };
        let relative = RelPath::new(target.strip_prefix(root).expect("ancestor").to_path_buf())?;
        let mut instance = Instance {
            path: root.to_path_buf(),
            attached_at: super::attach::now(),
            created_dirs: vec![],
        };
        let settings = Settings {
            link_targets: vec![relative],
            git_exclude: false,
            previous_link_targets: vec![],
            ..Settings::default()
        };
        reports.push(linker::reconcile(
            context.fs.as_ref(),
            &context.home,
            &mut instance,
            &project,
            &settings,
        )?);
    }
    Ok(reports)
}

pub fn run(context: &mut Context<'_>, command: GlobalCommand) -> Result<(), Error> {
    if matches!(command, GlobalCommand::List) {
        return list(context);
    }
    super::pins::write_locked(context, |context| {
        let mut registry = super::load_registry(context)?;
        match command {
            GlobalCommand::Add { skill } => {
                let (name, pin) = super::attach::skill_pin(context, &skill)?;
                if registry
                    .projects
                    .values()
                    .any(|p| p.skills.contains_key(&name))
                {
                    return Err(super::attach::usage(format!(
                        "Skill '{name}' is pinned by a project"
                    )));
                }
                registry.global.skills.insert(name, pin);
            }
            GlobalCommand::Remove { skill } => {
                registry.global.skills.remove(&skill);
            }
            GlobalCommand::List => return list(context),
        }
        if !context.flags.dry_run {
            registry.save(context.fs.as_ref(), &context.home.registry())?;
        }
        let mut reports = reconcile(context, &registry)?;
        if registry.settings.global_in_instances {
            let names = registry.projects.keys().cloned().collect::<Vec<_>>();
            reports.extend(super::pins::reconcile_projects(
                context,
                &mut registry,
                &names,
            )?);
        }
        context.output.result(&serde_json::json!({"skills":registry.global.skills,"reports":reports,"dry_run":context.flags.dry_run}),"Updated global skills").map_err(super::output_error)?;
        super::attach::conflicts(reports.iter().any(ReconcileReport::has_conflicts))
    })
}
fn list(context: &mut Context<'_>) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let rows = super::read_locked(context, || {
        let registry = super::load_registry(context)?;
        let mut rows = Vec::new();
        for (name, pin) in &registry.global.skills {
            let want = store::store_dir(&context.home, name, pin);
            let mut links = Vec::new();
            for target in &registry.settings.global_link_targets {
                let path: PathBuf = target.expand()?.join(name.as_str());
                links.push(serde_json::json!({"path":path,"status":super::status::health(context,&path,&want)?}));
            }
            rows.push(serde_json::json!({"name":name,"pin":pin,"links":links}));
        }
        Ok(rows)
    })?;
    let text = if rows.is_empty() {
        "No global skills.".into()
    } else {
        rows.iter()
            .map(|r| {
                format!(
                    "{}@{}\n  {}",
                    r["name"].as_str().unwrap_or(""),
                    r["pin"].as_str().unwrap_or(""),
                    r["links"]
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    context
        .output
        .result(&rows, &text)
        .map_err(super::output_error)
}
