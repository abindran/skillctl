//! Exercise document parsing through the public API and filesystem service.

use std::fs;

use semver::Version;
use skillctl::{
    commands::init::DEFAULT_TEMPLATE,
    error::Error,
    services::fs::{DryRunFs, RealFs},
    skill::Skill,
};

#[test]
fn loads_scaffolded_template_using_real_and_dry_run_filesystems() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("demo");
    fs::create_dir(&dir).unwrap();
    let document = DEFAULT_TEMPLATE.replace("{{name}}", "demo");
    fs::write(dir.join("SKILL.md"), &document).unwrap();

    let skill = Skill::load(&RealFs, &dir).unwrap();
    assert_eq!(skill.dir, dir);
    assert_eq!(
        skill.frontmatter.skill_name().unwrap().unwrap().as_str(),
        "demo"
    );
    assert_eq!(skill.version().unwrap(), Some(Version::new(0, 1, 0)));
    assert!(skill.body.contains("# demo"));
    assert_eq!(skill, Skill::parse(&dir, &document).unwrap());

    let dry_run = DryRunFs::new();
    assert_eq!(Skill::load(&dry_run, &dir).unwrap(), skill);
    assert!(dry_run.actions().is_empty());
    assert_eq!(fs::read_to_string(dir.join("SKILL.md")).unwrap(), document);
}

#[test]
fn missing_root_document_preserves_io_error_and_path() {
    let temp = tempfile::tempdir().unwrap();
    let error = Skill::load(&RealFs, temp.path()).unwrap_err();
    assert!(error.to_string().contains("SKILL.md"));
    match error {
        Error::Io { source, .. } => assert_eq!(source.kind(), std::io::ErrorKind::NotFound),
        other => panic!("expected I/O error, got {other}"),
    }
}

#[test]
fn invalid_utf8_and_malformed_documents_report_the_file_path() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("SKILL.md");
    for bytes in [
        b"\xff\xfe".as_slice(),
        b"---\nname: [\n---\n",
        b"no frontmatter",
    ] {
        fs::write(&path, bytes).unwrap();
        let error = Skill::load(&RealFs, temp.path()).unwrap_err();
        assert_eq!(error.exit_code(), 3);
        assert!(error.to_string().contains(path.to_str().unwrap()));
    }
}

#[test]
fn structural_parsing_does_not_apply_later_validation_rules() {
    // Missing version, invalid name and folder mismatch are validation concerns.
    let skill = Skill::parse("different-folder", "---\nname: BAD_NAME\n---\n").unwrap();
    assert!(skill.frontmatter.skill_name().is_err());
    assert_eq!(skill.version().unwrap(), None);
    assert!(skill.body.is_empty());
}
