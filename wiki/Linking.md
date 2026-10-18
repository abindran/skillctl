# Linking

## Result in a project

```mermaid
flowchart LR
    subgraph Project["~/work/crm-web"]
        A[".agents/skills/org-code-review"]
        C[".claude/skills/org-code-review"]
        X[".git/info/exclude"]
    end
    ST["~/.skills/store/org-code-review/1.4.0/org-code-review"]
    A -- symlink --> ST
    C -- symlink --> ST
    X -. hides links .-> A
    X -. hides links .-> C
```

Links are absolute and point straight at the store, never at each other.

| Link target | Read by | Default |
|---|---|---|
| `.agents/skills/` | Cursor, ZohoCode | On |
| `.claude/skills/` | Claude Code, Cursor | On |
| `.zcode/skills/` | ZohoCode | Off |
| `.cursor/skills/` | Cursor | Off |

Targets are a setting; change with `config set link_targets` then `relink --all`.

## Ownership rule

A path is managed by `skillctl` **only if it is a symlink pointing into the store**. Everything else is never touched. No extra bookkeeping is needed, even if the registry is lost.

## Reconcile

`reconcile()` is the only code path that creates or deletes links.

```mermaid
flowchart TD
    A[Build desired links from project pins] --> B[Remove managed links not desired]
    B --> C{Inspect each desired path}
    C -- missing --> D[linked]
    C -- managed, correct --> E[ok]
    C -- managed, wrong or dangling --> F[atomic replace: updated]
    C -- real folder or foreign link --> G[conflict: skipped]
    D --> H[Update git exclude block]
    E --> H
    F --> H
    G --> H
    H --> I{Any conflict?}
    I -- yes --> J[exit 4]
    I -- no --> K[exit 0]
```

Atomic replace: create `.<name>.skillctl-tmp`, then rename over the old link.

## Instance resolution

Paths are unreliable (case-insensitive APFS, `/tmp` → `/private/tmp`), so folders are matched by **device + inode**: walk from the current folder up to `/`; the first ancestor matching an instance wins.

## Git exclude block

Per-instance managed block in `info/exclude` (never pushed), listing each link individually so project-owned skills stay visible to Git.

```
# >>> skillctl /Users/me/work/crm-web >>>
/.agents/skills/org-code-review
/.claude/skills/org-code-review
# <<< skillctl /Users/me/work/crm-web <<<
```

## Global skills

Pinned in the registry's `global` section and linked into `~/.claude/skills/`. A name cannot be both global and project-pinned. No Git exclude step applies.
