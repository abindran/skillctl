use super::*;
use crate::services::fs::{DryRunFs, FsAction, RealFs};

fn instance(path: PathBuf) -> Instance {
    Instance {
        path,
        attached_at: "2026-10-18T09:12:00Z".parse().unwrap(),
        created_dirs: vec![
            ".agents".parse().unwrap(),
            ".agents/skills".parse().unwrap(),
        ],
    }
}

fn populated(root: &Path) -> Registry {
    let mut registry = Registry {
        settings: Settings {
            link_targets: vec![".zcode/skills".parse().unwrap()],
            git_exclude: false,
            global_link_targets: vec![
                "~/.agents/skills".parse().unwrap(),
                root.join("global").try_into().unwrap(),
            ],
            global_in_instances: true,
            validate: ValidationSettings {
                allow: vec!["O102".into(), "H201".into()],
            },
            previous_link_targets: vec![".claude/skills".parse().unwrap()],
        },
        ..Registry::default()
    };
    registry
        .global
        .skills
        .insert("org-global".parse().unwrap(), "2.0.1".parse().unwrap());
    registry.projects.insert(
        "web".parse().unwrap(),
        Project {
            skills: BTreeMap::from([
                (
                    "org-review".parse().unwrap(),
                    "1.4.0-beta.1+build.2".parse().unwrap(),
                ),
                ("org-draft".parse().unwrap(), Pin::Draft),
            ]),
            instances: vec![instance(root.join("web")), instance(root.join("web-copy"))],
        },
    );
    registry
}

#[test]
fn every_field_round_trips_through_yaml_and_disk() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let registry = populated(&root);
    let yaml = registry.to_yaml().unwrap();
    assert_eq!(Registry::from_yaml(yaml.as_bytes()).unwrap(), registry);
    let path = root.join("registry.yml");
    registry.save(&RealFs, &path).unwrap();
    assert_eq!(Registry::load(&RealFs, &path).unwrap(), registry);
    assert!(!Registry::backup_path(&path, 1).exists());
    assert!(yaml.find("org-draft").unwrap() < yaml.find("org-review").unwrap());
}

#[test]
fn defaults_apply_to_missing_sections_and_individual_settings() {
    let registry = Registry::from_yaml(b"schema: 1\n").unwrap();
    assert_eq!(registry, Registry::default());
    let settings = &registry.settings;
    assert_eq!(
        settings
            .link_targets
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [".agents/skills", ".claude/skills"]
    );
    assert_eq!(
        settings.global_link_targets[0].to_string(),
        "~/.claude/skills"
    );
    assert!(settings.git_exclude);
    assert!(!settings.global_in_instances);
    let partial = Registry::from_yaml(
        b"schema: 1\nsettings:\n  git_exclude: false\nprojects:\n  empty: {}\n",
    )
    .unwrap();
    assert!(!partial.settings.git_exclude);
    assert_eq!(partial.settings.link_targets, settings.link_targets);
    assert_eq!(partial.projects["empty"], Project::default());
}

#[test]
fn pins_are_plain_strings_and_reject_invalid_versions() {
    for value in ["draft", "1.4.0", "1.0.0-rc.1+build"] {
        let pin: Pin = value.parse().unwrap();
        assert_eq!(serde_json::to_value(&pin).unwrap(), value);
        let yaml = serde_yaml_ng::to_string(&pin).unwrap();
        assert!(!yaml.contains('!'));
        assert_eq!(serde_yaml_ng::from_str::<Pin>(&yaml).unwrap(), pin);
    }
    for invalid in ["latest", "1.2", "v1.2.3", "Draft", "", "^1.0.0"] {
        assert!(invalid.parse::<Pin>().is_err());
    }
    for invalid in ["42", "null", "[draft]", "{version: 1.0.0}"] {
        assert!(serde_yaml_ng::from_str::<Pin>(invalid).is_err());
    }
}

#[test]
fn unknown_schema_is_an_invalid_state_error_with_upgrade_hint() {
    for schema in [0, 2, u32::MAX] {
        let error = Registry::from_yaml(format!("schema: {schema}\nfuture: true\n").as_bytes())
            .unwrap_err();
        assert_eq!(error.exit_code(), 1);
        assert_eq!(error.kind(), "registry");
        assert!(error.hint().contains("Upgrade"));
    }
}

#[test]
fn malformed_fields_names_paths_and_duplicate_keys_are_rejected() {
    for yaml in [
        "",
        "[]",
        "schema: [",
        "projects: {}",
        "schema: 1\nunknown: true",
        "schema: 1\nprojects: {Bad: {}}",
        "schema: 1\nglobal: {skills: {Bad: draft}}",
        "schema: 1\nsettings: {link_targets: [/absolute]}",
        "schema: 1\nsettings: {global_link_targets: [relative]}",
        "schema: 1\nprojects: {web: {}, web: {}}",
        "schema: 1\nglobal: {skills: {demo: draft, demo: 1.0.0}}",
        "schema: 1\nprojects: {web: {instances: [{path: /missing, attached_at: invalid}]}}",
        "schema: 1\nprojects: {web: {instances: [{path: /missing, attached_at: '2026-10-18T09:12:00Z', created_dirs: ['../escape']}]}}",
    ] {
        assert!(
            Registry::from_yaml(yaml.as_bytes()).is_err(),
            "accepted {yaml:?}"
        );
    }
}

