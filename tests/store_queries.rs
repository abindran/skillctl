use std::{fs, io};

use semver::Version;
use skillctl::{
    error::Error,
    names::SkillName,
    paths::Paths,
    registry::Pin,
    services::fs::{DryRunFs, RealFs},
    store::{latest_stable, list_skills, list_versions, resolve_pin, store_dir},
};

fn name(text: &str) -> SkillName {
    text.parse().unwrap()
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn setup() -> (tempfile::TempDir, Paths) {
    let temp = tempfile::tempdir().unwrap();
    let paths = Paths::resolve(Some(temp.path())).unwrap();
    (temp, paths)
}

fn install(paths: &Paths, skill: &str, release: &str) {
    let dir = paths.store_dir(&name(skill), &version(release));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("SKILL.md"), "---\nname: demo\n---\nBody\n").unwrap();
}

#[test]
fn missing_store_and_skill_are_empty_without_creating_directories() {
    let (_temp, paths) = setup();
    assert!(list_skills(&RealFs, &paths).unwrap().is_empty());
    assert!(
        list_versions(&RealFs, &paths, &name("demo"))
            .unwrap()
            .is_empty()
    );
    assert!(!paths.store().exists());
    fs::create_dir(paths.store()).unwrap();
    assert!(
        list_versions(&RealFs, &paths, &name("demo"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn skills_are_sorted_valid_directories_including_empty_ones() {
    let (_temp, paths) = setup();
    for skill in ["zeta", "alpha", "Bad_Name", ".staging-abcd"] {
        fs::create_dir_all(paths.store().join(skill)).unwrap();
    }
    fs::write(paths.store().join("a-file"), "").unwrap();
    assert_eq!(
        list_skills(&RealFs, &paths).unwrap(),
        vec![name("alpha"), name("zeta")]
    );
}

#[test]
fn versions_use_semver_order_and_ignore_staging_and_files() {
    let (_temp, paths) = setup();
    for release in [
        "1.10.0",
        "1.4.0",
        "1.6.0-beta.10",
        "1.6.0-beta.2",
        "1.5.0",
        "1.5.0+build",
    ] {
        install(&paths, "demo", release);
    }
    for junk in [".staging-1234", "draft", "v2.0.0", "01.0.0"] {
        fs::create_dir(paths.skill_versions(&name("demo")).join(junk)).unwrap();
    }
    fs::write(paths.skill_versions(&name("demo")).join("9.0.0"), "").unwrap();
    let expected = [
        "1.4.0",
        "1.5.0",
        "1.5.0+build",
        "1.6.0-beta.2",
        "1.6.0-beta.10",
        "1.10.0",
    ]
    .map(version);
    assert_eq!(
        list_versions(&RealFs, &paths, &name("demo")).unwrap(),
        expected
    );
}

#[test]
fn latest_selects_highest_stable_not_highest_prerelease() {
    let (_temp, paths) = setup();
    for release in ["1.4.0", "1.6.0-beta", "1.5.0"] {
        install(&paths, "demo", release);
    }
    assert_eq!(
        latest_stable(&RealFs, &paths, &name("demo")).unwrap(),
        version("1.5.0")
    );
    assert_eq!(
        resolve_pin(&RealFs, &paths, &name("demo"), None).unwrap(),
        Pin::Version(version("1.5.0"))
    );
}

#[test]
fn no_stable_version_is_an_actionable_error_and_build_metadata_is_stable() {
    let (_temp, paths) = setup();
    for release in [None, Some("2.0.0-rc.1")] {
        if let Some(release) = release {
            install(&paths, "demo", release);
        }
        let error = latest_stable(&RealFs, &paths, &name("demo")).unwrap_err();
        assert_eq!(error.exit_code(), 1);
        assert!(error.to_string().contains("No stable version of demo"));
        assert!(error.hint().contains("install"));
    }
    install(&paths, "demo", "1.0.0+build.2");
    assert_eq!(
        latest_stable(&RealFs, &paths, &name("demo")).unwrap(),
        version("1.0.0+build.2")
    );
}

#[test]
fn explicit_versions_and_drafts_resolve_without_falling_back() {
    let (_temp, paths) = setup();
    install(&paths, "demo", "1.4.0");
    install(&paths, "demo", "2.0.0-beta");
    for release in ["1.4.0", "2.0.0-beta"] {
        let pin = Pin::Version(version(release));
        assert_eq!(
            resolve_pin(&RealFs, &paths, &name("demo"), Some(&pin)).unwrap(),
            pin
        );
        assert_eq!(
            store_dir(&paths, &name("demo"), &pin),
            paths.store_dir(&name("demo"), &version(release))
        );
    }
    let missing = Pin::Version(version("9.0.0"));
    assert!(resolve_pin(&RealFs, &paths, &name("demo"), Some(&missing)).is_err());
    assert!(resolve_pin(&RealFs, &paths, &name("demo"), Some(&Pin::Draft)).is_err());
    fs::create_dir_all(paths.draft_dir(&name("demo"))).unwrap();
    assert_eq!(
        resolve_pin(&RealFs, &paths, &name("demo"), Some(&Pin::Draft)).unwrap(),
        Pin::Draft
    );
    assert_eq!(
        store_dir(&paths, &name("demo"), &Pin::Draft),
        paths.draft_dir(&name("demo"))
    );
}

#[test]
fn directory_index_does_not_depend_on_valid_metadata_or_skill_contents() {
    let (_temp, paths) = setup();
    install(&paths, "demo", "1.0.0");
    fs::write(
        paths
            .version_dir(&name("demo"), &version("1.0.0"))
            .join("meta.json"),
        "broken JSON",
    )
    .unwrap();
    assert_eq!(
        list_versions(&RealFs, &paths, &name("demo")).unwrap(),
        vec![version("1.0.0")]
    );
    fs::remove_dir_all(paths.store_dir(&name("demo"), &version("1.0.0"))).unwrap();
    // Keep the broken version visible for doctor, but do not resolve a missing target.
    assert_eq!(
        list_versions(&RealFs, &paths, &name("demo")).unwrap(),
        vec![version("1.0.0")]
    );
    assert!(resolve_pin(&RealFs, &paths, &name("demo"), None).is_err());
}

#[test]
fn wrong_file_types_are_path_aware_errors_not_empty_results() {
    let (_temp, paths) = setup();
    fs::write(paths.store(), "not a directory").unwrap();
    let error = list_skills(&RealFs, &paths).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&paths.store().display().to_string())
    );
    assert!(
        matches!(error, Error::Io { source, .. } if source.kind() == io::ErrorKind::InvalidData)
    );
    assert!(list_versions(&RealFs, &paths, &name("demo")).is_err());
}

#[test]
fn dry_run_queries_record_no_mutations() {
    let (_temp, paths) = setup();
    install(&paths, "demo", "1.0.0");
    let fs = DryRunFs::new();
    assert_eq!(list_skills(&fs, &paths).unwrap(), vec![name("demo")]);
    assert_eq!(
        list_versions(&fs, &paths, &name("demo")).unwrap(),
        vec![version("1.0.0")]
    );
    assert_eq!(
        resolve_pin(&fs, &paths, &name("demo"), None).unwrap(),
        Pin::Version(version("1.0.0"))
    );
    assert!(fs.actions().is_empty());
}

#[cfg(unix)]
#[test]
fn symlink_entries_are_ignored_and_requested_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;

    let (_temp, paths) = setup();
    install(&paths, "demo", "1.0.0");
    symlink(
        paths.skill_versions(&name("demo")),
        paths.store().join("alias"),
    )
    .unwrap();
    symlink(
        paths.version_dir(&name("demo"), &version("1.0.0")),
        paths.skill_versions(&name("demo")).join("9.0.0"),
    )
    .unwrap();
    symlink("missing", paths.skill_versions(&name("demo")).join("8.0.0")).unwrap();
    assert_eq!(list_skills(&RealFs, &paths).unwrap(), vec![name("demo")]);
    assert_eq!(
        list_versions(&RealFs, &paths, &name("demo")).unwrap(),
        vec![version("1.0.0")]
    );
    assert!(list_versions(&RealFs, &paths, &name("alias")).is_err());
    let pin = Pin::Version(version("9.0.0"));
    assert!(resolve_pin(&RealFs, &paths, &name("demo"), Some(&pin)).is_err());
    fs::create_dir(paths.drafts()).unwrap();
    symlink(
        paths.store_dir(&name("demo"), &version("1.0.0")),
        paths.draft_dir(&name("demo")),
    )
    .unwrap();
    assert!(resolve_pin(&RealFs, &paths, &name("demo"), Some(&Pin::Draft)).is_err());
}
