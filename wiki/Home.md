# skillctl — Technical Architecture

`skillctl` is a single Rust binary that keeps every Agent Skill **once** on a developer's machine, under `~/.skills/store/`, and **symlinks** it into the project folders that Claude Code, Cursor and ZohoCode scan. The same store holds **hooks**, which skillctl registers once in Claude Code's user settings for both Claude Code and Cursor. Nothing is committed to project Git history: links are hidden through `.git/info/exclude`.

This wiki documents how the tool is built. Usage, use cases and build instructions are in the repository [README](https://github.com/abindran/skillctl#readme).

## Design principles

1. **One copy per skill version.** The store's folder tree is the index; `meta.json` records provenance and a SHA-256 content hash.
2. **Projects only receive symlinks.** A path is managed by skillctl if and only if it is a symlink into the store or drafts. Everything else belongs to the project and is never modified.
3. **Zero Git noise.** Each link is listed individually in the repository's `info/exclude`, inside a per-instance marker block.
4. **Idempotent convergence.** `reconcile()` is the only code path that creates or deletes links, and running it any number of times converges on the registry.
5. **Swappable services instead of mocked OS.** Commands are plain functions over a `Context` holding `Fs`, `Prompter` and `Git` traits; `--dry-run` and tests swap implementations.
6. **Identity, not strings.** Folders are matched to registered instances by device and inode, which survives `/tmp → /private/tmp` and case-insensitive APFS.
7. **Safe state changes.** Advisory locking, atomic writes with `fsync`, registry-before-filesystem ordering, staged installs, and a read-only store.

## Pages

| Page | Covers |
|---|---|
| [Architecture](Architecture) | Crate layout, layers, `Context` and service traits, error and exit-code model, command dispatch, test architecture |
| [Storage and Registry](Storage-and-Registry) | Home folder layout, registry data model and load-time invariants, `meta.json`, canonical content hash, staging and read-only sealing, locking and atomic writes |
| [Linking](Linking) | Link targets, ownership rule, instance resolution, the reconcile algorithm, atomic link replacement, Git exclude block, global skills |
| [Hooks](Hooks) | Hook artifacts (`HOOK.md`), registration into `~/.claude/settings.json` and the `~/.claude/hooks` link folder, how Cursor imports the same registration, the `skillctl-logger` internals, doctor's hook checks |
| [Validation](Validation) | Validation pipeline, `Rule` trait, spec/org/harness rule groups, suppression and strict policy, text/JSON/JUnit reports, reference-validator integration |
| [macOS App](macOS-App) | The SwiftUI front end in `ui/SkillctlApp`: how it drives the CLI's `--json` output, file layout, decisions |

## System context

```mermaid
flowchart LR
    Dev([Developer]) --> CLI[skillctl CLI]
    Dev --> App[Skillctl.app]
    App -- --json --> CLI
    CLI --> Reg[(~/.skills/registry.yml)]
    CLI --> Store[(~/.skills/store)]
    CLI --> Drafts[(~/.skills/drafts)]
    CLI -- symlinks --> Proj[Project instance folders]
    CLI -- symlinks --> UserDir[~/.claude/skills]
    CLI -- symlinks --> HookDir[~/.claude/hooks]
    CLI -- merge --> Settings[(~/.claude/settings.json)]
    CLI -- info/exclude --> GitRepo[(.git of each instance)]
    Proj --> H1[Claude Code]
    Proj --> H2[Cursor]
    Proj --> H3[ZohoCode]
    UserDir --> H1
    UserDir --> H2
    Settings --> H1
    Settings -- third-party import --> H2
    HookDir -. runs .-> Log[(~/.skills/log/events.jsonl)]
```
