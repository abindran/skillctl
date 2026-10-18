# skillctl

`skillctl` is a single Rust binary that keeps every [Agent Skill](https://agentskills.io) **once** on your machine and **symlinks** it into the project folders that need it, so Claude Code, Cursor and ZohoCode all see the same skills without anything being committed to the project's Git history.

- One copy of each skill version in `~/.skills/store/`, sealed read-only and content-hashed.
- Projects are named groups of pinned skills. A project can have many folders (clones, copies, worktrees); all of them get the same links.
- Links are hidden through `.git/info/exclude`, which is never pushed.
- A built-in validator checks skills against the Agent Skills specification, organisation rules and harness compatibility, with text, JSON and JUnit output.
- Everything is scriptable: every prompt has a flag, every mutating command has `--dry-run`, and every command has `--json`.

The technical architecture lives in the [project wiki](https://github.com/abindran/skillctl/wiki). This file covers use cases, usage and building the binary.

## Contents

- [Use cases](#use-cases)
- [How it works](#how-it-works)
- [Building the executable](#building-the-executable)
- [Quick start](#quick-start)
- [Command reference](#command-reference)
- [Global flags and exit codes](#global-flags-and-exit-codes)
- [Validation](#validation)
- [Configuration](#configuration)
- [Home folder layout](#home-folder-layout)
- [Development](#development)
- [Implementation status](#implementation-status)

## Use cases

| Need | How skillctl covers it |
|---|---|
| **Write a skill once, use it in every harness.** | `attach` links the project's skills into both `.agents/skills/` (Cursor, ZohoCode) and `.claude/skills/` (Claude Code, Cursor). The link targets are a setting, so a new harness folder is a config change, not a rewrite. |
| **Do not keep duplicate copies per harness or per clone.** | Each skill version exists exactly once in `~/.skills/store/`. Projects and harness folders only hold symlinks to it. |
| **Project-specific skills that follow the project to every clone.** | A project is a registry entry with pinned skill versions. Attach any number of folders to it; `add` and `remove` reconcile all of them, and `relink` restores links after `git clean -fdx` or a fresh checkout. |
| **Keep project repositories clean.** | Links are listed individually in `.git/info/exclude`, so `git status` stays empty and project-owned skills in the same folder remain visible to Git. |
| **Validate skills against open standards.** | `validate` implements the Agent Skills spec rules (S001–S008), organisation quality rules (O101–O133) and harness-compatibility warnings (H201–H205). JUnit output plugs into CI merge-request widgets. |
| **Skills that bundle scripts, references and assets.** | The validator checks that every bundled file is referenced, that scripts have a shebang and executable bit, and that no script downloads to a shell, asks for interactive input, or embeds secrets. Harnesses run the scripts; skillctl only lints them. |
| **Skills that are safe from accidental edits by an AI agent.** | Sealed store versions are `0444`/`0555`. Editing happens in a writable draft under `~/.skills/drafts/`, which can be linked into a project with `add <skill> --draft` for trying out before sealing. |
| **Personal skills available in every folder.** | `global add` pins a skill in the registry's `global` section and links it into `~/.claude/skills/`. Project and global names cannot clash, so a personal skill never silently shadows a project one. |

## How it works

```
~/.skills/store/org-code-review/1.4.0/org-code-review   (read-only, hashed)
        ▲                       ▲
        │ symlink               │ symlink
~/work/crm-web/.agents/skills/org-code-review
~/work/crm-web/.claude/skills/org-code-review
~/work/crm-web/.git/info/exclude   ← managed block hides both links
```

1. `init` creates `~/.skills/` with a `registry.yml`, an empty store, a drafts folder and a `SKILL.md` template.
2. `new` scaffolds a draft; `seal` validates it, copies it into the store under its `metadata.version`, makes it read-only and writes `meta.json` with a SHA-256 content hash.
3. `attach` registers the current folder as an instance of a project, records which skills the project pins, and runs the reconcile algorithm, which creates, updates or removes only symlinks that point into the store or drafts. Real folders and foreign links are never touched and are reported as conflicts.
4. `status`, `projects` and `skills` are read-only views over the registry and the store.

All state is under one home folder (`~/.skills/`, overridable with `SKILLCTL_HOME` or `--home`). Project folders only ever receive symlinks.

## Building the executable

### Prerequisites

- Rust toolchain **1.85 or newer** (edition 2024). Install with [rustup](https://rustup.rs).
- `git` **2.31 or newer** on `PATH` at run time. skillctl uses `git rev-parse --path-format=absolute` to find the per-repository exclude file, including for linked worktrees.
- macOS or Linux. Windows is out of scope for v1.

### Build from source

```bash
git clone https://github.com/abindran/skillctl.git
```

```bash
cd skillctl && cargo build --release --locked
```

The optimised binary is written to `target/release/skillctl` (about 3 MB; the release profile uses thin LTO, a single codegen unit and symbol stripping). Copy it anywhere on your `PATH`, or let Cargo install it into `~/.cargo/bin`:

```bash
cargo install --path . --locked
```

### Universal macOS binary

To ship one binary for Apple silicon and Intel Macs:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

```bash
cargo build --release --locked --target aarch64-apple-darwin && cargo build --release --locked --target x86_64-apple-darwin && lipo -create -output skillctl target/aarch64-apple-darwin/release/skillctl target/x86_64-apple-darwin/release/skillctl
```

A binary that will be downloaded through a browser should additionally be signed with a Developer ID certificate (`codesign --options runtime --timestamp`) and notarised (`xcrun notarytool submit … --wait`). Binaries installed with `cargo install` or from a Homebrew tap do not need notarisation to run.

### Verify the build

```bash
skillctl --version
```

```
skillctl 0.1.0 (commit 7e88fa5422b0, registry schema 1)
```

```bash
skillctl --help
```

### Install from crates.io

The crate is published as `skillctl-cli` (the name `skillctl` on crates.io belongs to an unrelated project); the installed binary is `skillctl`.

```bash
cargo install skillctl-cli --locked
```

Then:

```bash
skillctl init && skillctl completions zsh > ~/.zfunc/_skillctl
```

Every tagged release also publishes a prebuilt universal macOS tarball and its SHA-256 on the [releases page](https://github.com/abindran/skillctl/releases); unpack it anywhere on your `PATH`.

## Quick start

Initialise the home folder. Running it twice is safe; existing files are left alone.

```bash
skillctl init
```

```
Initialized /Users/me/.skills
```

Create a draft skill. The draft is a normal writable folder; open `SKILL.md` in your editor and fill in the description and instructions.

```bash
skillctl new demo
```

```
Created draft demo at /Users/me/.skills/drafts/demo
```

Validate the draft, then seal it into the store. Sealing refuses on validation errors and leaves the draft untouched if anything fails.

```bash
skillctl validate demo@draft
```

```bash
skillctl seal demo
```

```
Sealed demo at /Users/me/.skills/store/demo/0.1.0
```

Attach a project folder. Inside a Git repository this creates the links and a managed block in `.git/info/exclude`, so `git status --porcelain` stays empty.

```bash
cd ~/work/crm-web && skillctl attach --new web --skills demo
```

```
Attached /Users/me/work/crm-web to web
  Linked /Users/me/work/crm-web/.agents/skills/demo
  Linked /Users/me/work/crm-web/.claude/skills/demo
```

Without flags, `attach` is interactive: it lists existing projects with their instance and skill counts, offers to create a new project, and shows a multi-select of store skills with the latest version preselected. When stdin is not a terminal and an answer is missing, it exits with code 2 and names the flag that would have answered.

Check the folder from anywhere inside it (subfolders resolve to the owning instance):

```bash
skillctl status
```

```
Project: web
Instance: /Users/me/work/crm-web
  demo@0.1.0 .agents/skills: ok
  demo@0.1.0 .claude/skills: ok
```

Attach a second clone of the same project and add a skill from either folder; both folders receive the link:

```bash
cd ~/experiments/crm-web-copy && skillctl attach --project web
```

```bash
skillctl add org-code-review@1.4.0
```

Repair links after `git clean -fdx`, a fresh checkout or a change of link targets:

```bash
skillctl relink
```

Detach a folder. Managed links, the exclude block and any folders skillctl itself created (if now empty) are removed; committed project content is untouched.

```bash
skillctl detach
```

List what is in the store and who uses it:

```bash
skillctl skills
```

```
demo
  0.1.0 — web
```

Install skills published in a Git repository, for example an organisation skills repo tagged per release. Several skills in one repository prompt for a selection; `--all` takes them all.

```bash
skillctl install git@gitlab.example.com:platform/org-skills.git --ref v1.4.0 --all
```

```
Installed org-code-review@1.4.0 at /Users/me/.skills/store/org-code-review/1.4.0
Installed payments-deploy@0.3.0 at /Users/me/.skills/store/payments-deploy/0.3.0
```

Check the whole setup, and let skillctl repair what it safely can:

```bash
skillctl doctor --fix
```

```
No problems found.
```

## Command reference

Every command that asks a question also accepts flags that answer it, so all of them can run non-interactively. Commands that change state take an exclusive advisory lock on `~/.skills/registry.lock` for their whole run; read-only commands take a shared lock. A second process waits up to 10 seconds.

### Setup

| Command | Run from | What it changes |
|---|---|---|
| `skillctl init` | anywhere | Creates `~/.skills/` with `registry.yml`, `store/`, `drafts/` and `templates/default/SKILL.md`. Idempotent. Every other command runs an implicit `init` if the home folder is missing. |

### Projects and instances

| Command | Run from | What it changes |
|---|---|---|
| `skillctl attach [--project <name>] [--new <name>] [--skills a,b@1.2.0] [--top-level \| --here]` | instance folder | Registers the canonical current folder as an instance of an existing (`--project`) or new (`--new`) project, moves it if it is already attached elsewhere, refuses nested instances, writes the registry, then reconciles this folder. Inside a Git subfolder it offers the repository root instead; `--top-level` or `--here` answers that question without a terminal. |
| `skillctl detach [--keep-links] [--delete-project]` | instance folder | Removes the instance from its project, deletes managed links in every link target, removes the exclude block and deletes recorded empty folders. `--keep-links` skips all filesystem cleanup. `--delete-project` removes the project when its last instance leaves. |
| `skillctl status` | instance folder | Prints the project, the instance path, each pinned skill with per-target link health, any conflicts, and global skills under a separate heading. Outside an instance prints `Not attached` and exits 0. |
| `skillctl projects` | anywhere | Lists projects, their pinned skill count and every instance, marking folders that no longer exist. |

### Pins and links

| Command | Run from | What it changes |
|---|---|---|
| `skillctl add <skill>[@<version>] [--project <name>] [--draft]` | instance folder | Pins a skill in the current (or named) project and reconciles every instance of it. Without a version, pins the highest stable store version. `--draft` links the writable draft instead. Re-adding with a different version changes the pin. Fails with a hint if the skill is not in the store. |
| `skillctl remove <skill> [--project <name>]` | instance folder | Unpins the skill and reconciles every instance, deleting its links. |
| `skillctl relink [--project <name>] [--all]` | instance folder or anywhere | Reconciles the current instance, every instance of one project, or everything. `--all` reconciles global skills first and cleans links in link targets that were removed from the settings. |
| `skillctl global add <skill>[@<version>]` | anywhere | Pins a skill globally and links it into the user-level folders (`~/.claude/skills/` by default). Refuses a name that any project pins. |
| `skillctl global remove <skill>` | anywhere | Unpins and removes the user-level links. |
| `skillctl global list` | anywhere | Shows each global skill, its version and per-folder link health. |

### Authoring

| Command | Run from | What it changes |
|---|---|---|
| `skillctl new <skill> [--from-template <name>]` | anywhere | Scaffolds `~/.skills/drafts/<skill>/` from `templates/<name>/SKILL.md` (default template by default) with the name filled in, `metadata.version: 0.1.0`, and empty `scripts/` and `references/` folders. Never replaces an existing draft. |
| `skillctl seal <skill> [--bump major\|minor\|patch]` | anywhere | Runs the full validator with the registry allow-list; errors leave the draft intact. Stages a read-only copy under `store/<skill>/<version>/<skill>/`, writes `meta.json` with `source.kind: local`, then deletes the draft. Fails if the version already exists. |
| `skillctl edit <skill>[@<version>] [--force]` | anywhere | Copies a stored version (default: highest stable) into a writable draft with its patch version bumped, restoring write permission on the copy. Refuses to replace an existing draft unless `--force` is given. Link the draft into a project with `add <skill> --draft`, then `seal` it as the next version. |

### Store

| Command | Run from | What it changes |
|---|---|---|
| `skillctl skills` | anywhere | Lists every skill and version in the store with the projects pinning it and whether it is global. |
| `skillctl install <source> [--ref <tag>] [--path <subdir>] [--all] [--version <semver>]` | anywhere | Installs from a Git URL (`git@…`, `https://…`, `ssh://…`, `file://…`, shallow-cloned with `--depth 1` using your own SSH keys and credential helpers) or from an existing local folder. Discovers every folder containing `SKILL.md` (or only `--path`), asks which to install when there are several (`--all` answers), validates each with the registry allow-list, and stages the selected ones with `meta.json` recording the URL, ref, commit and subfolder. The same version with the same content reports "already installed" and exits 0; the same version with different content is refused. `--version` fills in a missing `metadata.version` in the stored copy. |
| `skillctl gc [--keep <n>]` | anywhere | Lists store versions that no project or global pin uses and that are not among a skill's newest `n` (default 1), asks for confirmation (`--yes` answers), restores write permission and deletes them. Empty skill folders are removed too. |

### Quality and maintenance

| Command | Run from | What it changes |
|---|---|---|
| `skillctl validate [<skill>[@<version>\|@draft] \| <path>] [--format text\|json\|junit] [--strict] [--with-reference]` | anywhere | Validates one store version, a draft, any folder, or with no argument every draft and every store version. Nothing is written. |
| `skillctl doctor [--fix]` | anywhere | Reports, each with the command that fixes it: a missing or too-old `git`; instance folders that no longer exist or were replaced; pinned versions missing from the store; missing, wrong, dangling or conflicting links in every instance and user-level folder; store versions whose content no longer matches `meta.json` or that gained write permission; personal copies in `~/.claude/skills`, `~/.agents/skills` or `~/.cursor/skills` that shadow a project skill; symlinked or repository-resident global targets; repositories with `core.symlinks=false`. `--fix` removes vanished instances from the registry, relinks everything and restores read-only modes. Exits 1 while problems remain. |
| `skillctl config get\|set <key> [<value>]` | anywhere | Reads or changes `link_targets`, `global_link_targets`, `git_exclude`, `global_in_instances` and `validate.allow` (lists are comma-separated). After changing link targets it offers to run `relink --all` (`--yes` answers), which also removes managed links from targets that were dropped. |
| `skillctl completions <shell>` | anywhere | Prints a completion script for bash, zsh, fish, elvish or PowerShell. Touches nothing else. |

## Global flags and exit codes

| Flag | Effect |
|---|---|
| `--home <path>` | Use another home folder (same as `SKILLCTL_HOME`). |
| `-y`, `--yes` | Accept every confirmation prompt. |
| `--json` | Machine-readable output on stdout; messages go to stderr. |
| `--dry-run` | Print the actions a command would take without changing anything. Reads still happen, so the plan reflects the real filesystem. |
| `-q`, `--quiet` / `-v`, `--verbose` | Less or more output. Mutually exclusive. |

| Exit code | Meaning |
|---|---|
| 0 | Success |
| 1 | Command failed (I/O error, invalid state, not implemented) |
| 2 | Usage error, or a required answer was missing in non-interactive mode |
| 3 | Validation found errors |
| 4 | Finished, but some links were skipped as conflicts |

Errors print one `Error:` line and one `Hint:` line naming the command to run next. With `--json` the same information is emitted as a JSON object.

## Validation

`skillctl validate` runs three rule groups. Each rule has a stable ID, a default severity and a fix hint.

| Group | IDs | Severity | Covers |
|---|---|---|---|
| Agent Skills spec | S001–S008 | error | `SKILL.md` present; YAML frontmatter is a mapping; `name` matches `^[a-z0-9]+(-[a-z0-9]+)*$` and the folder name; `description` is 1–1024 characters; `compatibility`, `metadata`, `license`, `allowed-tools` have the right types. |
| Organisation rules | O101–O133 | mixed | `metadata.version` is semver; description length and "use when" clause; no `<`/`>`; body and reference-file size budgets; every referenced path exists and every bundled file is referenced; no symlinks or `..` escapes; scripts have a shebang and executable bit, no interactive input, no `curl … \| sh` or `sudo`, no likely secrets. |
| Harness compatibility | H201–H205 | warning | Claude-only frontmatter fields, `allowed-tools`, Claude substitutions in the body, `context: fork` combined with asking the user, unknown fields. |

Examples:

```bash
skillctl validate
```

```bash
skillctl validate org-code-review@1.4.0 --strict --format json
```

```bash
skillctl validate ./skills/org-code-review --format junit > validation.xml
```

- `--strict` promotes remaining warnings to errors. Exit code 3 on any error, 0 otherwise.
- `--format` overrides the global `--json` flag. `--quiet` hides the text report but keeps machine-readable reports intact.
- Failure reports are always exactly one JSON array or one XML document, so they can be redirected straight into a CI artifact. For GitLab, point `artifacts:reports:junit` at the redirected file.
- `--with-reference` additionally runs the reference `skills-ref validate` tool if it is on `PATH` and merges a failed verdict as error `R001`. Missing tools are ignored.

Suppress organisation or harness rules per skill with a frontmatter field, or for every validation run through the registry. Spec rules cannot be suppressed.

```yaml
# SKILL.md
metadata:
  version: "1.4.0"
  skillctl-allow: "O102,H201"
```

```yaml
# ~/.skills/registry.yml
settings:
  validate:
    allow: [O102, H201]
```

## Configuration

All settings live in `~/.skills/registry.yml`. Read or change them with `config get <key>` and `config set <key> <value>`; editing the file by hand and running `skillctl relink --all` works too.

```yaml
schema: 1
settings:
  link_targets: [".agents/skills", ".claude/skills"]   # folders linked inside every instance
  git_exclude: true                                     # write the managed block into .git/info/exclude
  global_link_targets: ["~/.claude/skills"]             # user-level folders for global skills
  global_in_instances: false                            # also link global skills into every instance
  validate:
    allow: []                                           # org/harness rule IDs suppressed everywhere
global:
  skills: {}
projects: {}
```

| Link target | Read by | Default |
|---|---|---|
| `.agents/skills/` | Cursor, ZohoCode | on |
| `.claude/skills/` | Claude Code, Cursor | on |
| `.zcode/skills/` | ZohoCode | off |
| `.cursor/skills/` | Cursor | off |

If a harness lists a skill twice because two targets resolve to the same store folder, drop one target and let `config set` relink:

```bash
skillctl --yes config set link_targets .claude/skills,.zcode/skills
```

Dropped targets are remembered until the next conflict-free `relink --all`, so their managed links get cleaned up.

Shell completions:

```bash
skillctl completions zsh > "$(brew --prefix)/share/zsh/site-functions/_skillctl"
```

## Home folder layout

```
~/.skills/
├── registry.yml                 projects, instances, settings: the only mutable state
├── registry.lock                advisory lock for concurrent runs
├── store/
│   └── org-code-review/
│       └── 1.4.0/
│           ├── meta.json        source, content hash, installed_at, skillctl_version
│           └── org-code-review/ the skill itself, read-only (0444 files, 0555 dirs and executables)
├── drafts/
│   └── org-code-review/         writable copy created by `new`
└── templates/
    └── default/SKILL.md         scaffold used by `new`
```

The skill folder sits one level below the version folder so that its own folder name equals the skill `name`, which the spec and Cursor require. `meta.json` sits beside it so harnesses never see it.

## Development

```bash
cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
```

- Unit tests live in `src/**/tests.rs` and `#[cfg(test)]` modules; integration tests in `tests/*.rs` drive the compiled binary through `CARGO_BIN_EXE_skillctl`.
- Every test sets `SKILLCTL_HOME` to a temporary folder and uses explicit temporary global targets, so tests never touch your real store or `~/.claude/skills/`.
- CI (`.github/workflows/ci.yml`) runs formatting, Clippy and the locked test suite on Ubuntu and macOS 14.

## Releasing

Changes are tracked in [CHANGELOG.md](CHANGELOG.md) (Keep a Changelog format). Add entries under **Unreleased** as you go; when cutting a release, rename that heading to the version and date. A release is a Git tag `vX.Y.Z` whose version matches `Cargo.toml` **and** has a `## [X.Y.Z]` section in the changelog, which becomes the GitHub release notes. Pushing the tag runs [release.yml](.github/workflows/release.yml):

1. **Checks** on macOS: `cargo fmt --check`, Clippy with `-D warnings`, the locked test suite.
2. **Build**: refuses a tag without a changelog section; both Apple targets, joined with `lipo` into one universal binary; `--version` records the tagged commit. When signing secrets are present the binary is signed with the Developer ID certificate (`codesign --options runtime --timestamp`) and notarized with `notarytool --wait`.
3. **Verify** on a separate, clean macOS runner: checksum, unpack, run the end-to-end scenario in `scripts/release/smoke.sh` (init, author, seal, attach two clones, `git clean`, relink, doctor, gc, detach with an unchanged repository).
4. **Publish** a GitHub release carrying `skillctl-X.Y.Z-macos-universal.tar.gz` and its `.sha256`.
5. **Publish the crate** `skillctl-cli` to crates.io with `cargo publish --locked`, when the `CARGO_REGISTRY_TOKEN` secret is configured (a dry-run publish is part of the build step regardless).
6. **Verify the crate** on another clean runner: `cargo install skillctl-cli --locked --version X.Y.Z`, then the same scenario again.

To cut a release, move the **Unreleased** notes in `CHANGELOG.md` under a new `## [0.2.0] - YYYY-MM-DD` heading, then:

```bash
sed -i '' 's/^version = ".*"/version = "0.2.0"/' Cargo.toml && cargo build --locked && scripts/release/changelog-notes.sh 0.2.0 && git commit -am "Release 0.2.0" && git tag v0.2.0 && git push origin main v0.2.0
```

Optional repository configuration:

| Setting | Kind | Purpose |
|---|---|---|
| `APPLE_CERTIFICATE_P12`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | secrets | Developer ID Application certificate (base64 `.p12`), its password and the identity string |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | secrets | Notarization account, team and app-specific password |
| `CARGO_REGISTRY_TOKEN` | secret | crates.io API token with publish rights for `skillctl-cli` (create one at https://crates.io/settings/tokens) |

Without the signing secrets the release is unsigned, which is fine for `cargo install`; only binaries downloaded through a browser hit Gatekeeper quarantine. Without the crates.io token the GitHub release is still created and the crate job logs a warning; re-run the workflow for the tag after adding the token:

```bash
gh workflow run release.yml --ref v0.1.0
```

Publishing by hand works too: `cargo login` once, then `cargo publish --locked` from the tagged commit.

The same steps run locally:

```bash
scripts/release/build-universal.sh && scripts/release/package.sh 0.1.0 && scripts/release/smoke.sh dist/skillctl
```

## Implementation status

| Area | Status |
|---|---|
| CLI, errors, output, names, paths, services (`Fs`, `Prompter`, `Git`) | Implemented |
| Registry model, locking, atomic writes with backup, `init` | Implemented |
| Store: queries, `meta.json`, content hash, staging, read-only sealing | Implemented |
| Authoring: `new`, `seal`, `add --draft` | Implemented |
| Linking: instance resolution, reconcile, Git exclude block | Implemented |
| `attach`, `detach`, `status`, `add`, `remove`, `relink`, `global add/remove/list`, `projects`, `skills` | Implemented |
| Validation: S, O and H rules; text, JSON and JUnit; allow-lists; `--strict`; `--with-reference` | Implemented |
| `install` from Git and local folders, `edit`, `doctor --fix`, `gc`, `config`, `completions` | Implemented |
| Release pipeline: universal build, optional signing and notarization, clean-runner verification, GitHub release, crates.io publish and verification | Implemented (GitHub Actions) |
| Manual harness compatibility matrix (Claude Code, Cursor, ZohoCode) | Not run; default link targets are provisional |

## License

Apache License, Version 2.0. See [LICENSE](LICENSE).
