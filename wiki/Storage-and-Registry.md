# Storage and Registry

All state lives under one home folder: `~/.skills/` by default, overridable with `--home` or `SKILLCTL_HOME`. Project folders only ever receive symlinks.

## Home folder layout

```mermaid
flowchart TD
    H["~/.skills/"] --> R["registry.yml — projects, instances, settings (the only mutable state)"]
    H --> B["registry.yml.bak-1 — previous bytes, written before each save"]
    H --> L["registry.lock — advisory lock file"]
    H --> S["store/"]
    H --> D["drafts/<skill>/ — writable copies for authoring"]
    H --> T["templates/default/SKILL.md — scaffold for new"]
    H --> TH["templates/hook/ — HOOK.md and hook.py, scaffold for new --kind hook"]
    S --> N["store/org-code-review/"]
    N --> ST[".staging-<random>/ — in-progress version, never visible to callers"]
    N --> V["1.4.0/"]
    V --> M["meta.json"]
    V --> K["org-code-review/ — the skill itself, read-only"]
    K --> SM["SKILL.md, scripts/, references/, assets/"]
```

The skill folder sits **one level below** its version folder so that its own folder name is exactly the skill `name`; the spec and Cursor require that `name` match the folder name even when a link is resolved to its real path. `meta.json` sits beside the skill rather than inside it, so harnesses never see it and the validator never flags it as an unreferenced file.

Hooks use the same layout in the same `store/` namespace (`store/<hook>/<version>/<hook>/HOOK.md`), so a skill and a hook cannot share a name. The kind is detected from the root document (`artifact::Kind::detect`) and recorded in `meta.json`; store queries stay directory-based and kind-agnostic.

## Registry data model

```mermaid
erDiagram
    REGISTRY ||--|| SETTINGS : has
    REGISTRY ||--|| GLOBAL : has
    REGISTRY ||--o{ PROJECT : "projects (BTreeMap)"
    PROJECT ||--o{ INSTANCE : "instances (Vec)"
    PROJECT ||--o{ PIN : "skills (BTreeMap)"
    GLOBAL ||--o{ PIN : "skills (BTreeMap)"
    GLOBAL ||--o{ PIN : "hooks (BTreeMap, omitted when empty)"
    SETTINGS ||--|| VALIDATION_SETTINGS : validate
```

```yaml
schema: 1
settings:
  link_targets: [".agents/skills", ".claude/skills"]   # RelPath, linked inside every instance
  git_exclude: true
  global_link_targets: ["~/.claude/skills"]             # HomePath, expanded at use
  global_in_instances: false
  validate:
    allow: []                                           # org/harness rule IDs suppressed globally
  previous_link_targets: []                             # omitted when empty; see Linking
global:
  skills:
    org-commit-message: 2.0.1
  hooks:                                                # linked under ~/.claude/hooks, registered in settings.json
    skillctl-logger: 1.0.0
projects:
  crm-web:
    skills:
      org-code-review: 1.4.0
      payments-deploy: draft
    instances:
      - path: /Users/me/work/crm-web
        attached_at: 2026-10-18T09:12:00Z
        created_dirs: [".agents", ".agents/skills"]
```

| Type | Fields | Notes |
|---|---|---|
| `Registry` | `schema: u32`, `settings`, `global`, `projects: BTreeMap<ProjectName, Project>` | `BTreeMap` keeps YAML sorted so diffs stay readable if the home folder is kept in a personal Git repo. Unknown fields are rejected (`deny_unknown_fields`). |
| `Settings` | `link_targets: Vec<RelPath>`, `git_exclude: bool`, `global_link_targets: Vec<HomePath>`, `global_in_instances: bool`, `validate: ValidationSettings`, `previous_link_targets: Vec<RelPath>` | Defaults apply field by field, so a registry written by an older build loads with new settings defaulted. |
| `Global` | `skills: BTreeMap<SkillName, Pin>`, `hooks: BTreeMap<SkillName, Pin>` | `hooks` is skipped when empty, so registries written before hooks existed round-trip unchanged. A name cannot be in both maps, nor in `hooks` and any project's `skills`. |
| `Project` | `skills: BTreeMap<SkillName, Pin>`, `instances: Vec<Instance>` | |
| `Instance` | `path: PathBuf`, `attached_at: DateTime<Utc>`, `created_dirs: Vec<RelPath>` | `path` is absolute and canonical. `created_dirs` lists folders skillctl created, shallowest first, so `detach` removes only those and only when empty. |
| `Pin` | `Version(semver::Version)` or `Draft` | Serialised as `"1.4.0"` or `"draft"`. |

