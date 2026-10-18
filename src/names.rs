//! Validated skill and project names shared by the CLI and registry.
//!
//! Names contain 1–64 ASCII lowercase letters, digits, or single separating
//! hyphens. Construction and deserialization both enforce the same rules.
use std::{borrow::Borrow, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::error::Error;

fn validate_name(value: &str) -> Result<(), &'static str> {
    if value.is_empty() || value.len() > 64 {
        return Err("must contain 1–64 ASCII characters");
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("must contain only lowercase ASCII letters, digits, and hyphens");
    }
    if value.starts_with('-') || value.ends_with('-') || value.contains("--") {
        return Err("hyphens must be single separators between letters or digits");
    }
    Ok(())
}

macro_rules! define_name {
    ($name:ident, $kind:literal) => {
        #[doc = concat!("A validated ", $kind, " name. The inner string cannot be mutated.")]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Validate a name without trimming, lowercasing, or otherwise changing it.
            pub fn new(value: impl Into<String>) -> Result<Self, Error> {
                let value = value.into();
                validate_name(&value).map_err(|reason| Error::Usage {
                    message: format!("Invalid {} name {value:?}: {reason}", $kind),
                    hint:
                        "Use 1–64 lowercase ASCII letters or digits, separated by single hyphens."
                            .to_owned(),
                })?;
                Ok(Self(value))
            }

            /// Borrow the validated name.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume the name and return its string without copying it.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl FromStr for $name {
            type Err = Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = Error;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = Error;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.into_string()
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

define_name!(SkillName, "skill");
define_name!(ProjectName, "project");

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};

    #[test]
    fn valid_name_table_passes_for_both_types() {
        for value in [
            "a".to_owned(),
            "0".to_owned(),
            "123".to_owned(),
            "org-code-review".to_owned(),
            "a1-b2-3".to_owned(),
            "a".repeat(64),
            format!("{}-b", "a".repeat(62)),
        ] {
            let skill = SkillName::new(value.clone()).unwrap();
            let project = ProjectName::new(value.clone()).unwrap();
            assert_eq!(skill.as_str(), value);
            assert_eq!(project.as_str(), value);
        }
    }

    #[test]
    fn invalid_name_table_fails_for_both_types() {
        for value in [
            "".to_owned(),
            "a".repeat(65),
            "-a".to_owned(),
            "a-".to_owned(),
            "a--b".to_owned(),
            "A".to_owned(),
            "a_b".to_owned(),
            "a.b".to_owned(),
            " a".to_owned(),
            "a ".to_owned(),
            "a b".to_owned(),
            "a\nb".to_owned(),
            "a\tb".to_owned(),
            "a\0b".to_owned(),
            "../a".to_owned(),
            "a/b".to_owned(),
            "a\\b".to_owned(),
            "demo@1.2.0".to_owned(),
            "café".to_owned(),
            "技能".to_owned(),
            "a–b".to_owned(),
        ] {
            for error in [
                SkillName::new(value.clone()).unwrap_err(),
                ProjectName::new(value.clone()).unwrap_err(),
            ] {
                assert_eq!(error.exit_code(), 2, "{value:?}");
                assert!(!error.hint().is_empty());
            }
        }
    }

    #[test]
    fn string_conversions_preserve_validated_names() {
        let skill: SkillName = "org-review".parse().unwrap();
        assert_eq!(skill.to_string(), "org-review");
        assert_eq!(skill.as_ref(), "org-review");
        assert_eq!(String::from(skill.clone()), "org-review");
        assert_eq!(SkillName::try_from("org-review").unwrap(), skill);
        assert_eq!(SkillName::try_from("org-review".to_owned()).unwrap(), skill);
        assert_eq!(skill.into_string(), "org-review");

        let project: ProjectName = "web-app".parse().unwrap();
        assert_eq!(project.to_string(), "web-app");
        assert_eq!(project.as_ref(), "web-app");
        assert_eq!(String::from(project.clone()), "web-app");
        assert_eq!(ProjectName::try_from("web-app").unwrap(), project);
        assert_eq!(
            ProjectName::try_from("web-app".to_owned()).unwrap(),
            project
        );
        assert_eq!(project.into_string(), "web-app");
        assert!("Invalid".parse::<SkillName>().is_err());
        assert!("Invalid".parse::<ProjectName>().is_err());
    }

    #[test]
    fn names_serialize_as_strings_and_deserialization_validates() {
        let skill = SkillName::new("org-review").unwrap();
        let project = ProjectName::new("web-app").unwrap();
        assert_eq!(serde_json::to_string(&skill).unwrap(), "\"org-review\"");
        assert_eq!(serde_json::to_string(&project).unwrap(), "\"web-app\"");
        assert_eq!(
            serde_json::from_str::<SkillName>("\"org-review\"").unwrap(),
            skill
        );
        assert_eq!(
            serde_json::from_str::<ProjectName>("\"web-app\"").unwrap(),
            project
        );
        for value in ["\"\"", "\"Invalid\"", "\"a--b\"", "null", "42", "{}"] {
            assert!(serde_json::from_str::<SkillName>(value).is_err());
            assert!(serde_json::from_str::<ProjectName>(value).is_err());
        }
    }

    #[test]
    fn names_work_as_sorted_and_hashed_keys() {
        let skills = BTreeMap::from([
            (SkillName::new("z-last").unwrap(), 2),
            (SkillName::new("a-first").unwrap(), 1),
        ]);
        assert_eq!(skills.get("a-first"), Some(&1));
        let json = serde_json::to_string(&skills).unwrap();
        assert_eq!(json, r#"{"a-first":1,"z-last":2}"#);
        assert_eq!(
            serde_json::from_str::<BTreeMap<SkillName, i32>>(&json).unwrap(),
            skills
        );
        assert!(serde_json::from_str::<BTreeMap<SkillName, i32>>(r#"{"INVALID":1}"#).is_err());

        let projects = BTreeMap::from([(ProjectName::new("web").unwrap(), 1)]);
        assert_eq!(projects.get("web"), Some(&1));
        let json = serde_json::to_string(&projects).unwrap();
        assert_eq!(
            serde_json::from_str::<BTreeMap<ProjectName, i32>>(&json).unwrap(),
            projects
        );
        assert!(serde_json::from_str::<BTreeMap<ProjectName, i32>>(r#"{"INVALID":1}"#).is_err());

        let skills = HashSet::from([
            SkillName::new("demo").unwrap(),
            SkillName::new("demo").unwrap(),
        ]);
        assert_eq!(skills.len(), 1);
        let projects = HashSet::from([
            ProjectName::new("web").unwrap(),
            ProjectName::new("web").unwrap(),
        ]);
        assert_eq!(projects.len(), 1);
    }
}
