use crate::{
    artifact::Kind,
    context::Context,
    error::Error,
    names::{ProjectName, SkillName},
    registry::Pin,
    store::{self, meta::Metadata},
};
use semver::Version;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SkillListing {
    pub name: SkillName,
    /// From the newest version's `meta.json`; a skill when it cannot be read.
    pub kind: Kind,
    pub versions: Vec<VersionUsage>,
}
#[derive(Debug, Serialize)]
pub struct VersionUsage {
    pub version: Version,
    pub projects: Vec<ProjectName>,
    pub global: bool,
}

pub fn run(context: &mut Context<'_>) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let rows = super::read_locked(context, || {
        let registry = super::load_registry(context)?;
        let mut rows = Vec::new();
        for name in store::list_skills(context.fs.as_ref(), &context.home)? {
            let mut versions = Vec::new();
            let listed = store::list_versions(context.fs.as_ref(), &context.home, &name)?;
            let kind = listed
                .last()
                .and_then(|version| {
                    Metadata::load(
                        context.fs.as_ref(),
                        &context.home.version_dir(&name, version).join("meta.json"),
                    )
                    .ok()
                })
                .map(|meta| meta.kind)
                .unwrap_or_default();
            for version in listed {
                let pin = Pin::Version(version.clone());
                let projects = registry
                    .projects
                    .iter()
                    .filter(|(_, project)| project.skills.get(&name) == Some(&pin))
                    .map(|(name, _)| name.clone())
                    .collect();
                let global = registry.global.skills.get(&name) == Some(&pin)
                    || registry.global.hooks.get(&name) == Some(&pin);
                versions.push(VersionUsage {
                    version,
                    projects,
                    global,
                });
            }
            rows.push(SkillListing {
                name,
                kind,
                versions,
            });
        }
        Ok(rows)
    })?;
    context
        .output
        .result(&rows, &text(&rows))
        .map_err(super::output_error)
}

pub fn text(rows: &[SkillListing]) -> String {
    if rows.is_empty() {
        return "No installed skills.".into();
    }
    rows.iter()
        .map(|skill| {
            let mut text = skill.name.to_string();
            if skill.kind != Kind::Skill {
                text.push_str(&format!(" ({})", skill.kind));
            }
            if skill.versions.is_empty() {
                text.push_str(" (no versions)");
            }
            for version in &skill.versions {
                let mut usage: Vec<_> = version.projects.iter().map(ToString::to_string).collect();
                if version.global {
                    usage.push("global".into());
                }
                text.push_str(&format!(
                    "\n  {} — {}",
                    version.version,
                    if usage.is_empty() {
                        "unused".into()
                    } else {
                        usage.join(", ")
                    }
                ));
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n")
}
