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
- [Hooks](#hooks)
- [Validation](#validation)
- [Instruction files](#instruction-files)
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
| **Validate skills against open standards.** | `validate` implements the Agent Skills spec rules (S001–S008), organisation quality rules (O101–O133), directive-description rules (O140–O145) and harness-compatibility warnings (H201–H205); hooks get their own rules (K101–K111). JUnit output plugs into CI merge-request widgets. |
| **Store hooks next to skills, register them once.** | A hook is a `HOOK.md` declaring lifecycle events plus the script a harness runs. Hooks are authored, validated, versioned and installed exactly like skills, in the same store. `hook add` links one under `~/.claude/hooks/` and merges its events into `~/.claude/settings.json`, which Cursor reads too, so one registration serves both tools. |
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

1. `init` creates `~/.skills/` with a `registry.yml`, an empty store, a drafts folder, a `SKILL.md` template and a hook template.
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
Telemetry (advisory; does not affect the exit status):
  [warning] Claude Code OpenTelemetry export is off (CLAUDE_CODE_ENABLE_TELEMETRY is not 1), so tokens and cost per skill and agent are not recorded
      fix: export CLAUDE_CODE_ENABLE_TELEMETRY=1 OTEL_METRICS_EXPORTER=otlp OTEL_LOGS_EXPORTER=otlp OTEL_EXPORTER_OTLP_PROTOCOL=grpc OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 OTEL_LOG_TOOL_DETAILS=1
  [warning] ccusage is not on PATH; it sums tokens and cost from Claude Code's local session logs
      fix: npm install -g ccusage (or run it ad hoc with npx ccusage)
  [info] Cursor exports no telemetry; its baseline comes from the usage dashboard (cursor.com, Settings, Usage) and from hook logs
      fix: Nothing to set; record Cursor numbers from the dashboard and the hook log
No problems found.
```

The telemetry block is the observability baseline: it tells you whether this machine records the token and cost numbers that every later optimisation is measured against, and it never fails `doctor`.

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
| `skillctl status [--budget]` | instance folder | Prints the project, the instance path, each pinned skill with per-target link health, any conflicts, and global skills under a separate heading. Outside an instance prints `Not attached` and exits 0. With `--budget`, also sizes the skill listing the agent sees here (project pins plus global skills): name and description characters and estimated tokens per skill, the total against 1% of a 200,000-token context window, and any description over 1,536 characters, which harnesses truncate (O143). |
| `skillctl projects` | anywhere | Lists projects, their pinned skill count and every instance, marking folders that no longer exist. |

### Pins and links

| Command | Run from | What it changes |
|---|---|---|
| `skillctl add <skill>[@<version>] [--project <name>] [--draft]` | instance folder | Pins a skill in the current (or named) project and reconciles every instance of it. Without a version, pins the highest stable store version. `--draft` links the writable draft instead. Re-adding with a different version changes the pin. Fails with a hint if the skill is not in the store. After pinning, prints any directive-description warnings (O140–O145) for the skill and the project's index budget (O143) on stderr; the same data is under `advisory` in `--json`. `global add` and `attach --skills` do the same. |
| `skillctl remove <skill> [--project <name>]` | instance folder | Unpins the skill and reconciles every instance, deleting its links. |
| `skillctl relink [--project <name>] [--all]` | instance folder or anywhere | Reconciles the current instance, every instance of one project, or everything. `--all` reconciles global skills first and cleans links in link targets that were removed from the settings. |
| `skillctl global add <skill>[@<version>]` | anywhere | Pins a skill globally and links it into the user-level folders (`~/.claude/skills/` by default). Refuses a name that any project pins. |
| `skillctl global remove <skill>` | anywhere | Unpins and removes the user-level links. |
| `skillctl global list` | anywhere | Shows each global skill, its version and per-folder link health. |

### Authoring

| Command | Run from | What it changes |
|---|---|---|
| `skillctl new <name> [--kind skill\|hook] [--from-template <name>]` | anywhere | Scaffolds `~/.skills/drafts/<name>/` with the name filled in and `metadata.version: 0.1.0`. A skill (the default) comes from `templates/default/SKILL.md`, whose description is a directive template with `[placeholders]` to replace (O144 flags any left in), with empty `scripts/` and `references/` folders. A hook (`--kind hook`) comes from `templates/hook/`: `HOOK.md` plus every other file in the template folder (the `hook.py` entry script, executable), with `{{name}}` substituted. Never replaces an existing draft. |
| `skillctl seal <name> [--bump major\|minor\|patch]` | anywhere | Runs the full validator with the registry allow-list; errors leave the draft intact. Stages a read-only copy under `store/<name>/<version>/<name>/`, writes `meta.json` with `kind` (`skill` or `hook`) and `source.kind: local`, then deletes the draft. Fails if the version already exists. |
| `skillctl edit <name>[@<version>] [--force]` | anywhere | Copies a stored version (default: highest stable) into a writable draft with its patch version bumped, restoring write permission on the copy. Refuses to replace an existing draft unless `--force` is given. Link a skill draft into a project with `add <skill> --draft`, then `seal` it as the next version. |

### Hook commands

| Command | Where | What it does |
|---|---|---|
| `skillctl hook add <hook>[@<version>]` | anywhere | Pins a stored hook in the registry's `global.hooks`, links `~/.claude/hooks/<hook>` to the store version, and registers one entry per declared event in `~/.claude/settings.json` (with the hook's `matchers` and `timeout`). The file is read, merged and written back atomically; your other settings and hand-written hooks are untouched; the previous bytes go to `settings.json.bak`; a second run reports every event as `present` and writes nothing; after a hook upgrade that changed a matcher, timeout or entry script, the existing entries are brought in line and reported as `updated`. A file that is not valid JSON is a hard stop, never overwritten. Cursor loads Claude Code hooks from this file through its third-party imports (on by default), so the hook runs in both tools. |
| `skillctl hook remove <hook>` | anywhere | Unpins the hook, removes its link, and removes only its entries from `settings.json`, pruning events that become empty. |
| `skillctl hook list` | anywhere | Each pinned hook with its pin, link health, declared events and which of them are not registered. |

Hooks never go into `skills/` folders: `add`, `attach --skills` and `global add` refuse a hook and point at `hook add`. `relink --all` and `doctor --fix` restore hook links; `doctor` also reports a pinned hook whose events are missing from `settings.json` (`hook-unregistered`), the same command registered twice or also present in `~/.cursor/hooks.json` (`hook-duplicate`), and an unparsable settings file (`settings-invalid`).

### Store

| Command | Run from | What it changes |
|---|---|---|
| `skillctl skills` | anywhere | Lists every artifact and version in the store with the projects pinning it and whether it is global. Hooks are marked `(hook)` in the text and carry `"kind": "hook"` in `--json`. |
| `skillctl install <source> [--ref <tag>] [--path <subdir>] [--all] [--version <semver>]` | anywhere | Installs from a Git URL (`git@…`, `https://…`, `ssh://…`, `file://…`, shallow-cloned with `--depth 1` using your own SSH keys and credential helpers) or from an existing local folder. Discovers every folder containing `SKILL.md` or `HOOK.md` (or only `--path`), asks which to install when there are several (`--all` answers), validates each with the registry allow-list, and stages the selected ones with `meta.json` recording the kind, URL, ref, commit and subfolder. The same version with the same content reports "already installed" and exits 0; the same version with different content is refused. `--version` fills in a missing `metadata.version` in the stored copy. |
| `skillctl gc [--keep <n>]` | anywhere | Lists store versions that no project or global pin uses and that are not among a skill's newest `n` (default 1), asks for confirmation (`--yes` answers), restores write permission and deletes them. Empty skill folders are removed too. |

### Quality and maintenance

| Command | Run from | What it changes |
|---|---|---|
| `skillctl lint [<folder>] [--format text\|json\|junit] [--strict]` | anywhere | Checks a project's instruction files (`AGENTS.md`, `CLAUDE.md`, `.claude/CLAUDE.md`, `CLAUDE.local.md`, `.cursor/rules/*.mdc`) with rules I301–I305, I308 and I309 and reports each problem with a suggested fix. Read-only: those files are team-owned and changed through review, never by skillctl. Same formats, allow-list and exit code 3 as `validate`. |
| `skillctl validate [<name>[@<version>\|@draft] \| <path>] [--format text\|json\|junit] [--strict] [--with-reference]` | anywhere | Validates one store version, a draft, any folder, or with no argument every draft and every store version. A folder with `SKILL.md` gets the skill rules, one with `HOOK.md` the hook rules. Nothing is written. |
| `skillctl doctor [--fix]` | anywhere | Reports, each with the command that fixes it: a missing or too-old `git`; instance folders that no longer exist or were replaced; pinned versions missing from the store; missing, wrong, dangling or conflicting links in every instance and user-level folder; store versions whose content no longer matches `meta.json` or that gained write permission; personal copies in `~/.claude/skills`, `~/.agents/skills` or `~/.cursor/skills` that shadow a project skill; symlinked or repository-resident global targets; repositories with `core.symlinks=false`. `--fix` removes vanished instances from the registry, relinks everything and restores read-only modes. Exits 1 while problems remain. An **MCP servers** section lists every server the harnesses connect to from this folder (`~/.claude.json` user and per-project entries, `.mcp.json`, `~/.cursor/mcp.json`, `.cursor/mcp.json`) with its command or URL, disabled state and how often the event log has seen its tools called; a server with no recorded call after a week of logging is `mcp-unused` (warning, with a CLI alternative such as `gh` where one exists), earlier it is `mcp-unmeasured` (info). Every connected server's tool definitions are loaded on every turn, so this is where the largest token savings tend to be. A separate **Telemetry** section, which never affects the exit status, says whether Claude Code's OpenTelemetry export is on and complete (`CLAUDE_CODE_ENABLE_TELEMETRY=1`, `OTEL_METRICS_EXPORTER`, `OTEL_LOGS_EXPORTER`, `OTEL_EXPORTER_OTLP_PROTOCOL`, `OTEL_EXPORTER_OTLP_ENDPOINT`, and `OTEL_LOG_TOOL_DETAILS=1` so `agent.name` is not redacted), whether `ccusage` is on `PATH`, that Cursor's usage comes from its dashboard and hook logs and, once a hook is registered, whether the event log has seen Cursor run it (`cursor-imports`: a warning asking you to confirm **Settings → Agents → Third-Party Imports** while no Cursor event has arrived, an info line with the count once one has); `--json` carries it under `telemetry`. |
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

## Hooks

A hook is a script a harness runs on a lifecycle event (a prompt was submitted, a turn ended, the context was compacted, a subagent finished). skillctl stores hooks in the same store as skills, validates them, and registers them with one command for both Claude Code and Cursor.

### Hook artifacts

A hook folder holds `HOOK.md` and the script the harness runs:

```yaml
---
name: skillctl-logger
description: Appends one JSONL line per lifecycle event so compactions and subagent runs can be counted per session.
metadata:
  version: "1.0.0"
events: [SessionEnd, SubagentStop, PreCompact, UserPromptSubmit, PostToolUse]
matchers:
  PostToolUse: Skill
timeout: 10
entry: hook.py
---
```

| Field | Meaning |
|---|---|
| `events` | The Claude Code lifecycle events the hook handles, in Claude Code's spelling. Cursor maps them itself: `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `Stop`, `SubagentStop`, `SessionStart`, `SessionEnd` and `PreCompact` run in both tools; `Notification` and `PermissionRequest` are not supported by Cursor; anything else (`SubagentStart`, `PostCompact`, …) runs in Claude Code only. |
| `matchers` | Optional tool-name pattern per tool event, for example `PostToolUse: Skill` to see only skill invocations. |
| `timeout` | Optional seconds the harness waits (default 10, at most 600). |
| `entry` | The script, relative to the folder. Any interpreter works as long as the file has a shebang and the executable bit. The harness passes the event as JSON on stdin; exit 0 to continue, 2 to block where the event allows it. |

Hooks are validated with rules K101–K111 (see [Validation](#validation)). They share the store's flat namespace with skills, so a skill and a hook cannot have the same name, and they are never linked into a `skills/` folder.

### Writing a hook

```bash
skillctl new --kind hook my-hook
```

This scaffolds `~/.skills/drafts/my-hook/` with a `HOOK.md` and an executable Python `hook.py` that reads the event from stdin and exits 0. Edit both, then:

```bash
skillctl validate my-hook@draft && skillctl seal my-hook
```

To change a sealed hook later, `skillctl edit my-hook` copies it back into a draft with a patch bump; seal again and re-run `hook add` to repoint the link.

### Registering a hook

```bash
skillctl hook add my-hook
```

`hook add` does three things, all idempotent:

1. Pins the hook in the registry's `global.hooks`.
2. Links `~/.claude/hooks/my-hook` to the store version, so upgrading the hook repoints one symlink and never rewrites your settings.
3. Merges one entry per declared event into `~/.claude/settings.json`, with the hook's `matchers` and `timeout`. The command stored is `"$HOME/.claude/hooks/my-hook/<entry>"`. Your other settings and any hooks you wrote by hand are left exactly as they are; the previous bytes are saved to `settings.json.bak` first; a file that is not valid JSON is refused rather than overwritten. Running `hook add` again reports every event as `present` and writes nothing.

Cursor reads Claude Code hooks from `~/.claude/settings.json` through **Settings → Agents → Third-Party Imports** (on by default), so the same registration runs the hook in Cursor. Do not also add it to `~/.cursor/hooks.json`; it would run twice, and `doctor` warns when it finds that.

`skillctl hook list` shows each pinned hook, its link health and any declared event that is not registered. `skillctl hook remove my-hook` unpins it, removes the link and removes only its entries from `settings.json`. `doctor` checks the link, reports `hook-unregistered` and `hook-duplicate`, and `doctor --fix` restores a missing link.

### The skillctl-logger hook

[`hooks/skillctl-logger/`](hooks/skillctl-logger/) is the observability baseline of the LLM optimisation strategy: one JSON line per harness event, from both tools, in one file. Every later measurement (sessions, prompts, compactions, subagent runs, skill activations, MCP tool calls) reads this log.

**Install and register once per machine.** From a checkout of this repository:

```bash
skillctl install ./hooks/skillctl-logger && skillctl hook add skillctl-logger
```

or straight from Git:

```bash
skillctl install https://github.com/abindran/skillctl.git --path hooks/skillctl-logger && skillctl hook add skillctl-logger
```

Claude Code picks the new entries up without a restart. Check that everything is in place:

```bash
skillctl hook list
```

```
skillctl-logger@1.0.0
  /Users/me/.claude/hooks/skillctl-logger: ok
  events: SessionStart, SessionEnd, UserPromptSubmit, PostToolUse, Stop, SubagentStart, SubagentStop, PreCompact, PostCompact (all registered)
```

(`skillctl-logger@1.1.0` at the time of writing; the version follows the hook's `metadata.version`.)

Then send one prompt in Claude Code (or Cursor) and watch the log:

```bash
tail -f ~/.skills/log/events.jsonl
```

**What it records.** The log lives at `$SKILLCTL_HOME/log/events.jsonl` (`~/.skills/log/events.jsonl` by default). Each line is one event:

```json
{"ts":"2026-10-19T13:18:38Z","tool":"claude-code","event":"UserPromptSubmit","session":"cf27…","cwd":"/Users/me/work/crm","transcript":"/Users/me/.claude/projects/…/cf27….jsonl","prompt_chars":711}
```

| Field | Present on | Meaning |
|---|---|---|
| `ts` | all | UTC time the hook ran |
| `tool` | all | `claude-code`, or `cursor` when the payload carries `cursor_version` or `CURSOR_VERSION` is set |
| `event` | all | The event in Claude Code's spelling; Cursor's camelCase names are normalised (`preCompact` → `PreCompact`) so one log is comparable across tools |
| `session` | all | Claude Code's `session_id` or Cursor's `conversation_id` |
| `cwd` | all | The working folder (Claude Code `cwd`, or Cursor's first `workspace_roots` entry) |
| `transcript` | when the harness provides it | Path to the conversation transcript, from which context size can be derived |
| `prompt_chars` | `UserPromptSubmit` | Length of the prompt. The text itself is never recorded. |
| `tool_name`, `skill` | `PostToolUse` | The tool that ran and, for the `Skill` tool, the skill name: this is how skill activations are counted |
| `server` | `PostToolUse` on an MCP tool | The server name from `mcp__<server>__<tool>`; `doctor` uses it to tell which configured servers are actually used |
| `agent`, `agent_id` | subagent events | The subagent type and id. `agent` is absent for the harness's own auxiliary agents (for example the one that titles the session after each turn), which also fire `SubagentStop`; count only lines with `agent` when measuring delegation |
| `trigger` | `PreCompact`, `PostCompact` | `auto` or `manual` |
| `source`, `reason`, `stop_hook_active` | `SessionStart`, `SessionEnd`, `Stop`/`SubagentStop` | Copied from the event when present |

The hook never slows or blocks a turn: it runs in tens of milliseconds, exits 0 on every path, and ignores malformed input or an unwritable log.

**Reading the log.** A few `jq` one-liners that answer the strategy's questions:

```bash
jq -r 'select(.event=="PreCompact") | .session' ~/.skills/log/events.jsonl | sort | uniq -c | sort -rn
```

compactions per session, most first (the "tasks run too long in one context" signal);

```bash
jq -r 'select(.event=="PostToolUse" and .skill) | .skill' ~/.skills/log/events.jsonl | sort | uniq -c
```

skill activations by skill;

```bash
jq -r 'select(.event=="SubagentStop" and .agent) | .agent' ~/.skills/log/events.jsonl | sort | uniq -c
```

real subagent runs by type (the `.agent` filter drops the harness's auxiliary agents);

```bash
jq -r '.tool' ~/.skills/log/events.jsonl | sort | uniq -c
```

events per tool, which confirms that Cursor is importing the hook;

```bash
jq -r 'select(.server) | .server' ~/.skills/log/events.jsonl | sort | uniq -c
```

MCP tool calls by server (what `doctor`'s MCP audit is built on).

### The skill-forced-eval hook

[`hooks/skill-forced-eval/`](hooks/skill-forced-eval/) is the activation hook of the strategy's Story 1 (work items W13 and W14). On every prompt in Claude Code it shortlists the skills the agent can see whose quoted trigger phrasings, file paths or keywords match the prompt, and injects a check the agent must answer before using any other tool:

```
MANDATORY SKILL CHECK. Candidates: db-migrations
For each: [name] - YES/NO - [reason]. Call Skill(name) for every YES before any other tool.
```

In community measurements this forced YES/NO evaluation took activation from about 50% to 100% and stayed silent on prompts that needed no skill, where an LLM-based classifier invented skill names. It is deliberately the second layer: directive descriptions (O140–O145) come first, and `hook add` warns if any pinned skill still fails them.

```bash
skillctl install ./hooks/skill-forced-eval && skillctl hook add skill-forced-eval
```

It stays silent, and costs nothing, when there is nothing to say: in Cursor (`CURSOR_VERSION` is set; Cursor cannot inject text from a prompt hook and gets the same check as an always-apply rule instead), in folders with no `.claude/skills/`, `.agents/skills/` or `~/.claude/skills/` entries, and for prompts that match no skill. It reads the same `SKILL.md` files the harness lists, so it needs no `skillctl` binary at run time. Set `SKILLCTL_FORCED_EVAL_DEBUG=1` to see its shortlist reasoning in the transcript.

**Upgrading, pausing, removing.** A new version of the logger is `skillctl install …` followed by `skillctl hook add skillctl-logger@<version>`; the link moves, the settings stay. To pause logging without losing the registration history, `skillctl hook remove skillctl-logger` and `hook add` it again later. The log file is plain JSONL; rotate or delete it whenever you like, the hook recreates it.

## Validation

`skillctl validate` runs three rule groups on skills and one on [hooks](#hooks). Each rule has a stable ID, a default severity and a fix hint.

| Group | IDs | Severity | Covers |
|---|---|---|---|
| Agent Skills spec | S001–S008 | error | `SKILL.md` present; YAML frontmatter is a mapping; `name` matches `^[a-z0-9]+(-[a-z0-9]+)*$` and the folder name; `description` is 1–1024 characters; `compatibility`, `metadata`, `license`, `allowed-tools` have the right types. |
| Directive descriptions | O140–O145 | warning | The description is the activation surface, so it must read as a directive: a trigger verb addressing "this skill" (O140), a negative constraint such as "Do not … without this skill" (O141), concrete trigger phrasings, quoted or introduced with "including"/"such as" (O142), no hedging ("helps with", "various", "etc.") and no scaffold `[placeholders]` left in (O144), and no `keywords` field, which has no measured effect (O145). O143 is the index budget, reported by `status --budget` and after `add`. |
| Organisation rules | O101–O133 | mixed | `metadata.version` is semver; description length and "when" clause; no `<`/`>`; body and reference-file size budgets; every referenced path exists and every bundled file is referenced; no symlinks or `..` escapes; scripts have a shebang and executable bit, no interactive input, no `curl … \| sh` or `sudo`, no likely secrets. |
| Harness compatibility | H201–H205 | warning | Claude-only frontmatter fields, `allowed-tools`, Claude substitutions in the body, `context: fork` combined with asking the user, unknown fields. |
| Hook artifacts | K101–K111 | mixed | `HOOK.md` present and parsable; `name` matches the folder; `description` 1–1024 characters; `metadata.version` is semver; `events` is a non-empty list (unknown event names warn); `entry` is a regular file inside the folder with a shebang and the executable bit; no interactive input, `sudo` or `curl … \| sh` in scripts; no likely secrets; no symlinks; `matchers` names declared events and `timeout` is 1–600 seconds. K101–K105 and K107 cannot be allow-listed. |

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
- `--with-reference` additionally runs the reference `skills-ref validate` tool if it is on `PATH` and merges a failed verdict as error `R001`. Missing tools are ignored; hooks are skipped because the reference tool knows only the Agent Skills spec.

### Writing a description that activates

At session start the agent sees only each skill's name and description, about 100 tokens per skill, and loads the body after deciding the skill applies. The description is therefore the whole activation surface. In a 650-trial study, descriptions in directive form activated 100% of the time with no hook, weak ones fell to 37% even with a hook, and a `keywords` field changed nothing. The form the rules O140–O145 check for:

```yaml
description: ALWAYS invoke this skill when the user asks to create, modify, roll back or review a database migration, schema change or ORM model change, including indirect phrasing like "add a column" or "rename this field". Do not write migration files directly without this skill.
```

That is a trigger verb addressing *this skill*, a *when* clause, the ways people actually phrase the request, and what not to do without the skill. `skillctl new` scaffolds exactly this shape with placeholders. Existing templates are never overwritten, so a home created by an earlier version keeps its old `templates/default/SKILL.md` until you delete it (`init` then recreates the directive one).

Descriptions are checked when a skill is sealed or installed, when it is pinned (`add`, `global add`, `attach --skills`), when an activation hook is registered (`hook add` warns if any pinned skill fails the rules, because a prompt hook on top of weak descriptions measured worse than no hook), and in CI with `validate --strict`. The index budget (O143) adds up every visible name and description: Claude Code caps the skill listing at 1% of the context window and truncates a description at 1,536 characters, and skills past either limit lose their description and become invisible.

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

## Instruction files

Claude Code reads `CLAUDE.md`; Cursor reads `AGENTS.md`. Keeping both means paying for the same instructions twice and watching them drift. The arrangement skillctl checks for is one shared file and a one-line hand-off:

```
AGENTS.md      # team-shared instructions: build and test commands, conventions, layout
CLAUDE.md      # one line, "@AGENTS.md", plus anything only Claude Code can act on
```

Cursor reads `AGENTS.md` natively. Claude Code expands the `@AGENTS.md` import at launch, so both tools see the same text. Recent Claude Code versions can also read `AGENTS.md` directly, but only when no `CLAUDE.md` or `CLAUDE.local.md` exists in the folder or any folder above it, so the import is the arrangement that keeps working everywhere.

`skillctl lint` reports what stands in the way, and never edits the files:

| Rule | Severity | Finding | Suggested fix |
|---|---|---|---|
| I301 | warning | `AGENTS.md` is over 200 lines; every line is paid for on every turn in both tools | Move procedures into skills, path-specific guidance into `.claude/rules/` or `.cursor/rules/`, reference material into files read on demand |
| I302 | warning | A `.cursor/rules/*.mdc` file is over 500 lines, or an `alwaysApply: true` rule is over 100 lines (an always-apply rule is `AGENTS.md` by another name) | Split by topic, or scope the rule with `globs`/`description` so it loads on demand |
| I303 | warning | A section of `AGENTS.md` reads like a procedure: five or more ordered steps, three or more code blocks, or a heading naming a procedure (*how to, process, runbook, checklist, steps, workflow, release, deploy, migration*) with a few steps or a code block; reported at the heading | Create a skill from the section (`skillctl new <name>`) and leave a one-line pointer; a skill's body loads only when the task needs it |
| I305 | warning | An always-loaded instruction file (`AGENTS.md`, `CLAUDE.md`, `CLAUDE.local.md`, `.claude/rules/*.md`, `.cursor/rules/*.mdc`) has content that changes between sessions: a timestamp or "last updated" date, a `$(…)`/`${…}` expansion or `` !`command` `` injection, or an `@import` of a file that changes often (logs, status, notes, TODOs). Fenced code is skipped. Every change invalidates the prompt cache for the whole prefix | Keep these files byte-identical between sessions; move changing content behind a skill or a file the agent reads on demand |
| I304 | error | `CLAUDE.md` (or `.claude/CLAUDE.md`) exists but has no `@AGENTS.md` import (`@../AGENTS.md` from `.claude/`), or imports one that does not exist | Move the shared instructions into `AGENTS.md` and reduce `CLAUDE.md` to the import plus Claude-only notes |
| I308 | warning | A line of `CLAUDE.md` also appears in `AGENTS.md` (reported with its line number) | Delete the duplicate; both tools already read it from `AGENTS.md` |
| I309 | error | `AGENTS.md` exists, nothing imports it, and a `CLAUDE.md` or `CLAUDE.local.md` in the folder or a parent (up to the repository root) makes Claude Code skip it | Add a `CLAUDE.md` containing `@AGENTS.md`, or set Claude Code's **Project instructions** to `claude-md-and-agents-md` |

```bash
skillctl lint
```

```
/Users/me/work/crm/CLAUDE.md I304 error CLAUDE.md does not import AGENTS.md, so Claude Code and Cursor read different instructions
  Hint: Move the team-shared instructions into AGENTS.md (Cursor reads it natively) and reduce this file to a line containing @AGENTS.md plus any Claude-only notes.
1 error(s), 0 warning(s)
```

A folder with no instruction files passes. The budgets are the optimisation strategy's numbers; allow-list a rule in `settings.validate.allow` if a team's files legitimately exceed them. One more I-rule (quiet-wrapper references, I306) arrives with the work item that needs it.

Why I305 matters: Claude Code and Cursor cache the stable prefix of every request (system prompt, tools, instruction files), and cached input is billed at a fraction of the price. Anything in an always-loaded file that differs from the previous session, even a date, means the prefix is rebuilt and paid in full on every turn of that session.

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
│           ├── meta.json        kind, source, content hash, installed_at, skillctl_version
│           └── org-code-review/ the skill itself, read-only (0444 files, 0555 dirs and executables)
│   └── skillctl-logger/
│       └── 1.0.0/
│           ├── meta.json        "kind": "hook"
│           └── skillctl-logger/ HOOK.md and hook.py, read-only
├── drafts/
│   └── org-code-review/         writable copy created by `new`
└── templates/
    ├── default/SKILL.md         scaffold used by `new`
    └── hook/                    scaffold used by `new --kind hook`: HOOK.md and hook.py
```

The artifact folder sits one level below the version folder so that its own folder name equals its `name`, which the spec and Cursor require. `meta.json` sits beside it so harnesses never see it. Skills and hooks share one namespace.

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
| Hook artifacts: `HOOK.md` + script in the store, `new --kind hook`, K101–K111 validation, `kind` in `meta.json` | Implemented (W02a) |
| Hook registration: `hook add/remove/list`, `~/.claude/hooks/` links, `settings.json` merge with backup, doctor checks, the bundled `skillctl-logger` | Implemented (W02b) |
| Linking: instance resolution, reconcile, Git exclude block | Implemented |
| `attach`, `detach`, `status`, `add`, `remove`, `relink`, `global add/remove/list`, `hook add/remove/list`, `projects`, `skills` | Implemented |
| Validation: S, O, H and K rules; text, JSON and JUnit; allow-lists; `--strict`; `--with-reference` | Implemented |
| Instruction-file lint (`lint`): I301–I305, I308, I309, read-only | Implemented (W06, W07, W11) |
| `install` from Git and local folders, `edit`, `doctor --fix`, `gc`, `config`, `completions` | Implemented |
| Release pipeline: universal build, optional signing and notarization, clean-runner verification, GitHub release, crates.io publish and verification | Implemented (GitHub Actions) |
| Manual harness compatibility matrix (Claude Code, Cursor, ZohoCode) | Not run; default link targets are provisional |

## License

Apache License, Version 2.0. See [LICENSE](LICENSE).
