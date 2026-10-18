# skillctl

`skillctl` is a single Rust binary that stores every Agent Skill **once** on your machine and **symlinks** it into the projects that need it. It works for Claude Code, Cursor and ZohoCode, and nothing is committed to project Git history.

## Goals

- One copy of each skill version in `~/.skills/store/`
- Zero Git changes: links are hidden via `.git/info/exclude`
- Interactive setup (`attach`) with a flag for every prompt
- Idempotent linking: `relink` converges on the registry state
- Built-in validator for the Agent Skills spec and org rules
- Install skills from a Git URL

## Non-goals (v1)

Auto-detecting projects, Windows support, syncing registries across machines, a marketplace, running or sandboxing skill scripts.

## Pages

- [Architecture](Architecture)
- [Storage and Registry](Storage-and-Registry)
- [Linking](Linking)
- [Commands and Validation](Commands-and-Validation)
- [Testing and Release](Testing-and-Release)

## Implementation status

| Area | Status |
|---|---|
| CLI definitions, errors, output, names, paths | Done |
| `fs`, `prompt` services, `Context` | Done |
| Registry model and lock, `init`, frontmatter parsing | Done |
| Store queries, metadata, staging, drafts (`new`, `seal`) | Done |
| Spec validation S001–S008, `skills`, `projects` | Done |
| Git service, instance resolution, reconciliation (items 19–21) | Done |
| Git exclude blocks (item 22) | Done |
| Attachment and status (item 23) | Done |
| Project pin changes and relinking (item 24) | Done |
| Remaining commands and release (items 25–33) | Pending |
