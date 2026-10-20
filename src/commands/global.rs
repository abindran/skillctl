use crate::{
    artifact::Kind,
    cli::GlobalCommand,
    context::Context,
    error::Error,
    linker::{self, ReconcileReport},
    names::SkillName,
    paths::{HomePath, Paths, RelPath},
    registry::{Instance, Pin, Project, Registry, Settings},
    services::fs::Fs,
    store,
};
use std::{collections::BTreeMap, path::PathBuf};

/// Kinds that can be pinned globally: skills (linked into
/// `global_link_targets`), and commands and agents (linked into the
/// user-level folders both harnesses read: `~/.claude/commands`,
/// `~/.cursor/commands`, `~/.claude/agents`, `~/.cursor/agents`). Rules and
/// scripts are project-level; hooks are registered with `hook add`.
pub fn global_kind(kind: Kind) -> bool {
    matches!(kind, Kind::Skill | Kind::Command | Kind::Agent)
}

/// A set of pins by artifact name.
pub type Pins = BTreeMap<SkillName, Pin>;

/// Split global pins into skills and the commands and agents beside them. A
/// pin whose kind cannot be read stays with the skills, as in the linker.
pub fn split(fs: &dyn Fs, home: &Paths, pins: &Pins) -> Result<(Pins, Pins), Error> {
    let mut skills = BTreeMap::new();
    let mut others = BTreeMap::new();
    for (name, pin) in pins {
        let kind = Kind::detect(fs, &store::store_dir(home, name, pin))?.unwrap_or(Kind::Skill);
        if kind == Kind::Skill {
            skills.insert(name.clone(), pin.clone());
        } else {
            others.insert(name.clone(), pin.clone());
        }
    }
    Ok((skills, others))
}

/// The user home as the root the user-level kind folders live under.
pub fn user_root() -> Result<PathBuf, Error> {
    crate::paths::canonicalize(&crate::paths::user_home_dir()?)
}

/// The links the global commands and agents need, rooted at the user home.
pub fn user_level_plans(
    fs: &dyn Fs,
    home: &Paths,
    pins: &BTreeMap<SkillName, Pin>,
) -> Result<Vec<linker::LinkPlan>, Error> {
    linker::plans(fs, home, pins, &[], linker::Layout::Kinds)
}

/// Reuse the ownership/convergence engine for user-level folders without Git.
/// Resolve the target directory itself when it is a symlink; links remain per-skill.
pub(super) fn reconcile(
    context: &Context<'_>,
    registry: &Registry,
) -> Result<Vec<ReconcileReport>, Error> {
    let (skills, others) = split(context.fs.as_ref(), &context.home, &registry.global.skills)?;
    let mut reports = reconcile_into(context, &skills, &registry.settings.global_link_targets)?;
    reports.push(reconcile_user_level(context, &others)?);
    Ok(reports)
}

/// Link global commands and agents under the user home with the project
/// layout, so `~/.claude/commands/<name>.md` and friends appear exactly as
/// they would inside a project. Always run, even with no such pins, so the
/// links of one removed from the global set are swept; only managed links
/// (symlinks into the store or drafts) are ever touched.
fn reconcile_user_level(
    context: &Context<'_>,
    pins: &BTreeMap<SkillName, Pin>,
) -> Result<ReconcileReport, Error> {
    let project = Project {
        skills: pins.clone(),
        instances: vec![],
    };
    let mut instance = Instance {
        path: user_root()?,
        attached_at: super::attach::now(),
        created_dirs: vec![],
    };
    let settings = Settings {
        link_targets: vec![],
        git_exclude: false,
        previous_link_targets: vec![],
        ..Settings::default()
    };
    linker::reconcile(
        context.fs.as_ref(),
        &context.home,
        &mut instance,
        &project,
        &settings,
        linker::Layout::Kinds,
    )
}

