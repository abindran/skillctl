# Changelog

All notable changes to skillctl are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

Every release tag `vX.Y.Z` must have a matching `## [X.Y.Z]` section below; the
release workflow refuses to publish without one and uses that section as the
GitHub release notes. Add entries under **Unreleased** as you go and rename the
heading when cutting a release.

## [Unreleased]

## [0.1.0] - 2026-10-18

First release. Published as the crate `skillctl-cli` (the binary is `skillctl`)
with a prebuilt universal macOS tarball attached to the GitHub release.

### Added

- Store and registry: one read-only copy of each skill version under
  `~/.skills/store/`, a YAML registry of projects, instances and pins with
  advisory locking, atomic writes and a backup on every save, and a canonical
  SHA-256 content hash per version.
- Linking: `attach`, `detach`, `status`, `add`, `remove`, `relink` and
  `global add/remove/list`. Links are symlinks into the store, created only
  where skillctl owns the path, repaired idempotently by `reconcile`, hidden
  through per-instance blocks in `.git/info/exclude`, and resolved to their
  project by device and inode rather than by path text.
- Authoring: `new` scaffolds a draft from a template, `edit` copies a stored
  version into a writable draft with a patch bump, `add --draft` links a draft
  for trying out, and `seal` validates, hashes and publishes it read-only.
- Installation: `install` from a Git URL (shallow clone, your own credentials)
  or a local folder, with skill discovery, `--path`, `--all`, `--ref`,
  `--version`, duplicate detection by content hash, and provenance in
  `meta.json`.
- Validation: `validate` with the Agent Skills spec rules S001–S008,
  organisation rules O101–O133 and harness-compatibility warnings H201–H205;
  text, JSON and JUnit reports; per-skill and registry-wide allow-lists;
  `--strict`; optional `--with-reference` against `skills-ref`.
- Maintenance: `doctor [--fix]` for drift between registry, links, store and
  environment; `gc [--keep n]` for unused versions; `config get/set` with
  automatic relinking after link-target changes; `completions <shell>`.
- Every mutating command supports `--dry-run`, every command supports `--json`,
  and every prompt has a flag so commands run non-interactively.
- `--version` prints the version, build commit and supported registry schema.
- Release pipeline on GitHub Actions: checks, universal macOS binary, optional
  signing and notarization, clean-runner end-to-end verification, GitHub
  release with tarball and checksum, crates.io publish and a verification
  install from crates.io.

### Known gaps

- The manual harness compatibility matrix (Claude Code, Cursor, ZohoCode) has
  not been run; the default link targets are provisional.
- `-v/--verbose` has no effect yet, and `--dry-run` prints a summary rather than
  the full recorded action plan.

[Unreleased]: https://github.com/abindran/skillctl/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/abindran/skillctl/releases/tag/v0.1.0
