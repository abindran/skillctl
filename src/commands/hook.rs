//! `hook add|remove|list`: pin a stored hook globally, link it under
//! `~/.claude/hooks/<name>` and register its events in Claude Code's user
//! settings (which Cursor reads too). Everything is idempotent and the settings
//! file is merged, never replaced.
use crate::{
    artifact::Kind,
    cli::HookCommand,
    context::Context,
    error::Error,
    harness::{self, claude},
    names::SkillName,
    registry::Registry,
    skill::Skill,
    store,
    validate::hook as rules,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Everything `settings.json` needs to know about a stored hook version.
pub(super) struct Declared {
    pub entry: String,
    pub registrations: Vec<claude::Registration>,
}

/// Read `HOOK.md` from a stored (or draft) hook folder.
pub(super) fn declared(context: &Context<'_>, dir: &Path) -> Result<Declared, Error> {
    let hook = Skill::load_kind(context.fs.as_ref(), dir, Kind::Hook)?;
    let entry = rules::entry(&hook)
        .ok_or_else(|| invalid(dir, "HOOK.md declares no entry script"))?
        .to_owned();
    let events = rules::events(&hook).ok_or_else(|| invalid(dir, "HOOK.md declares no events"))?;
    let matchers = rules::matchers(&hook).unwrap_or_default();
    let timeout = rules::timeout(&hook).unwrap_or(rules::DEFAULT_TIMEOUT);
    let registrations = events
        .into_iter()
        .map(|event| claude::Registration {
            matcher: matchers.get(&event).cloned(),
            event,
            timeout,
        })
        .collect();
    Ok(Declared {
        entry,
        registrations,
    })
}

fn invalid(dir: &Path, message: &str) -> Error {
    Error::Validation {
        summary: format!("{}: {message}", dir.display()),
        hint: "Run skillctl validate on the hook and fix the reported findings.".into(),
    }
}

/// Resolve `name[@pin]` to a hook in the store.
fn hook_pin(
    context: &Context<'_>,
    value: &str,
) -> Result<(SkillName, crate::registry::Pin, PathBuf), Error> {
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
    if Kind::of(context.fs.as_ref(), &dir)? != Kind::Hook {
        return Err(super::attach::usage(format!(
            "'{name}' is a skill, not a hook; use skillctl global add {name} or skillctl add {name}"
        )));
    }
    Ok((name, pin, dir))
}

pub fn run(context: &mut Context<'_>, command: HookCommand) -> Result<(), Error> {
    if matches!(command, HookCommand::List) {
        return list(context);
    }
    super::pins::write_locked(context, |context| {
        let mut registry = super::load_registry(context)?;
        let settings_path = harness::claude_settings().expand()?;
        let mut settings = claude::load(context.fs.as_ref(), &settings_path)?;
        let (name, outcomes) = match command {
            HookCommand::Add { hook } => {
                let (name, pin, dir) = hook_pin(context, &hook)?;
                if registry.global.skills.contains_key(&name)
                    || registry
                        .projects
                        .values()
                        .any(|project| project.skills.contains_key(&name))
                {
                    return Err(super::attach::usage(format!(
                        "'{name}' is already pinned as a skill"
                    )));
                }
                let declared = declared(context, &dir)?;
                registry.global.hooks.insert(name.clone(), pin);
                let outcomes = claude::register(
                    &mut settings,
                    &name,
                    &declared.entry,
                    &declared.registrations,
                )?;
                (name, outcomes)
            }
            HookCommand::Remove { hook } => {
                registry.global.hooks.remove(&hook);
                let outcomes = claude::unregister(&mut settings, &hook);
                (hook, outcomes)
            }
            HookCommand::List => unreachable!("handled above"),
        };
        if !context.flags.dry_run {
            registry.save(context.fs.as_ref(), &context.home.registry())?;
        }
        let reports = reconcile(context, &registry)?;
        let changed = outcomes.iter().any(|outcome| outcome.status != "present");
        let backup = if changed {
            Some(claude::save(
                context.fs.as_ref(),
                &settings_path,
                &settings,
            )?)
        } else {
            None
        };
        let mut text = format!(
            "{}{} {name}",
            if context.flags.dry_run {
                "Would update"
            } else {
                "Updated"
            },
            if registry.global.hooks.contains_key(&name) {
                " hook"
            } else {
                "; removed hook"
            }
        );
        for outcome in &outcomes {
            text.push_str(&format!("\n  {}: {}", outcome.event, outcome.status));
        }
        for report in &reports {
            for link in &report.links {
                text.push_str(&format!("\n  {:?} {}", link.status, link.path.display()));
            }
        }
        if let Some(backup) = &backup {
            text.push_str(&format!(
                "\n  settings: {} (previous copy at {})",
                settings_path.display(),
                backup.display()
            ));
        } else {
            text.push_str(&format!(
                "\n  settings: {} unchanged",
                settings_path.display()
            ));
        }
        context
            .output
            .result(
                &serde_json::json!({
                    "hook": name,
                    "hooks": registry.global.hooks,
                    "settings": settings_path,
                    "backup": backup,
                    "registered": outcomes,
                    "reports": reports,
                    "dry_run": context.flags.dry_run,
                }),
                &text,
            )
            .map_err(super::output_error)?;
        super::attach::conflicts(
            reports
                .iter()
                .any(crate::linker::ReconcileReport::has_conflicts),
        )
    })
}

/// Link every pinned hook under `~/.claude/hooks/`, removing links of hooks
/// that are no longer pinned. Shared with `relink --all` and `doctor --fix`.
pub(super) fn reconcile(
    context: &Context<'_>,
    registry: &Registry,
) -> Result<Vec<crate::linker::ReconcileReport>, Error> {
    super::global::reconcile_into(
        context,
        &registry.global.hooks,
        std::slice::from_ref(&harness::hook_link_target()),
    )
}

fn list(context: &mut Context<'_>) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let rows = super::read_locked(context, || {
        let registry = super::load_registry(context)?;
        let settings_path = harness::claude_settings().expand()?;
        let settings = claude::load(context.fs.as_ref(), &settings_path)?;
        let target = harness::hook_link_target().expand()?;
        let mut rows = Vec::new();
        for (name, pin) in &registry.global.hooks {
            let want = store::store_dir(&context.home, name, pin);
            let path = target.join(name.as_str());
            let link = super::status::health(context, &path, &want)?;
            let declared: Vec<String> = declared(context, &want)
                .map(|d| d.registrations.into_iter().map(|r| r.event).collect())
                .unwrap_or_default();
            let registered = claude::registered_events(&settings, name);
            let missing: Vec<&String> = declared
                .iter()
                .filter(|event| !registered.contains(*event))
                .collect();
            rows.push(serde_json::json!({
                "name": name,
                "pin": pin,
                "link": {"path": path, "status": link},
                "events": declared,
                "registered": registered,
                "missing": missing,
            }));
        }
        Ok(rows)
    })?;
    let text = if rows.is_empty() {
        "No hooks.".to_owned()
    } else {
        rows.iter()
            .map(|row| {
                let missing = row["missing"].as_array().map(Vec::len).unwrap_or(0);
                format!(
                    "{}@{}\n  {}: {}\n  events: {}{}",
                    row["name"].as_str().unwrap_or(""),
                    row["pin"].as_str().unwrap_or(""),
                    row["link"]["path"].as_str().unwrap_or(""),
                    row["link"]["status"].as_str().unwrap_or(""),
                    row["events"]
                        .as_array()
                        .map(|events| {
                            events
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default(),
                    if missing == 0 {
                        " (all registered)".to_owned()
                    } else {
                        format!(" ({missing} not registered; run skillctl hook add)")
                    }
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