/// Link a set of global pins into user-level target folders. Skills go to
/// `global_link_targets`; hooks go to the hook link folder.
pub(super) fn reconcile_into(
    context: &Context<'_>,
    pins: &BTreeMap<SkillName, Pin>,
    targets: &[HomePath],
) -> Result<Vec<ReconcileReport>, Error> {
    let project = Project {
        skills: pins.clone(),
        instances: vec![],
    };
    let mut reports = Vec::new();
    for target in targets {
        let target = target.expand()?;
        // Resolve the deepest existing ancestor so symlinked parents (for example
        // a dotfiles `~/.claude`) still receive per-skill links in the real folder.
        let mut existing = target.as_path();
        while std::fs::symlink_metadata(existing)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            existing = existing
                .parent()
                .ok_or_else(|| super::attach::usage("Global target has no existing parent"))?;
        }
        let target = crate::paths::canonicalize(existing)?
            .join(target.strip_prefix(existing).expect("ancestor"));
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
            linker::Layout::Folders,
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
        let mut added = None;
        match command {
            GlobalCommand::Add { skill } => {
                let (name, pin) = super::attach::skill_pin(context, &skill)?;
                let dir = store::store_dir(&context.home, &name, &pin);
                let kind = Kind::of(context.fs.as_ref(), &dir)?;
                if !global_kind(kind) {
                    return Err(super::attach::usage(match kind {
                        Kind::Hook => {
                            format!("'{name}' is a hook; register it with skillctl hook add {name}")
                        }
                        _ => format!(
                            "'{name}' is a {kind}; {kind}s are project-level, pin them with skillctl add inside the project"
                        ),
                    }));
                }
                if registry
                    .projects
                    .values()
                    .any(|p| p.skills.contains_key(&name))
                {
                    return Err(super::attach::usage(format!(
                        "'{name}' is pinned by a project; unpin it there first (skillctl remove {name} --project <project>)"
                    )));
                }
                registry.global.skills.insert(name.clone(), pin.clone());
                added = Some((name, pin));
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
        // A global skill is in every listing, so size the global set itself.
        let advisory = match &added {
            Some((name, pin)) => Some(super::advisory::report(
                context,
                &BTreeMap::from([(name.clone(), pin.clone())]),
                &registry.global.skills,
            )?),
            None => None,
        };
        context.output.result(&serde_json::json!({"skills":registry.global.skills,"reports":reports,"advisory":advisory,"dry_run":context.flags.dry_run}),"Updated global artifacts").map_err(super::output_error)?;
        super::attach::conflicts(reports.iter().any(ReconcileReport::has_conflicts))
    })
}
fn list(context: &mut Context<'_>) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let rows = super::read_locked(context, || {
        let registry = super::load_registry(context)?;
        let mut rows = Vec::new();
        let root = user_root()?;
        for (name, pin) in &registry.global.skills {
            let want = store::store_dir(&context.home, name, pin);
            let kind = Kind::detect(context.fs.as_ref(), &want)?.unwrap_or(Kind::Skill);
            let mut links = Vec::new();
            if kind == Kind::Skill {
                for target in &registry.settings.global_link_targets {
                    let path: PathBuf = target.expand()?.join(name.as_str());
                    links.push(serde_json::json!({"path":path,"status":super::status::health(context,&path,&want)?}));
                }
            } else {
                let single = BTreeMap::from([(name.clone(), pin.clone())]);
                for plan in user_level_plans(context.fs.as_ref(), &context.home, &single)? {
                    let path = plan.path(&root);
                    links.push(serde_json::json!({"path":path,"status":super::status::health(context,&path,&plan.target)?}));
                }
            }
            rows.push(serde_json::json!({"name":name,"pin":pin,"kind":kind,"links":links}));
        }
        Ok(rows)
    })?;
    let text = if rows.is_empty() {
        "Nothing is global.".into()
    } else {
        let mut text = String::new();
        for (index, r) in rows.iter().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            text.push_str(r["name"].as_str().unwrap_or(""));
            text.push('@');
            text.push_str(r["pin"].as_str().unwrap_or(""));
            for l in r["links"].as_array().into_iter().flatten() {
                text.push_str("\n  ");
                text.push_str(l["path"].as_str().unwrap_or(""));
                text.push_str(": ");
                text.push_str(l["status"].as_str().unwrap_or(""));
            }
        }
        text
    };
    context
        .output
        .result(&rows, &text)
        .map_err(super::output_error)
}