### Invariants enforced on load

- `schema` is checked **before** any other field is interpreted; an unsupported value aborts with a message to upgrade skillctl rather than a parse error on a newer layout.
- Project and skill names follow the spec pattern, enforced by the newtypes' `Deserialize`.
- Instance paths are absolute, each path appears in at most one project, and no instance path is an ancestor of another, so any folder resolves to exactly one project.
- Duplicate skill keys in one map are rejected rather than silently last-wins.
- A hook name is never also a global or project skill name.
- Pinned versions are **not** required to exist in the store at load time; a missing version is a `doctor` finding, so a damaged store never locks the user out of the registry.

### Save path

`Registry::save` serialises to YAML, copies the previous bytes to `registry.yml.bak-<schema>`, then performs an atomic write through the `Fs` service. A failed serialisation or backup never replaces the destination.

## Store queries

The directory tree **is** the index; queries do not open `meta.json` or `SKILL.md`, so a damaged version stays visible for diagnosis.

| Function | Behaviour |
|---|---|
| `list_skills` | Valid skill-name directories under `store/`, sorted. Files, symlink entries, staging directories and invalid names are ignored; a missing store yields an empty list. |
| `list_versions` | Semver directories under `store/<skill>/`, ascending, including prereleases and build metadata. |
| `latest_stable` | Highest version with no prerelease component. Never falls back to a prerelease: 1.5.0 wins over 1.4.0 and over 1.6.0-beta. |
| `store_dir(name, pin)` | `store/<name>/<version>/<name>` or `drafts/<name>`, computed without touching disk so dangling links can still be reasoned about. |
| `resolve_pin` | Accepts an explicit version, `draft`, or nothing (latest stable), and checks that the target and its managed ancestors are real directories. A missing explicit pin is an error, never a request to pick a different version. |

Queries perform no writes, including in dry runs. Callers hold the registry lock for a consistent view.

## `meta.json`

```json
{
  "name": "org-code-review",
  "version": "1.4.0",
  "kind": "skill",
  "source": { "kind": "git", "url": "git@gitlab.example.com:platform/org-skills.git", "ref": "v1.4.0", "commit": "9f2c1e7", "subdir": "skills/org-code-review" },
  "content_hash": "sha256:4b1f…",
  "installed_at": "2026-10-18T09:05:00Z",
  "skillctl_version": "0.1.0"
}
```

`kind` is `skill` or `hook`; it is optional on read (versions stored before hooks existed have no field and are skills) and always written. `source.kind` is `git`, `local` (installed from a path or sealed from a draft) or `new`. Git sources carry `url` and `commit`, with optional `ref` and repository-relative `subdir`. `Metadata` is read and atomically written through the `Fs` service; missing or malformed metadata is an error, not a default. Because `meta.json` lives beside the skill folder, writing it never changes the content hash.

## Canonical content hash

```mermaid
flowchart LR
    Walk["Walk skill folder (not the version folder, not meta.json)"] --> Sort["Sort files by UTF-8 relative path, '/' separators"]
    Sort --> Stream["For each file: path NUL execbit NUL decimal-length NUL bytes"]
    Stream --> Sha["SHA-256"] --> Out["sha256:&lt;lowercase hex&gt;"]
```

- Any Unix execute bit marks a file executable (`1`); other permission bits, timestamps, the absolute root location and empty directories do not affect the hash. Making a version read-only therefore preserves its identity.
- Symlinks (including the root), special files and non-UTF-8 paths are rejected, so the stream never has to describe them.
- Hashing reads through the `Fs` service and performs no writes.

## Staging and the read-only store

```mermaid
flowchart TD
    Src["Validated source (draft or checkout)"] --> Copy["Copy into store/&lt;skill&gt;/.staging-&lt;random&gt;/&lt;skill&gt;/"]
    Copy --> Hash["Hash the copied bytes; compare with supplied metadata hash"]
    Hash --> Meta["Write meta.json beside the skill"]
    Meta --> Seal["Dirs and executables 0555, other files 0444"]
    Seal --> Rename["Atomic rename to store/&lt;skill&gt;/&lt;version&gt;/"]
    Hash -- mismatch / error --> Clean["Restore write mode, remove staging entry, no version published"]
```