#[test]
fn duplicate_and_nested_instances_rejected_in_either_order_and_same_project() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for (a, b) in [("a", "a"), ("a", "a/child"), ("a/child", "a")] {
        for same_project in [true, false] {
            let mut registry = Registry::default();
            registry.projects.insert(
                "one".parse().unwrap(),
                Project {
                    instances: vec![instance(root.join(a))],
                    ..Project::default()
                },
            );
            registry
                .projects
                .entry(if same_project { "one" } else { "two" }.parse().unwrap())
                .or_default()
                .instances
                .push(instance(root.join(b)));
            let bytes = serde_yaml_ng::to_string(&registry).unwrap();
            assert!(Registry::from_yaml(bytes.as_bytes()).is_err());
        }
    }
}

#[test]
fn missing_instances_and_store_versions_remain_loadable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let registry = populated(&root);
    assert!(!root.join("web").exists());
    assert!(registry.validate().is_ok());
}

#[test]
fn global_project_name_clashes_are_rejected() {
    let mut registry = Registry::default();
    let pins = BTreeMap::from([("demo".parse().unwrap(), Pin::Draft)]);
    registry.global.skills = pins.clone();
    registry.projects.insert(
        "web".parse().unwrap(),
        Project {
            skills: pins,
            ..Project::default()
        },
    );
    assert!(registry.validate().is_err());
}

#[test]
fn noncanonical_relative_and_file_instances_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("file");
    std::fs::write(&file, b"not a folder").unwrap();
    for path in [
        PathBuf::from("relative"),
        root.join("../escape"),
        root.join("./missing"),
        root.join("missing//child"),
        file,
    ] {
        let mut registry = Registry::default();
        registry.projects.insert(
            "web".parse().unwrap(),
            Project {
                instances: vec![instance(path)],
                ..Project::default()
            },
        );
        assert!(registry.validate().is_err());
    }
}

#[cfg(unix)]
#[test]
fn existing_symlink_instances_are_not_canonical() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("actual")).unwrap();
    std::os::unix::fs::symlink(root.join("actual"), root.join("alias")).unwrap();
    let mut registry = Registry::default();
    registry.projects.insert(
        "web".parse().unwrap(),
        Project {
            instances: vec![instance(root.join("alias"))],
            ..Project::default()
        },
    );
    assert!(registry.validate().is_err());
    registry.projects.get_mut("web").unwrap().instances[0].path = root.join("actual");
    registry.validate().unwrap();
}

#[test]
fn save_backs_up_exact_previous_bytes_on_every_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("registry.yml");
    let original = b"# user comment\nschema: 1\n";
    std::fs::write(&path, original).unwrap();
    let mut registry = Registry::default();
    registry.settings.git_exclude = false;
    registry.save(&RealFs, &path).unwrap();
    let backup = Registry::backup_path(&path, 1);
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    let previous = std::fs::read(&path).unwrap();
    registry.settings.global_in_instances = true;
    registry.save(&RealFs, &path).unwrap();
    assert_eq!(std::fs::read(&backup).unwrap(), previous);
    assert_eq!(Registry::load(&RealFs, &path).unwrap(), registry);
}

#[test]
fn dry_run_records_backup_then_replacement_and_does_not_write() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("registry.yml");
    let original = b"schema: 1\n";
    std::fs::write(&path, original).unwrap();
    let fs = DryRunFs::new();
    Registry::default().save(&fs, &path).unwrap();
    let actions = fs.actions();
    assert_eq!(actions.len(), 2);
    assert!(
        matches!(&actions[0], FsAction::WriteAtomic { path: p, bytes } if p == &Registry::backup_path(&path, 1) && bytes == original)
    );
    assert!(matches!(&actions[1], FsAction::WriteAtomic { path: p, .. } if p == &path));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(!Registry::backup_path(&path, 1).exists());
}

#[test]
fn invalid_registry_or_failed_backup_never_replaces_destination() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("registry.yml");
    for original in [b"schema: 99\n".as_slice(), b"not: [yaml".as_slice()] {
        std::fs::write(&path, original).unwrap();
        assert!(Registry::default().save(&RealFs, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    let original = b"schema: 1\n";
    std::fs::write(&path, original).unwrap();
    std::fs::create_dir(Registry::backup_path(&path, 1)).unwrap();
    assert!(Registry::default().save(&RealFs, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn missing_registry_is_io_error_and_invalid_model_plans_no_writes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("registry.yml");
    let error = Registry::load(&RealFs, &path).unwrap_err();
    assert!(matches!(error, Error::Io { .. }));
    let invalid = Registry {
        schema: 0,
        ..Registry::default()
    };
    let fs = DryRunFs::new();
    assert!(invalid.save(&fs, &path).is_err());
    assert!(fs.actions().is_empty());
}
