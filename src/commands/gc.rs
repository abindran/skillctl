//! Remove store versions that nothing pins and that are not among a skill's
//! newest `keep` versions. Lists first, asks, then deletes under the exclusive lock.
use crate::{
    context::Context,
    error::Error,
    names::SkillName,
    registry::Pin,
    store::{self, drafts::remove_tree, stage::restore_write_mode},
};
use semver::Version;
use std::collections::BTreeSet;

pub fn run(context: &mut Context<'_>, keep: u32) -> Result<(), Error> {
    super::pins::write_locked(context, |context| {
        let fs = context.fs.as_ref();
        let registry = super::load_registry(context)?;
        let pinned: BTreeSet<(SkillName, Version)> = registry
            .projects
            .values()
            .flat_map(|project| project.skills.iter())
            .chain(registry.global.skills.iter())
            .chain(registry.global.hooks.iter())
            .filter_map(|(name, pin)| match pin {
                Pin::Version(version) => Some((name.clone(), version.clone())),
                Pin::Draft => None,
            })
            .collect();
        let mut candidates = Vec::new();
        for skill in store::list_skills(fs, &context.home)? {
            let versions = store::list_versions(fs, &context.home, &skill)?;
            let newest_start = versions.len().saturating_sub(keep as usize);
            for version in versions.into_iter().take(newest_start) {
                if !pinned.contains(&(skill.clone(), version.clone())) {
                    candidates.push((skill.clone(), version));
                }
            }
        }
        if candidates.is_empty() {
            return context
                .output
                .result(
                    &serde_json::json!({"deleted": [], "dry_run": context.flags.dry_run}),
                    "Nothing to delete.",
                )
                .map_err(super::output_error);
        }
        let listing = candidates
            .iter()
            .map(|(skill, version)| format!("  {skill}@{version}"))
            .collect::<Vec<_>>()
            .join("\n");
        if !context.flags.dry_run {
            context.prompt.set_answer_flag("--yes");
            let question = format!(
                "Delete {} unused version(s)?\n{listing}\nProceed",
                candidates.len()
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
        for (skill, version) in &candidates {
            let dir = context.home.version_dir(skill, version);
            restore_write_mode(fs, &dir)?;
            remove_tree(fs, &dir)?;
            deleted.push(serde_json::json!({"skill": skill, "version": version, "path": dir}));
            let parent = context.home.skill_versions(skill);
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
            "{}{}",
            if context.flags.dry_run {
                "Would delete:\n"
            } else {
                "Deleted:\n"
            },
            listing
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
