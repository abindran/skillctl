use std::{fs, io};

use semver::Version;
use serde_json::{Value, json};
use skillctl::{
    artifact::Kind,
    error::Error,
    services::fs::{DryRunFs, FsAction, RealFs},
    store::{
        hash::content_hash,
        meta::{Metadata, Source},
    },
};

fn example() -> Value {
    json!({
        "name": "org-code-review",
        "version": "1.4.0",
        "kind": "skill",
        "source": {
            "kind": "git",
            "url": "git@gitlab.zoho.com:platform/org-skills.git",
            "ref": "v1.4.0",
            "commit": "9f2c1e7",
            "subdir": "skills/org-code-review"
        },
        "content_hash": format!("sha256:{}", "ab".repeat(32)),
        "installed_at": "2026-10-18T09:05:00Z",
        "skillctl_version": "0.1.0"
    })
}

fn parse(value: &Value) -> Metadata {
    Metadata::from_json(&serde_json::to_vec(value).unwrap()).unwrap()
}

#[test]
fn design_example_round_trips_every_field() {
    let value = example();
    let metadata = parse(&value);
    assert_eq!(metadata.name.to_string(), "org-code-review");
    assert_eq!(metadata.version, Version::new(1, 4, 0));
    assert_eq!(metadata.skillctl_version, Version::new(0, 1, 0));
    assert!(
        matches!(&metadata.source, Source::Git { reference: Some(reference), .. } if reference == "v1.4.0")
    );
    let json = metadata.to_json().unwrap();
    assert!(json.ends_with('\n'));
    assert_eq!(serde_json::from_str::<Value>(&json).unwrap(), value);
    assert_eq!(Metadata::from_json(json.as_bytes()).unwrap(), metadata);
}

#[test]
fn metadata_without_kind_is_a_skill_and_hooks_round_trip() {
    // Versions stored before hooks existed have no kind field.
    let mut value = example();
    value.as_object_mut().unwrap().remove("kind");
    let metadata = parse(&value);
    assert_eq!(metadata.kind, Kind::Skill);
    let json: Value = serde_json::from_str(&metadata.to_json().unwrap()).unwrap();
    assert_eq!(json["kind"], "skill");
    let mut value = example();
    value["kind"] = json!("hook");
    let metadata = parse(&value);
    assert_eq!(metadata.kind, Kind::Hook);
    assert_eq!(
        serde_json::from_str::<Value>(&metadata.to_json().unwrap()).unwrap(),
        value
    );
}

#[test]
fn all_source_kinds_and_optional_git_fields_round_trip() {
    for source in [
        json!({"kind": "local"}),
        json!({"kind": "new"}),
        json!({"kind": "git", "url": "https://example.org/skills.git", "commit": "abc123"}),
    ] {
        let mut value = example();
        value["source"] = source;
        let metadata = parse(&value);
        assert_eq!(
            serde_json::from_str::<Value>(&metadata.to_json().unwrap()).unwrap(),
            value
        );
    }
}

#[test]
fn prerelease_and_build_versions_round_trip() {
    let mut value = example();
    value["version"] = json!("1.4.0-beta.1+build.42");
    value["skillctl_version"] = json!("0.2.0-rc.1+test");
    let metadata = parse(&value);
    assert_eq!(
        Metadata::from_json(metadata.to_json().unwrap().as_bytes()).unwrap(),
        metadata
    );
}

#[test]
fn malformed_and_incomplete_metadata_are_errors() {
    for bytes in [b"{".as_slice(), b"null", b"[]", b"{}", b"\xff"] {
        assert!(
            matches!(Metadata::from_json(bytes), Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidData)
        );
    }
    for field in [
        "name",
        "version",
        "source",
        "content_hash",
        "installed_at",
        "skillctl_version",
    ] {
        let mut value = example();
        value.as_object_mut().unwrap().remove(field);
        assert!(
            Metadata::from_json(&serde_json::to_vec(&value).unwrap()).is_err(),
            "missing {field}"
        );
    }
    for (field, invalid) in [
        ("name", json!("Bad_Name")),
        ("version", json!("not-semver")),
        ("skillctl_version", json!("1.2")),
        ("installed_at", json!("yesterday")),
        ("source", json!({"kind": "unknown"})),
        (
            "source",
            json!({"kind": "git", "url": "https://example.org/repo"}),
        ),
        ("content_hash", json!(42)),
    ] {
        let mut value = example();
        value[field] = invalid;
        assert!(
            Metadata::from_json(&serde_json::to_vec(&value).unwrap()).is_err(),
            "invalid {field}"
        );
    }
}

#[test]
fn save_load_and_atomic_replacement_beside_skill() {
    let temp = tempfile::tempdir().unwrap();
    let skill = temp.path().join("org-code-review");
    fs::create_dir(&skill).unwrap();
    fs::write(skill.join("SKILL.md"), "skill bytes").unwrap();
    let hash = content_hash(&RealFs, &skill).unwrap();
    let path = temp.path().join("meta.json");
    let mut metadata = parse(&example());
    metadata.content_hash = hash.clone();
    metadata.save(&RealFs, &path).unwrap();
    assert_eq!(Metadata::load(&RealFs, &path).unwrap(), metadata);
    metadata.source = Source::Local;
    metadata.save(&RealFs, &path).unwrap();
    assert_eq!(Metadata::load(&RealFs, &path).unwrap(), metadata);
    assert_eq!(content_hash(&RealFs, &skill).unwrap(), hash);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
}

#[test]
fn dry_run_records_exact_atomic_write_without_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("meta.json");
    let original = parse(&example());
    original.save(&RealFs, &path).unwrap();
    let dry = DryRunFs::new();
    assert_eq!(Metadata::load(&dry, &path).unwrap(), original);
    let mut changed = original.clone();
    changed.source = Source::New;
    changed.save(&dry, &path).unwrap();
    let missing = temp.path().join("missing.json");
    changed.save(&dry, &missing).unwrap();
    assert!(!missing.exists());
    assert_eq!(Metadata::load(&RealFs, &path).unwrap(), original);
    assert_eq!(
        dry.actions(),
        vec![
            FsAction::WriteAtomic {
                path,
                bytes: changed.to_json().unwrap().into_bytes()
            },
            FsAction::WriteAtomic {
                path: missing,
                bytes: changed.to_json().unwrap().into_bytes()
            },
        ]
    );
}

#[test]
fn io_errors_include_path_and_preserve_source() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("meta.json");
    let error = Metadata::load(&RealFs, &path).unwrap_err();
    assert!(error.to_string().contains(&path.display().to_string()));
    assert!(matches!(error, Error::Io { source, .. } if source.kind() == io::ErrorKind::NotFound));
    fs::write(&path, "not JSON").unwrap();
    let error = Metadata::load(&RealFs, &path).unwrap_err();
    assert!(error.to_string().contains(&path.display().to_string()));
    assert_eq!(error.exit_code(), 1);
    assert!(
        matches!(error, Error::Io { source, .. } if source.kind() == io::ErrorKind::InvalidData)
    );
    let missing_parent = temp.path().join("missing/meta.json");
    let error = parse(&example())
        .save(&RealFs, &missing_parent)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&missing_parent.display().to_string())
    );
    assert!(!missing_parent.parent().unwrap().exists());
}
