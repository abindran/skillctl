//! `uninstall <name>[@<version>]`: delete one store version, or every version
//! of an artifact, provided nothing pins it. Pinned versions are refused with
//! the pins named, because deleting them would leave dangling links; unpin
//! first (`remove`, `global remove`, `hook remove`). Drafts are untouched.
use crate::{
    context::Context,
    error::Error,
    names::SkillName,
    registry::{Pin, Registry},
    store::{self, drafts::remove_tree, stage::restore_write_mode},
};
use semver::Version;

/// Who pins `name@version`: project names, `global`, `hook`.
fn pinners(registry: &Registry, name: &SkillName, version: &Version) -> Vec<String> {
    let pin = Pin::Version(version.clone());
    let mut by: Vec<String> = registry
        .projects
        .iter()
        .filter(|(_, project)| project.skills.get(name) == Some(&pin))
        .map(|(project, _)| project.to_string())
        .collect();
    if registry.global.skills.get(name) == Some(&pin) {
        by.push("global".into());
    }
    if registry.global.hooks.get(name) == Some(&pin) {
        by.push("hook".into());
    }
    by
}

fn parse_target(target: &str) -> Result<(SkillName, Option<Version>), Error> {
    let (name, version) = match target.split_once('@') {
        Some((name, version)) => (name, Some(version)),
        None => (target, None),
    };
    let name: SkillName = name.parse().map_err(|_| Error::Usage {
        message: format!("{name:?} is not an artifact name"),
        hint: "Use lowercase letters, digits and hyphens, optionally followed by @<version>."
            .into(),
    })?;
    let version = match version {
        Some(text) => Some(text.parse::<Version>().map_err(|_| Error::Usage {
            message: format!("{text:?} is not a semantic version"),
            hint: "Write the version as in `skillctl artifacts`, for example 1.2.0.".into(),
        })?),
        None => None,
    };
    Ok((name, version))
}

pub fn run(context: &mut Context<'_>, target: &str) -> Result<(), Error> {
    let (name, wanted) = parse_target(target)?;
    super::pins::write_locked(context, |context| {
        let fs = context.fs.as_ref();
        let registry = super::load_registry(context)?;
        let stored = store::list_versions(fs, &context.home, &name)?;
        if stored.is_empty() {
            return Err(Error::Usage {
                message: format!("No stored version of {name}"),
                hint: "Run skillctl artifacts to see what is in the store.".into(),
            });
        }
        let versions: Vec<Version> = match &wanted {
            Some(version) if stored.contains(version) => vec![version.clone()],
            Some(version) => {
                return Err(Error::Usage {
                    message: format!(
                        "{name}@{version} is not in the store (stored: {})",
                        stored
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    hint: "Run skillctl artifacts to see the stored versions.".into(),
                });
            }
            None => stored,
        };
        let pinned: Vec<String> = versions
            .iter()
            .filter_map(|version| {
                let by = pinners(&registry, &name, version);
                (!by.is_empty()).then(|| format!("{name}@{version} is pinned by {}", by.join(", ")))
            })
            .collect();
        if !pinned.is_empty() {
            return Err(Error::Conflicts {
                summary: pinned.join("; "),
                hint: format!(
                    "Unpin first: skillctl remove {name} --project <project>, skillctl global remove {name}, or skillctl hook remove {name}; then uninstall again."
                ),
            });
        }
        let listing = versions
            .iter()
            .map(|version| format!("  {name}@{version}"))
            .collect::<Vec<_>>()
            .join("\n");
        if !context.flags.dry_run {
            context.prompt.set_answer_flag("--yes");
            let question = format!(
                "Delete {} version(s) of {name} from the store?\n{listing}\nProceed",
                versions.len()
            );
            if !context.prompt.confirm(&question, false)? {
                return context
                    .output
                    .result(
                        &serde_json::json!({"deleted": [], "cancelled": true, "dry_run": false}),
                        "Cancelled; nothing deleted.",
                    )
                    .map_err(super::output_error);
            }
        }
        let mut deleted = Vec::new();
        for version in &versions {
            let dir = context.home.version_dir(&name, version);
            if !context.flags.dry_run {
                restore_write_mode(fs, &dir)?;
                remove_tree(fs, &dir)?;
            }
            deleted.push(serde_json::json!({"skill": name, "version": version, "path": dir}));
        }
        if !context.flags.dry_run {
            let parent = context.home.skill_versions(&name);
            let remaining = fs.read_dir(&parent).map_err(|source| Error::Io {
                operation: format!("List {}", parent.display()),
                source,
                hint: "Check store permissions.".into(),
            })?;
            if remaining.is_empty() {
                fs.remove_dir(&parent).map_err(|source| Error::Io {
                    operation: format!("Remove {}", parent.display()),
                    source,
                    hint: "Check store permissions.".into(),
                })?;
            }
        }
        let text = format!(
            "{}{listing}",
            if context.flags.dry_run {
                "Would delete:\n"
            } else {
                "Deleted:\n"
            }
        );
        context
            .output
            .result(
                &serde_json::json!({"deleted": deleted, "dry_run": context.flags.dry_run}),
                &text,
            )
            .map_err(super::output_error)
    })
}
