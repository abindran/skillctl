//! `lint [<folder>]`: read-only checks on a project's instruction files
//! (`AGENTS.md`, `CLAUDE.md`, `CLAUDE.local.md`). Reports with the same
//! formats and exit codes as `validate`; never writes, because those files are
//! owned by the team and changed only through review.
use crate::{
    cli::OutputFormat,
    context::Context,
    error::Error,
    validate::{self, Severity, instructions, report::SkillReport},
};
use std::path::Path;

pub fn run(
    context: &mut Context<'_>,
    folder: Option<&Path>,
    format: Option<OutputFormat>,
    strict: bool,
) -> Result<(), Error> {
    let folder = match folder {
        Some(folder) => crate::paths::canonicalize(folder)?,
        None => crate::paths::canonicalize(&super::attach::cwd()?)?,
    };
    let registry = super::load_registry_or_default(context)?;
    let allow = &registry.settings.validate.allow;
    let repo_root = context.git.toplevel(&folder)?;
    // Script bundles pinned in this folder's project, for I306.
    let scripts: Vec<String> =
        match crate::registry::resolve::InstanceResolver::new(&registry)?.resolve(&folder)? {
            Some(found) => {
                let mut names = Vec::new();
                for (name, pin) in &found.project.skills {
                    let dir = crate::store::store_dir(&context.home, name, pin);
                    if crate::artifact::Kind::detect(context.fs.as_ref(), &dir)?
                        == Some(crate::artifact::Kind::Script)
                    {
                        names.push(name.to_string());
                    }
                }
                names
            }
            None => Vec::new(),
        };
    let mut findings =
        instructions::check(context.fs.as_ref(), &folder, repo_root.as_deref(), &scripts)?;
    findings.retain(|finding| !allow.iter().any(|id| id == &finding.rule));
    if strict {
        for finding in &mut findings {
            finding.severity = Severity::Error;
        }
    }
    let format = format.unwrap_or(if context.flags.json {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    });
    let document = match format {
        OutputFormat::Text => validate::report::text(&findings),
        OutputFormat::Json => validate::report::json(&findings).map_err(super::output_error)?,
        OutputFormat::Junit => validate::report::junit(&[SkillReport {
            dir: folder.clone(),
            findings: findings.clone(),
        }]),
    };
    context
        .output
        .document(&document, !matches!(format, OutputFormat::Text))
        .map_err(super::output_error)?;
    if findings
        .iter()
        .any(|finding| finding.severity == Severity::Error)
    {
        Err(Error::ReportedValidation)
    } else {
        Ok(())
    }
}
