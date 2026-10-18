use super::*;

fn document(yaml: &str) -> String {
    format!("---\n{yaml}\n---\n# Instructions\n")
}

#[test]
fn parses_all_spec_fields_and_preserves_unknown_fields() {
    let (parsed, body) = parse(&document(
        "name: demo\ndescription: A demo\nlicense: MIT\ncompatibility: Requires git\nallowed-tools: Bash Read\nmetadata:\n  version: '1.2.3-beta.1+build.7'\n  author: Example\ncontext: fork\nhooks:\n  before: [one, two]\ncustom: true",
    ))
    .unwrap();
    assert_eq!(parsed.skill_name().unwrap().unwrap().as_str(), "demo");
    assert_eq!(parsed.description.value().unwrap(), "A demo");
    assert_eq!(parsed.license.value().unwrap(), "MIT");
    assert_eq!(parsed.compatibility.value().unwrap(), "Requires git");
    assert_eq!(parsed.allowed_tools.value().unwrap(), "Bash Read");
    assert_eq!(parsed.metadata.value().unwrap()["author"], "Example");
    assert_eq!(
        parsed.version().unwrap().unwrap().to_string(),
        "1.2.3-beta.1+build.7"
    );
    assert_eq!(parsed.extra.len(), 3);
    assert_eq!(parsed.extra["context"].as_str(), Some("fork"));
    assert!(parsed.extra["hooks"].is_mapping());
    assert_eq!(parsed.extra["custom"].as_bool(), Some(true));
    assert_eq!(body, "# Instructions\n");
}

#[test]
fn accepts_bom_and_crlf_without_normalizing_body() {
    for bom in ["", "\u{feff}"] {
        for newline in ["\n", "\r\n"] {
            let body = format!("{newline}# Demo{newline}---{newline}é 🦀{newline}");
            let input = format!("{bom}---{newline}name: demo{newline}---{newline}{body}");
            let (parsed, actual_body) = parse(&input).unwrap();
            assert_eq!(parsed.name.value().unwrap(), "demo");
            assert_eq!(actual_body, body);
        }
    }
}

#[test]
fn rejects_missing_or_inexact_delimiters() {
    for input in [
        "",
        "name: demo",
        "\n---\n{}\n---",
        " ---\n{}\n---",
        "--- \n{}\n---",
        "---",
        "---\nname: demo",
        "---\n{}\n--- ",
        "---\n{}\n ---",
        "---\n{}\n---\r",
    ] {
        let error = parse(input).unwrap_err();
        assert_eq!(error.exit_code(), 3, "{input:?}");
        assert!(!error.hint().is_empty());
    }
}

#[test]
fn rejects_empty_non_mapping_and_malformed_yaml() {
    for yaml in [
        "",
        "# comment",
        "null",
        "~",
        "text",
        "42",
        "[]",
        "- name: demo",
        "name: [",
        "name: one\nname: two",
        "? [a, b]\n: value",
    ] {
        assert!(parse(&document(yaml)).is_err(), "{yaml:?}");
    }
}

#[test]
fn empty_mapping_is_structurally_valid_and_fields_remain_missing() {
    let (parsed, _) = parse(&document("{}")).unwrap();
    assert_eq!(parsed, Frontmatter::default());
    assert_eq!(parsed.skill_name().unwrap(), None);
    assert_eq!(parsed.version().unwrap(), None);
}

#[test]
fn closing_delimiter_at_eof_and_exact_split_are_supported() {
    let input = "---\nname: demo\n---";
    assert_eq!(split(input).unwrap(), ("name: demo\n", ""));
    assert_eq!(parse(input).unwrap().1, "");
    let input = "---\r\nname: demo\r\n---\r\nbody without final newline";
    assert_eq!(
        split(input).unwrap(),
        ("name: demo\r\n", "body without final newline")
    );
}

#[test]
fn wrong_types_are_preserved_for_individual_validation_rules() {
    let (parsed, _) = parse(&document(
        "name: 42\ndescription: null\nlicense: [MIT]\ncompatibility: false\nallowed-tools: {tool: Bash}\nmetadata:\n  version: 123",
    ))
    .unwrap();
    assert!(matches!(parsed.name, Field::Invalid(Value::Number(_))));
    assert_eq!(parsed.description, Field::Invalid(Value::Null));
    assert!(matches!(parsed.license, Field::Invalid(Value::Sequence(_))));
    assert!(matches!(
        parsed.compatibility,
        Field::Invalid(Value::Bool(false))
    ));
    assert!(matches!(
        parsed.allowed_tools,
        Field::Invalid(Value::Mapping(_))
    ));
    assert!(matches!(parsed.metadata, Field::Invalid(Value::Mapping(_))));
    assert!(parsed.skill_name().is_err());
    assert!(parsed.version().is_err());
}

#[test]
fn missing_version_is_distinct_from_invalid_version() {
    for yaml in ["{}", "metadata: {}", "metadata: {author: Example}"] {
        assert_eq!(parse(&document(yaml)).unwrap().0.version().unwrap(), None);
    }
    for yaml in [
        "metadata: null",
        "metadata: []",
        "metadata: {version: null}",
        "metadata: {version: ''}",
        "metadata: {version: '1.2'}",
        "metadata: {version: 'v1.2.3'}",
    ] {
        assert!(
            parse(&document(yaml)).unwrap().0.version().is_err(),
            "{yaml}"
        );
    }
    let parsed = parse(&document("name: Invalid_Name")).unwrap().0;
    assert!(parsed.skill_name().is_err());
}

#[test]
fn supports_yaml_multiline_strings_and_retains_unicode() {
    let (parsed, _) = parse(&document(
        "name: demo\ndescription: >-\n  Use when café\n  needs help.\nmetadata:\n  version: 1.2.3",
    ))
    .unwrap();
    assert_eq!(
        parsed.description.value().unwrap(),
        "Use when café needs help."
    );
    assert_eq!(parsed.version().unwrap(), Some(Version::new(1, 2, 3)));
}
