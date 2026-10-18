//! Agent Skills specification errors S003–S008 (S001/S002 are runner prerequisites).
use super::{Finding, Rule};
use crate::{
    names::SkillName,
    skill::{Field, Skill},
};

pub struct SpecRules;

impl Rule for SpecRules {
    fn check(&self, skill: &Skill) -> Vec<Finding> {
        let mut findings = Vec::new();
        let fields = &skill.frontmatter;
        let mut error = |rule, message, hint| {
            findings.push(Finding::error(
                rule,
                skill.dir.join("SKILL.md"),
                message,
                hint,
            ))
        };
        match fields.name.value() {
            Some(name) => {
                if SkillName::new(name.clone()).is_err() {
                    error(
                        "S003",
                        "name must match the skill-name pattern and contain 1–64 characters",
                        "Use lowercase ASCII letters and digits separated by single hyphens.",
                    );
                }
                if skill.dir.file_name().and_then(|value| value.to_str()) != Some(name.as_str()) {
                    error(
                        "S004",
                        "name does not equal the skill folder's name",
                        "Make the frontmatter name and skill folder name identical.",
                    );
                }
            }
            None => error(
                "S003",
                "name is required and must be a string",
                "Set name to 1–64 lowercase ASCII letters or digits separated by single hyphens.",
            ),
        }
        if !fields
            .description
            .value()
            .is_some_and(|value| !value.trim().is_empty() && value.chars().count() <= 1024)
        {
            error(
                "S005",
                "description must be a non-empty string of at most 1,024 characters",
                "Add a concise, non-empty description of at most 1,024 characters.",
            );
        }
        match &fields.compatibility {
            Field::Missing => {}
            Field::Valid(value) if (1..=500).contains(&value.chars().count()) => {}
            _ => error(
                "S006",
                "compatibility must be a string of 1–500 characters",
                "Remove compatibility or provide a string of 1–500 characters.",
            ),
        }
        if matches!(fields.metadata, Field::Invalid(_)) {
            error(
                "S007",
                "metadata must map strings to strings",
                "Use a YAML mapping with string keys and string values for metadata.",
            );
        }
        for (name, field) in [
            ("license", &fields.license),
            ("allowed-tools", &fields.allowed_tools),
        ] {
            if matches!(field, Field::Invalid(_)) {
                findings.push(Finding::error(
                    "S008",
                    skill.dir.join("SKILL.md"),
                    format!("{name} must be a string"),
                    "Remove the field or provide a string value.",
                ));
            }
        }
        findings
    }
}