- An existing version directory (even empty, even a symlink) is never replaced.
- Symlinks and special files in the source are rejected; source permissions are never modified; empty directories are retained.
- `restore_write_mode` resets a version to `0755` throughout so a later `gc` can delete it; it refuses symlinks and deletes nothing itself.
- All of this runs through `Fs`, so `seal --dry-run` hashes the planned copy and records creation, writes, mode changes and the final rename without creating a staged folder.

## Drafts

`~/.skills/drafts/<name>/` is an ordinary writable folder. `new` scaffolds it from `templates/<template>/`: the root document (`SKILL.md`, or `HOOK.md` with `--kind hook`, default templates `default` and `hook`) with `{{name}}` replaced and `metadata.version: 0.1.0` set, plus every other regular file in the template folder with `{{name}}` substituted and the exec bit preserved (the hook template ships `hook.py`). Skill drafts also get empty `scripts/` and `references/`. The scaffold is checked with the spec rules (skills) or the frontmatter hook rules (hooks) before anything is written. `edit` copies a stored version into a draft (folders `0755`, files `0644` or `0755`) and bumps the patch version in the copy's frontmatter; an existing draft is replaced only with `--force`. `seal` runs the full validator for the draft's kind, stages the draft into the store with its `kind` and `source.kind: local` (applying `--bump` to the stored copy's frontmatter), and removes the draft only after the version is published. A draft can be pinned with `add <skill> --draft`, in which case links point at the draft folder and `status` shows the pin as `draft`.

## Install flow

```mermaid
flowchart LR
    S["Git URL or local folder"] --> C["Checkout: git clone --depth 1 [--branch ref] into a temp folder"]
    S -. "existing folder" .-> D
    C --> D["discover: folders holding SKILL.md or HOOK.md (with their Kind), or only --path"]
    D --> P["multi-select unless --all"]
    P --> V["validate each with the registry allow-list (through a read overlay when --version fills a missing metadata.version)"]
    V --> H["content_hash of the overlay"]
    H --> X{"version exists?"}
    X -- "same hash" --> A["already installed, exit 0"]
    X -- "different hash" --> E["error: bump the version"]
    X -- no --> ST["stage_version with meta.json (kind; git: url, ref, commit, subdir)"]
```

The clone happens before the exclusive registry lock is taken; everything selected is validated before anything is staged, so a multi-skill install never stops halfway.

## Garbage collection and doctor

`gc` deletes versions that no project pin, global skill pin or global hook pin references and that are not among a skill's newest `--keep` versions. It restores write permission (`restore_write_mode`), removes the version folder, and removes the skill folder when it becomes empty. `doctor` recomputes every stored version's content hash against `meta.json`, walks each version for entries that gained write permission (`--fix` restores `0444`/`0555`), and reports pins whose folders are missing.

`doctor` also runs two advisory checks that are deliberately not problems. `commands/mcp.rs` reads the MCP server configuration of both harnesses (`~/.claude.json` user and per-project `mcpServers` plus `disabledMcpjsonServers`, `.mcp.json`, `~/.cursor/mcp.json`, `.cursor/mcp.json`), scores each server by the `PostToolUse` events the logger recorded for `mcp__<server>__*` tools, and reports `mcp-unused` (warning, after `UNUSED_AFTER_DAYS` of log history) or `mcp-unmeasured` (info) with a CLI alternative where one is known; the result is the `mcp` object of the JSON report. `commands/telemetry.rs` is the observability check: it reads the Claude Code OpenTelemetry variables and probes `PATH` for `ccusage` through injected closures (so the logic is unit-tested without process state), produces `Problem` values with severities `warning` and `info`, and returns them in a separate `telemetry` object. They are rendered above the problem list and never counted in `remaining`, so a machine without telemetry is reported as unmeasured, not broken.

## Locking and atomic writes

| Mechanism | Detail |
|---|---|
| Advisory lock | `RegistryLock` on `registry.lock` via `fd-lock`. Exclusive for writers, shared for readers. A second process waits up to 10 seconds, then fails with a clear message. |
| Atomic write | `Fs::write_atomic`: temporary file in the same directory, `fsync`, rename over the original. Used for the registry, `meta.json` and the Git exclude file. |
| Order of operations | Registry first, filesystem second. A crash between the two leaves a registry that describes the intended state; `relink` converges the filesystem. |
| Staged installs | New versions are built under `.staging-*` and renamed into place only after validation and hashing, so a failed install or seal never leaves a half-written version. |
| Read-only store | Prevents accidental edits through a project link, including edits by an AI agent. Write mode is restored only immediately before deletion. |
