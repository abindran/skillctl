# Commands and Validation

## Global flags

`--home`, `-y/--yes`, `--json`, `--dry-run`, `-q/--quiet`, `-v/--verbose`. Every prompt has a flag; without a TTY and without the answer, the command exits with code 2.

## Commands

| Group | Commands |
|---|---|
| Setup | `init`, `config get/set`, `completions` |
| Projects | `attach`, `detach`, `status`, `projects` |
| Pins | `add`, `remove`, `relink`, `global add/remove/list` |
| Authoring | `new`, `edit`, `seal` |
| Store | `install`, `skills`, `gc` |
| Quality | `validate`, `doctor [--fix]` |

## Attach flow

```mermaid
sequenceDiagram
    participant U as Developer
    participant C as skillctl attach
    participant R as Registry
    participant L as Linker
    U->>C: run in project folder
    C->>C: canonicalize path, check Git root, check nesting
    C->>U: choose or create project, pick skills
    C->>R: save instance (under lock)
    C->>L: reconcile this instance
    L-->>U: linked / conflicts / exclude file
```

## Install flow

```mermaid
flowchart LR
    S[Git URL or local path] --> CL["git clone --depth 1"]
    CL --> F[Find SKILL.md folders]
    F --> V[Validate]
    V --> ST[Stage in .staging-*]
    ST --> H[Hash + meta.json]
    H --> RO[Read-only, rename into store]
```

Same version with same hash reports "already installed".

## Validation

```mermaid
flowchart LR
    SK[Skill folder] --> P[Parse SKILL.md]
    P --> S[Spec rules S001-S008: errors]
    P --> O[Org rules O101-O133]
    P --> H[Harness rules H201-H205: warnings]
    S --> RP[Report]
    O --> RP
    H --> RP
    RP --> T[text]
    RP --> J[json]
    RP --> X[junit]
```

- Rules have stable IDs, severities and fix hints
- `--strict` turns warnings into errors; exit code 3 on errors
- Spec rules can't be disabled; others via `metadata.skillctl-allow` or `settings.validate.allow`

## Doctor checks

Missing instance folders, wrong or dangling links, missing pinned versions, hash mismatches, writable store files, shadowing skills in user folders, `core.symlinks=false`, missing `git`.
