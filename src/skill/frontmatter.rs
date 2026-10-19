//! Structural parsing only: field-level spec and organization rules belong to validation.

use std::collections::BTreeMap;

use semver::Version;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_yaml_ng::Value;

use crate::{error::Error, names::SkillName};

/// Preserve missing and ill-typed values so validation can report each offending field.
/// In particular, explicit YAML null is invalid rather than silently treated as missing.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Field<T> {
    #[default]
    Missing,
    Valid(T),
    Invalid(Value),
}

impl<T> Field<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Valid(value) => Some(value),
            Self::Missing | Self::Invalid(_) => None,
        }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value.is_null() {
            return Ok(Self::Invalid(value));
        }
        Ok(match serde_yaml_ng::from_value(value.clone()) {
            Ok(parsed) => Self::Valid(parsed),
            Err(_) => Self::Invalid(value),
        })
    }
}

/// Spec fields are typed; harness-specific and unknown fields retain their YAML values.
/// Missing/invalid fields are deliberately not rejected until the validation work item.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Frontmatter {
    #[serde(default)]
    pub name: Field<String>,
    #[serde(default)]
    pub description: Field<String>,
    #[serde(default)]
    pub license: Field<String>,
    #[serde(default)]
    pub compatibility: Field<String>,
    #[serde(default, rename = "allowed-tools")]
    pub allowed_tools: Field<String>,
    #[serde(default)]
    pub metadata: Field<BTreeMap<String, String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Frontmatter {
    /// Validate the declared name without requiring it during structural parsing.
    pub fn skill_name(&self) -> Result<Option<SkillName>, Error> {
        match &self.name {
            Field::Missing => Ok(None),
            Field::Valid(name) => SkillName::new(name.clone())
                .map(Some)
                .map_err(|error| invalid(format!("Invalid name: {error}"))),
            Field::Invalid(_) => Err(invalid("name must be a string")),
        }
    }

    /// Read metadata.version as semver. Absence is allowed here so install/new can
    /// supply an explicit version; malformed metadata or versions are never ignored.
    pub fn version(&self) -> Result<Option<Version>, Error> {
        match &self.metadata {
            Field::Missing => Ok(None),
            Field::Invalid(_) => Err(invalid("metadata must map strings to strings")),
            Field::Valid(metadata) => metadata
                .get("version")
                .map(|version| {
                    Version::parse(version).map_err(|error| {
                        invalid(format!("Invalid metadata.version {version:?}: {error}"))
                    })
                })
                .transpose(),
        }
    }
}

pub(super) fn invalid(message: impl Into<String>) -> Error {
    Error::Validation {
        summary: message.into(),
        hint: "Fix the root document (SKILL.md or HOOK.md): use YAML mapping frontmatter between exact --- lines; metadata.version must be a semver string.".into(),
    }
}

/// Split exact delimiter lines, accepting one leading UTF-8 BOM and LF/CRLF.
/// The Markdown body is returned byte-for-byte, including its original newlines.
/// A closing delimiter at EOF is valid and produces an empty body.
pub fn split(document: &str) -> Result<(&str, &str), Error> {
    let document = document.strip_prefix('\u{feff}').unwrap_or(document);
    let mut lines = document.split_inclusive('\n');
    let opening = lines.next().ok_or_else(|| invalid("Missing frontmatter"))?;
    if line_text(opening) != "---" {
        return Err(invalid("The document must start with an exact --- line"));
    }
    let start = opening.len();
    let mut offset = start;
    for line in lines {
        if line_text(line) == "---" {
            return Ok((&document[start..offset], &document[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(invalid("Missing closing --- frontmatter delimiter"))
}

fn line_text(line: &str) -> &str {
    // Only CRLF is a newline; a bare CR must not turn a non-delimiter into one.
    match line.strip_suffix('\n') {
        Some(line) => line.strip_suffix('\r').unwrap_or(line),
        None => line,
    }
}

/// Parse a single YAML mapping and retain the Markdown body separately.
pub fn parse(document: &str) -> Result<(Frontmatter, String), Error> {
    let (yaml, body) = split(document)?;
    let value: Value = serde_yaml_ng::from_str(yaml)
        .map_err(|error| invalid(format!("Invalid frontmatter YAML: {error}")))?;
    if !value.is_mapping() {
        return Err(invalid("Frontmatter must be a YAML mapping"));
    }
    let frontmatter = serde_yaml_ng::from_value(value)
        .map_err(|error| invalid(format!("Invalid frontmatter mapping: {error}")))?;
    Ok((frontmatter, body.to_owned()))
}

#[cfg(test)]
mod tests;
