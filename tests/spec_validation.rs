use skillctl::{
    services::fs::{DryRunFs, RealFs},
    validate::{self, Severity},
};

const VALID: &str = "---\nname: demo\ndescription: A valid description\n---\nBody\n";

fn check(document: Option<&str>, folder: &str) -> Vec<validate::Finding> {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join(folder);
    std::fs::create_dir(&dir).unwrap();
    if let Some(document) = document {
        std::fs::write(dir.join("SKILL.md"), document).unwrap();
    }
    validate::run(&RealFs, &dir).unwrap()
}

#[test]
fn one_fixture_per_spec_rule_has_only_that_rule() {
    let cases = [
        ("S001", None, "demo"),
        ("S002", Some("not frontmatter".to_string()), "demo"),
        ("S003", Some(VALID.replace("demo", "Bad")), "Bad"),
        ("S004", Some(VALID.to_string()), "different"),
        (
            "S005",
            Some(VALID.replace("A valid description", "''")),
            "demo",
        ),
        (
            "S006",
            Some(VALID.replace("description:", "compatibility: []\ndescription:")),
            "demo",
        ),
        (
            "S007",
            Some(VALID.replace("description:", "metadata: {version: 1}\ndescription:")),
            "demo",
        ),
        (
            "S008",
            Some(VALID.replace("description:", "license: []\ndescription:")),
            "demo",
        ),
    ];
    for (id, document, dir) in cases {
        let findings = check(document.as_deref(), dir);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.rule.as_str())
                .collect::<Vec<_>>(),
            [id],
            "{id}: {findings:?}"
        );
        assert_eq!(findings[0].severity, Severity::Error);
        assert!(!findings[0].hint.is_empty());
    }
}

#[test]
fn fully_valid_unknown_fields_bom_and_crlf_have_no_findings() {
    let text = "---\nname: demo\ndescription: Valid\ncompatibility: Rust\nlicense: MIT\nallowed-tools: Bash\nmetadata: {version: '1.0.0', skillctl-allow: S003}\ncustom-field: true\n---\nBody\n";
    assert!(check(Some(text), "demo").is_empty());
    assert!(
        check(
            Some(&format!("\u{feff}{}", text.replace('\n', "\r\n"))),
            "demo"
        )
        .is_empty()
    );
}

#[test]
fn limits_count_unicode_characters_and_optional_types_are_checked() {
    assert!(
        check(
            Some(&VALID.replace("A valid description", &"界".repeat(1024))),
            "demo"
        )
        .is_empty()
    );
    assert_eq!(
        check(
            Some(&VALID.replace("A valid description", &"界".repeat(1025))),
            "demo"
        )[0]
        .rule,
        "S005"
    );
    for value in ["null", "[]", "3", "''", &format!("'{}'", "界".repeat(501))] {
        let text = VALID.replace(
            "description:",
            &format!("compatibility: {value}\ndescription:"),
        );
        assert_eq!(check(Some(&text), "demo")[0].rule, "S006", "{value}");
    }
    for field in ["license", "allowed-tools"] {
        assert_eq!(
            check(
                Some(&VALID.replace("description:", &format!("{field}: false\ndescription:"))),
                "demo"
            )[0]
            .rule,
            "S008"
        );
    }
    for value in ["null", "[]", "{1: text}", "{version: false}"] {
        assert_eq!(
            check(
                Some(&VALID.replace("description:", &format!("metadata: {value}\ndescription:"))),
                "demo"
            )[0]
            .rule,
            "S007"
        );
    }
}

#[test]
fn structural_errors_do_not_cascade_and_fields_accumulate() {
    for text in ["---\n[]\n---\n", "---\nname: [\n---\n", "---\n---\n"] {
        let findings = check(Some(text), "demo");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "S002");
    }
    let findings = check(Some("---\n{}\n---\n"), "demo");
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.rule.as_str())
            .collect::<Vec<_>>(),
        ["S003", "S005"]
    );
}

#[test]
fn report_has_locations_hints_summary_and_runner_is_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let fs = DryRunFs::new();
    let mut findings = validate::run(&fs, temp.path()).unwrap();
    findings[0].line = Some(1);
    let report = validate::report::text(&findings);
    assert!(report.contains("SKILL.md:1 S001 error"));
    assert!(report.contains("Hint: "));
    assert!(report.ends_with("1 error(s), 0 warning(s)"));
    assert_eq!(validate::report::text(&[]), "0 error(s), 0 warning(s)");
    assert!(fs.actions().is_empty());
}

#[test]
fn invalid_utf8_is_s002_and_directory_is_s001() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("SKILL.md"), [255]).unwrap();
    assert_eq!(validate::run(&RealFs, temp.path()).unwrap()[0].rule, "S002");
    std::fs::remove_file(temp.path().join("SKILL.md")).unwrap();
    std::fs::create_dir(temp.path().join("SKILL.md")).unwrap();
    assert_eq!(validate::run(&RealFs, temp.path()).unwrap()[0].rule, "S001");
}
