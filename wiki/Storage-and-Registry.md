# Storage and Registry

All state lives under `~/.skills/` (override with `SKILLCTL_HOME` or `--home`). Projects only ever receive symlinks.

## Home layout

```mermaid
flowchart TD
    H["~/.skills/"] --> R[registry.yml: only mutable state]
    H --> L[registry.lock: advisory lock]
    H --> S[store/]
    H --> D[drafts/: writable skill copies]
    H --> T[templates/default/SKILL.md]
    S --> N["org-code-review/"]
    N --> V["1.4.0/"]
    V --> M[meta.json]
    V --> K["org-code-review/ (read-only skill)"]
    K --> SM[SKILL.md]
```

The skill folder sits **below** the version folder, so its name equals the skill `name` that harnesses require. `meta.json` sits beside it so harnesses never see it.

## Registry model

```mermaid
erDiagram
    REGISTRY ||--|| SETTINGS : has
    REGISTRY ||--|| GLOBAL : has
    REGISTRY ||--o{ PROJECT : contains
    PROJECT ||--o{ INSTANCE : "has folders"
    PROJECT ||--o{ PIN : "pins skills"
    GLOBAL ||--o{ PIN : "pins skills"
```

- **Pin**: a semver version or `draft`
- **Instance**: one folder on disk (clone, copy, worktree)
- Instance paths are canonical, unique, and never nested
- `BTreeMap` keeps YAML sorted for clean diffs

## Skill lifecycle

```mermaid
stateDiagram-v2
    [*] --> Draft: new / edit
    Draft --> Store: seal (validate, hash, read-only)
    [*] --> Store: install (git or local)
    Store --> Pinned: add / global add
    Pinned --> Store: remove
    Store --> [*]: gc (unpinned old versions)
```

## Safe writes

- **Lock**: exclusive for writers, shared for readers, 10 s wait
- **Atomic**: temp file, `fsync`, rename
- **Order**: write registry first, then reconcile; `relink` repairs partial runs
- **Staged installs**: built in `.staging-*`, renamed only after validation and hashing
- **Read-only store**: dirs `0555`, files `0444`, executables `0555`
- **Content hash**: SHA-256 over sorted files (path, exec bit, length, bytes); `doctor` detects tampering
