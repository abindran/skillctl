//! Native validation. Structural failures suppress dependent field checks, not I/O errors.

pub mod report;
pub mod spec;

use crate::{error::Error, services::fs::Fs, skill::Skill};
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
}

pub fn run(fs: &dyn Fs, dir: &Path) -> Result<Vec<Finding>, Error> {
    run_with_rules(fs, dir, &[&spec::SpecRules])
}

pub fn run_with_rules(fs: &dyn Fs, dir: &Path, rules: &[&dyn Rule]) -> Result<Vec<Finding>, Error> {
    let path = dir.join("SKILL.md");
    match fs.symlink_metadata(&path) {
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(vec![Finding::error(
                "S001",
                path,
                "SKILL.md is missing from the skill root",
                "Create SKILL.md at the skill folder root.",
            )]);
        }
        Err(source) => return Err(io_error(&path, source)),
        Ok(info) if !info.is_file() && !info.is_symlink() => {
            return Ok(vec![Finding::error(
                "S001",
                path,
                "SKILL.md must be a file",
                "Replace SKILL.md with a readable Markdown file.",
            )]);
        }
        Ok(_) => {}
    }
    let bytes = fs.read(&path).map_err(|source| io_error(&path, source))?;
    let parsed = std::str::from_utf8(&bytes)
        .map_err(|error| error.to_string())
        .and_then(|document| Skill::parse(dir, document).map_err(|error| error.to_string()));
    let skill = match parsed {
        Ok(skill) => skill,
        Err(message) => {
            return Ok(vec![Finding::error(
                "S002",
                path,
                message,
                "Start SKILL.md with YAML mapping frontmatter between exact --- lines.",
            )]);
        }
    };
    let mut findings = Vec::new();
    for rule in rules {
        findings.extend(rule.check(&skill));
    }
    Ok(findings)
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Validate {}", path.display()),
        source,
        hint: "Check the skill folder and its read permissions.".into(),
    }
}
