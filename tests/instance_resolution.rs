#![cfg(unix)]

use std::{fs, path::Path};

use chrono::DateTime;
use skillctl::{
    error::Error,
    registry::{Instance, Registry, resolve::InstanceResolver},
};

fn register(registry: &mut Registry, name: &str, path: &Path) {
    registry
        .projects
        .entry(name.parse().unwrap())
        .or_default()
        .instances
        .push(Instance {
            path: fs::canonicalize(path).unwrap(),
            attached_at: DateTime::parse_from_rfc3339("2026-10-18T09:12:00Z")
                .unwrap()
                .into(),
            created_dirs: vec![],
        });
}

#[test]
fn resolves_instance_and_deep_subfolder_with_borrowed_registry_data() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let child = root.join("src/nested");
    fs::create_dir_all(&child).unwrap();
    let mut registry = Registry::default();
    register(&mut registry, "web", &root);
    let resolver = InstanceResolver::new(&registry).unwrap();
    for path in [&root, &child] {
        let resolved = resolver.resolve(path).unwrap().unwrap();
        assert_eq!(resolved.project_name.as_str(), "web");
        let project = registry.projects.values().next().unwrap();
        assert!(std::ptr::eq(resolved.project, project));
        assert!(std::ptr::eq(resolved.instance, &project.instances[0]));
    }
}

#[test]
fn unattached_parent_sibling_and_similar_prefix_do_not_match() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let sibling = temp.path().join("project-copy");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&sibling).unwrap();
    let mut registry = Registry::default();
    register(&mut registry, "web", &root);
    let resolver = InstanceResolver::new(&registry).unwrap();
    for path in [temp.path(), &sibling] {
        assert!(resolver.resolve(path).unwrap().is_none());
    }
    assert!(
        InstanceResolver::new(&Registry::default())
            .unwrap()
            .resolve(&root)
            .unwrap()
            .is_none()
    );
}

#[test]
fn selects_correct_instance_across_projects_and_clones() {
    let temp = tempfile::tempdir().unwrap();
    let mut registry = Registry::default();
    for (folder, project) in [("one", "web"), ("two", "web"), ("three", "api")] {
        let root = temp.path().join(folder);
        fs::create_dir_all(root.join("src")).unwrap();
        register(&mut registry, project, &root);
    }
    let resolver = InstanceResolver::new(&registry).unwrap();
    for (folder, project) in [("one", "web"), ("two", "web"), ("three", "api")] {
        let resolved = resolver
            .resolve(&temp.path().join(folder).join("src"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.project_name.as_str(), project);
        assert_eq!(
            resolved.instance.path,
            fs::canonicalize(temp.path().join(folder)).unwrap()
        );
    }
}

#[test]
fn symlink_alias_walks_physical_parents_not_the_alias_parent() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let outside = temp.path().join("outside");
    fs::create_dir_all(root.join("src/deep")).unwrap();
    fs::create_dir(&outside).unwrap();
    symlink(root.join("src"), outside.join("alias")).unwrap();
    let mut registry = Registry::default();
    register(&mut registry, "web", &root);
    register(&mut registry, "other", &outside);
    let resolved = InstanceResolver::new(&registry)
        .unwrap()
        .resolve(&outside.join("alias/deep"))
        .unwrap()
        .unwrap();
    assert_eq!(resolved.project_name.as_str(), "web");
}

#[test]
fn cached_identity_survives_directory_rename() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("old");
    let new = temp.path().join("new");
    fs::create_dir_all(old.join("src")).unwrap();
    // Registered paths are canonical (/var -> /private/var on macOS); capture before renaming.
    let registered = fs::canonicalize(&old).unwrap();
    let mut registry = Registry::default();
    register(&mut registry, "web", &old);
    let resolver = InstanceResolver::new(&registry).unwrap();
    fs::rename(&old, &new).unwrap();
    // A textual path match or re-reading registered paths would fail here.
    let resolved = resolver.resolve(&new.join("src")).unwrap().unwrap();
    assert_eq!(resolved.project_name.as_str(), "web");
    assert_eq!(resolved.instance.path, registered);
    assert!(
        InstanceResolver::new(&registry)
            .unwrap()
            .resolve(&new)
            .unwrap()
            .is_none()
    );
}

#[test]
fn skips_missing_instances_without_hiding_real_io_errors() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    let present = temp.path().join("present");
    fs::create_dir(&missing).unwrap();
    fs::create_dir(&present).unwrap();
    let mut registry = Registry::default();
    register(&mut registry, "web", &missing);
    register(&mut registry, "web", &present);
    fs::remove_dir(&missing).unwrap();
    let resolver = InstanceResolver::new(&registry).unwrap();
    assert!(resolver.resolve(&present).unwrap().is_some());
    drop(resolver);
    // A symlink loop is not a missing-folder condition.
    std::os::unix::fs::symlink(&missing, &missing).unwrap();
    assert!(matches!(
        InstanceResolver::new(&registry),
        Err(Error::Io { .. })
    ));
}

#[test]
fn invalid_current_folder_is_an_error_not_not_attached() {
    let temp = tempfile::tempdir().unwrap();
    let registry = Registry::default();
    let resolver = InstanceResolver::new(&registry).unwrap();
    assert!(matches!(
        resolver.resolve(&temp.path().join("missing")),
        Err(Error::Io { .. })
    ));
    let file = temp.path().join("file");
    fs::write(&file, "not a directory").unwrap();
    assert!(matches!(resolver.resolve(&file), Err(Error::Usage { .. })));
}

#[test]
fn includes_filesystem_root_in_the_ancestor_walk() {
    let temp = tempfile::tempdir().unwrap();
    let mut registry = Registry::default();
    register(&mut registry, "root", Path::new("/"));
    let resolved = InstanceResolver::new(&registry)
        .unwrap()
        .resolve(temp.path())
        .unwrap()
        .unwrap();
    assert_eq!(resolved.instance.path, Path::new("/"));
}

#[test]
fn rejects_duplicate_file_identities_instead_of_arbitrarily_selecting_project() {
    let temp = tempfile::tempdir().unwrap();
    let mut registry = Registry::default();
    register(&mut registry, "one", temp.path());
    register(&mut registry, "two", temp.path());
    assert!(matches!(
        InstanceResolver::new(&registry),
        Err(Error::Registry { .. })
    ));
}

#[test]
fn differently_cased_alias_matches_on_case_insensitive_filesystems() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("MixedCase");
    fs::create_dir_all(root.join("src")).unwrap();
    let alias = temp.path().join("mixedcase/src");
    if !alias.is_dir() {
        return; // Linux commonly uses a case-sensitive filesystem.
    }
    let mut registry = Registry::default();
    register(&mut registry, "web", &root);
    assert!(
        InstanceResolver::new(&registry)
            .unwrap()
            .resolve(&alias)
            .unwrap()
            .is_some()
    );
}
