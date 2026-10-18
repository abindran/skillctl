use skillctl::{
    error::Error,
    validate::{
        Finding, Severity,
        report::{self, SkillReport},
    },
};

#[test]
fn already_reported_validation_preserves_exit_contract() {
    assert_eq!(Error::ReportedValidation.exit_code(), 3);
    assert_eq!(Error::ReportedValidation.kind(), "validation");
    assert!(!Error::ReportedValidation.hint().is_empty());
}

#[test]
fn json_preserves_all_public_finding_fields_and_escapes_control_characters() {
    let finding = Finding {
        rule: "H205".into(),
        severity: Severity::Warning,
        path: "demo/SKILL.md".into(),
        line: Some(8),
        message: "Unknown \"field\"\nnext line".into(),
        hint: "Remove it".into(),
    };
    let json = report::json(std::slice::from_ref(&finding)).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value[0]["rule"], "H205");
    assert_eq!(value[0]["severity"], "warning");
    assert_eq!(value[0]["line"], 8);
    assert_eq!(value[0]["message"], finding.message);
    assert_eq!(value[0]["hint"], "Remove it");
    assert_eq!(value[0]["path"], "demo/SKILL.md");
}

#[test]
fn junit_keeps_passing_skills_and_sanitizes_xml_controls() {
    let finding = Finding::error(
        "O104",
        "demo/SKILL.md".into(),
        "bad <field> & \"value\"\u{1}",
        "Remove < and >",
    );
    let reports = vec![
        SkillReport {
            dir: "demo & sample".into(),
            findings: vec![finding],
        },
        SkillReport {
            dir: "passing".into(),
            findings: vec![],
        },
    ];
    let xml = report::junit(&reports);
    assert!(!xml.contains('\u{1}'));
    let parsed = quick_junit::Report::deserialize_from_str(&xml).unwrap();
    assert_eq!(parsed.tests, 2);
    assert_eq!(parsed.failures, 1);
    assert_eq!(parsed.test_suites[0].test_cases.len(), 2);
    assert_eq!(
        parsed.test_suites[0].test_cases[0].name.as_str(),
        "demo & sample"
    );
}
