//! Public content-hash contract on real temporary skill trees.
#![cfg(unix)]

use std::fs;
use std::io;
use std::os::unix::{ffi::OsStringExt, fs::symlink};
use std::path::Path;

use sha2::{Digest, Sha256};
use skillctl::{
    error::Error,
    services::fs::{DryRunFs, Fs, RealFs},
    store::hash::content_hash,
};

fn write(root: &Path, relative: &str, bytes: &[u8], mode: u32) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    RealFs.set_mode(&path, mode).unwrap();
}

#[test]
fn matches_the_canonical_stream_and_sorts_globally() {
    let temp = tempfile::tempdir().unwrap();
    // Directory-first traversal would put a/z before a.txt, which is wrong.
    write(temp.path(), "a/z", b"\0\xff", 0o755);
    write(temp.path(), "a.txt", b"", 0o644);
    write(temp.path(), "SKILL.md", b"hello", 0o644);
    let stream = b"SKILL.md\x000\x005\x00helloa.txt\x000\x000\x00a/z\x001\x002\x00\0\xff";
    let expected = format!("sha256:{}", hex::encode(Sha256::digest(stream)));
    assert_eq!(content_hash(&RealFs, temp.path()).unwrap(), expected);
}

#[test]
fn empty_tree_matches_the_sha256_empty_stream() {
    let temp = tempfile::tempdir().unwrap();
    let hash = content_hash(&RealFs, temp.path()).unwrap();
    assert_eq!(hash, format!("sha256:{}", hex::encode(Sha256::digest(b""))));
    assert_eq!(hash.len(), 71);
}

#[test]
fn stable_across_runs_creation_order_and_absolute_location() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let entries = [
        ("SKILL.md", b"document".as_slice()),
        ("references/é.md", b"ref"),
        ("assets/image", b"\xff\0"),
    ];
    for (name, bytes) in entries {
        write(first.path(), name, bytes, 0o644);
    }
    for (name, bytes) in entries.into_iter().rev() {
        write(second.path(), name, bytes, 0o644);
    }
    let expected = content_hash(&RealFs, first.path()).unwrap();
    assert_eq!(content_hash(&RealFs, first.path()).unwrap(), expected);
    assert_eq!(content_hash(&RealFs, second.path()).unwrap(), expected);
}

#[test]
fn changes_with_file_bytes_names_additions_and_removals() {
    let temp = tempfile::tempdir().unwrap();
    write(temp.path(), "SKILL.md", b"first", 0o644);
    let original = content_hash(&RealFs, temp.path()).unwrap();
    write(temp.path(), "SKILL.md", b"First", 0o644);
    assert_ne!(content_hash(&RealFs, temp.path()).unwrap(), original);
    write(temp.path(), "SKILL.md", b"first", 0o644);
    fs::rename(temp.path().join("SKILL.md"), temp.path().join("other.md")).unwrap();
    assert_ne!(content_hash(&RealFs, temp.path()).unwrap(), original);
    fs::rename(temp.path().join("other.md"), temp.path().join("SKILL.md")).unwrap();
    write(temp.path(), "empty", b"", 0o644);
    assert_ne!(content_hash(&RealFs, temp.path()).unwrap(), original);
    fs::remove_file(temp.path().join("empty")).unwrap();
    assert_eq!(content_hash(&RealFs, temp.path()).unwrap(), original);
}

#[test]
fn only_executable_permission_bits_affect_identity() {
    let temp = tempfile::tempdir().unwrap();
    write(temp.path(), "run", b"#!/bin/sh\n", 0o644);
    let plain = content_hash(&RealFs, temp.path()).unwrap();
    for mode in [0o744, 0o654, 0o645, 0o755, 0o555] {
        RealFs.set_mode(&temp.path().join("run"), mode).unwrap();
        assert_ne!(content_hash(&RealFs, temp.path()).unwrap(), plain);
    }
    RealFs.set_mode(&temp.path().join("run"), 0o444).unwrap();
    assert_eq!(content_hash(&RealFs, temp.path()).unwrap(), plain);
}

#[test]
fn ignores_empty_directories_and_metadata_outside_skill_root() {
    let temp = tempfile::tempdir().unwrap();
    let skill = temp.path().join("demo");
    fs::create_dir(&skill).unwrap();
    write(&skill, "SKILL.md", b"demo", 0o644);
    let original = content_hash(&RealFs, &skill).unwrap();
    fs::create_dir_all(skill.join("empty/nested")).unwrap();
    fs::write(temp.path().join("meta.json"), b"metadata").unwrap();
    assert_eq!(content_hash(&RealFs, &skill).unwrap(), original);
}

#[test]
fn dry_run_reads_without_mutations() {
    let temp = tempfile::tempdir().unwrap();
    write(temp.path(), "SKILL.md", b"demo", 0o644);
    let dry_run = DryRunFs::new();
    assert_eq!(
        content_hash(&dry_run, temp.path()).unwrap(),
        content_hash(&RealFs, temp.path()).unwrap()
    );
    assert!(dry_run.actions().is_empty());
}

#[test]
fn rejects_file_directory_and_dangling_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    write(temp.path(), "SKILL.md", b"demo", 0o644);
    fs::create_dir(temp.path().join("subdir")).unwrap();
    let link = temp.path().join("link");
    for target in ["SKILL.md", "subdir", "missing", "."] {
        symlink(target, &link).unwrap();
        let error = content_hash(&RealFs, temp.path()).unwrap_err();
        assert!(matches!(error, Error::Validation { .. }));
        assert!(error.to_string().contains("link"));
        fs::remove_file(&link).unwrap();
    }
}

#[test]
fn rejects_symlink_root_and_regular_file_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    fs::create_dir(&root).unwrap();
    let link = temp.path().join("link");
    symlink(&root, &link).unwrap();
    assert!(matches!(
        content_hash(&RealFs, &link),
        Err(Error::Validation { .. })
    ));
    write(temp.path(), "file", b"data", 0o644);
    assert!(matches!(
        content_hash(&RealFs, &temp.path().join("file")),
        Err(Error::Validation { .. })
    ));
}

#[test]
fn missing_root_preserves_io_error_and_path() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    let error = content_hash(&RealFs, &missing).unwrap_err();
    assert!(error.to_string().contains(missing.to_str().unwrap()));
    match error {
        Error::Io { source, .. } => assert_eq!(source.kind(), io::ErrorKind::NotFound),
        other => panic!("expected I/O error, got {other}"),
    }
}

#[test]
fn rejects_non_utf8_paths_without_lossy_collisions() {
    let temp = tempfile::tempdir().unwrap();
    let name = std::ffi::OsString::from_vec(vec![0xff]);
    // APFS refuses non-UTF-8 names (EILSEQ); the rejection path is Linux-only testable.
    if let Err(error) = fs::write(temp.path().join(name), b"data") {
        eprintln!("skipping: filesystem rejects non-UTF-8 names: {error}");
        return;
    }
    let error = content_hash(&RealFs, temp.path()).unwrap_err();
    assert!(error.to_string().contains("UTF-8"));
}

#[test]
fn rejects_special_files_instead_of_ignoring_them() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("socket");
    let _socket = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let error = content_hash(&RealFs, temp.path()).unwrap_err();
    assert!(matches!(error, Error::Validation { .. }));
    assert!(error.to_string().contains("socket"));
}
