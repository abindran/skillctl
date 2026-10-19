//! Native validation. Structural failures suppress dependent field checks, not I/O errors.

pub mod fields;
pub mod harness;
pub mod hook;
pub mod instructions;
pub mod org;
pub mod report;
pub mod spec;

use crate::{artifact::Kind, error::Error, services::fs::Fs, skill::Skill};
use serde::Serialize;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Error => "error",
            Self::Warning => "warning",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub path: PathBuf,
    pub line: Option<usize>,
    pub message: String,
    pub hint: String,
}

impl Finding {
    pub fn error(rule: &str, path: PathBuf, message: impl Into<String>, hint: &str) -> Self {
        Self {
            rule: rule.into(),
            severity: Severity::Error,
            path,
            line: None,
            message: message.into(),
            hint: hint.into(),
        }
    }
}

/// Rules inspect a structurally parsed skill and may return multiple findings.
/// The runner handles S001/S002 prerequisites before invoking field-level rules.
pub trait Rule {
    fn check(&self, skill: &Skill) -> Vec<Finding>;

    /// Filesystem-dependent rules use the same injectable read service as commands.
    fn check_fs(&self, _fs: &dyn Fs, skill: &Skill) -> Result<Vec<Finding>, Error> {
        Ok(self.check(skill))
    }
}

pub fn run(fs: &dyn Fs, dir: &Path) -> Result<Vec<Finding>, Error> {
    run_with_rules(fs, dir, &[&spec::SpecRules])
}

/// Rules that make an artifact unloadable or unregistrable are never suppressed
/// by an allow-list: the spec rules for skills, and the structural hook rules.
fn suppressible(rule: &str) -> bool {
    !(rule.starts_with('S') || hook::STRUCTURAL.contains(&rule))
}

/// Run all native groups for the folder's kind, suppress allowed IDs, then
/// promote remaining warnings. `run` remains the spec-only API for callers that
/// need structural validation of a skill.
pub fn run_all(
    fs: &dyn Fs,
    dir: &Path,
    allow: &[String],
    strict: bool,
) -> Result<Vec<Finding>, Error> {
    // A folder with neither document is reported as a skill missing SKILL.md.
    let kind = Kind::detect(fs, dir)?.unwrap_or_default();
    // Never read a linked document outside the artifact boundary.
    let document = dir.join(kind.document());
    if fs
        .symlink_metadata(&document)
        .is_ok_and(|info| info.is_symlink())
    {
        let (rule, what) = match kind {
            Kind::Skill => ("O122", "skill"),
            Kind::Hook => ("K110", "hook"),
        };
        return Ok(vec![Finding::error(
            rule,
            document,
            format!("Symlinks are not allowed inside a {what}"),
            &format!(
                "Replace {} with a regular file inside the {what}.",
                kind.document()
            ),
        )]);
    }
    let rules: &[&dyn Rule] = match kind {
        Kind::Skill => &[&spec::SpecRules, &org::OrgRules, &harness::HarnessRules],
        Kind::Hook => &[&hook::HookRules],
    };
    let mut findings = run_with_rules_kind(fs, dir, kind, rules)?;
    let local = if findings
        .iter()
        .any(|finding| matches!(finding.rule.as_str(), "S001" | "S002" | "K101"))
    {
        String::new()
    } else {
        Skill::load_kind(fs, dir, kind)?
            .frontmatter
            .metadata
            .value()
            .and_then(|metadata| metadata.get("skillctl-allow"))
            .cloned()
            .unwrap_or_default()
    };
    findings.retain(|finding| {
        !suppressible(&finding.rule)
            || !(allow.iter().any(|id| id == &finding.rule)
                || local.split(',').any(|id| id.trim() == finding.rule))
    });
    if strict {
        for finding in &mut findings {
            finding.severity = Severity::Error;
        }
    }
    Ok(findings)
}

pub fn run_with_rules(fs: &dyn Fs, dir: &Path, rules: &[&dyn Rule]) -> Result<Vec<Finding>, Error> {
    run_with_rules_kind(fs, dir, Kind::Skill, rules)
}

/// Gate on the root document (S001/S002 for skills, K101 for hooks), then run
/// every rule on the parsed document.
pub fn run_with_rules_kind(
    fs: &dyn Fs,
    dir: &Path,
    kind: Kind,
    rules: &[&dyn Rule],
) -> Result<Vec<Finding>, Error> {
    let path = dir.join(kind.document());
    let document = kind.document();
    let (missing_rule, parse_rule, what) = match kind {
        Kind::Skill => ("S001", "S002", "skill"),
        Kind::Hook => ("K101", "K101", "hook"),
    };
    match fs.symlink_metadata(&path) {
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(vec![Finding::error(
                missing_rule,
                path,
                format!("{document} is missing from the {what} root"),
                &format!("Create {document} at the {what} folder root."),
            )]);
        }
        Err(source) => return Err(io_error(&path, source)),
        Ok(info) if !info.is_file() && !info.is_symlink() => {
            return Ok(vec![Finding::error(
                missing_rule,
                path,
                format!("{document} must be a file"),
                &format!("Replace {document} with a readable Markdown file."),
            )]);
        }
        Ok(_) => {}
    }
    let bytes = fs.read(&path).map_err(|source| io_error(&path, source))?;
    let parsed = std::str::from_utf8(&bytes)
        .map_err(|error| error.to_string())
        .and_then(|text| Skill::parse_kind(dir, text, kind).map_err(|error| error.to_string()));
    let skill = match parsed {
        Ok(skill) => skill,
        Err(message) => {
            return Ok(vec![Finding::error(
                parse_rule,
                path,
                message,
                &format!("Start {document} with YAML mapping frontmatter between exact --- lines."),
            )]);
        }
    };
    let mut findings = Vec::new();
    for rule in rules {
        findings.extend(rule.check_fs(fs, &skill)?);
    }
    Ok(findings)
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Validate {}", path.display()),
        source,
        hint: "Check the folder and its read permissions.".into(),
    }
}
