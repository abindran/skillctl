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

fn target(folder: Option<&Path>) -> Result<std::path::PathBuf, Error> {
    match folder {
        Some(folder) => crate::paths::canonicalize(folder),
        None => crate::paths::canonicalize(&super::attach::cwd()?),
    }
}

/// `lint --files`: the instruction files lint reads in this folder, so a
/// clean lint can be told apart from a folder with nothing to lint.
pub fn files(context: &mut Context<'_>, folder: Option<&Path>) -> Result<(), Error> {
    let folder = target(folder)?;
    let repo_root = context.git.toplevel(&folder)?;
    let files = instructions::inventory(context.fs.as_ref(), &folder, repo_root.as_deref())?;
    let present = files.iter().filter(|file| file.present).count();
    let mut text = vec![format!(
        "{present} of the instruction files lint reads exist in {}",
        folder.display()
    )];
    for file in &files {
        text.push(format!(
            "  {:<8} {:<34} {:>10}  {}  [{}]",
            if file.present { "present" } else { "absent" },
            file.name,
            file.lines
                .map(|lines| format!("{lines} lines"))
                .unwrap_or_default(),
            file.loads,
            file.rules.join(" ")
        ));
    }
    if present == 0 {
        text.push(
            "Nothing to lint here: no AGENTS.md, CLAUDE.md, CLAUDE.local.md or rule files.".into(),
        );
    }
    context
        .output
        .result(&files, &text.join("\n"))
        .map_err(super::output_error)
}

pub fn run(
    context: &mut Context<'_>,
    folder: Option<&Path>,
    format: Option<OutputFormat>,
    strict: bool,
) -> Result<(), Error> {
    let folder = target(folder)?;
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
