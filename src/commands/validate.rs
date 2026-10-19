//! Read-only validation with one complete report, including when exiting with code 3.
use crate::{
    cli::OutputFormat,
    context::Context,
    error::Error,
    names::SkillName,
    registry::Pin,
    store,
    validate::{self, Severity, report::SkillReport},
};
use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

pub fn run(
    context: &mut Context<'_>,
    target: Option<&str>,
    format: Option<OutputFormat>,
    strict: bool,
    with_reference: bool,
) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let reports = super::read_locked(context, || {
        let registry = super::load_registry_or_default(context)?;
        let mut reports = Vec::new();
        for dir in targets(context, target)? {
            let mut findings = validate::run_all(
                context.fs.as_ref(),
                &dir,
                &registry.settings.validate.allow,
                strict,
            )?;
            // skills-ref knows only the Agent Skills spec; hooks are skipped.
            if with_reference
                && crate::artifact::Kind::detect(context.fs.as_ref(), &dir)?
                    != Some(crate::artifact::Kind::Hook)
            {
                findings.extend(reference(&dir)?);
            }
            reports.push(SkillReport { dir, findings });
        }
        Ok(reports)
    })?;
    let findings: Vec<_> = reports
        .iter()
        .flat_map(|report| report.findings.iter().cloned())
        .collect();
    let format = format.unwrap_or(if context.flags.json {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    });
    let document = match format {
        OutputFormat::Text => validate::report::text(&findings),
        OutputFormat::Json => validate::report::json(&findings).map_err(super::output_error)?,
        OutputFormat::Junit => validate::report::junit(&reports),
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

fn targets(context: &Context<'_>, target: Option<&str>) -> Result<Vec<PathBuf>, Error> {
    if let Some(target) = target {
        let path = Path::new(target);
        if path.is_absolute()
            || target.starts_with('.')
            || target.contains('/')
            || context.fs.symlink_metadata(path).is_ok()
        {
            return Ok(vec![crate::paths::canonicalize(path)?]);
        }
        let (name, pin) = match target.split_once('@') {
            Some((name, pin)) => (name, Some(pin.parse::<Pin>()?)),
            None => (target, None),
        };
        let name = SkillName::new(name)?;
        let pin = store::resolve_pin(context.fs.as_ref(), &context.home, &name, pin.as_ref())?;
        return Ok(vec![store::store_dir(&context.home, &name, &pin)]);
    }
    let mut dirs = Vec::new();
    match context.fs.read_dir(&context.home.drafts()) {
        Ok(entries) => {
            for path in entries {
                if context
                    .fs
                    .symlink_metadata(&path)
                    .map_err(|source| validate_io(&path, source))?
                    .is_dir()
                {
                    dirs.push(path);
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(validate_io(&context.home.drafts(), error)),
    }
    for name in store::list_skills(context.fs.as_ref(), &context.home)? {
        for version in store::list_versions(context.fs.as_ref(), &context.home, &name)? {
            dirs.push(context.home.store_dir(&name, &version));
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn validate_io(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Validate {}", path.display()),
        source,
        hint: "Check the target path and read permissions.".into(),
    }
}

/// The optional reference tool has no stable structured-output contract. Preserve
/// a failed verdict as R001 without copying its output (which may contain secrets).
fn reference(dir: &Path) -> Result<Vec<validate::Finding>, Error> {
    let output = match Command::new("skills-ref").arg("validate").arg(dir).output() {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(validate_io(dir, error)),
    };
    if output.status.success() {
        return Ok(Vec::new());
    }
    Ok(vec![validate::Finding::error(
        "R001",
        dir.join("SKILL.md"),
        "skills-ref validation failed",
        "Run skills-ref validate on this folder to inspect its diagnostics.",
    )])
}
