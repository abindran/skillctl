# Hooks

A hook is the second artifact kind in the store: a folder with `HOOK.md` and the script a harness runs on lifecycle events. This page covers how hooks are modelled, how `hook add` registers them, how the same registration reaches Cursor, what the bundled `skillctl-logger` does internally, and what `doctor` checks. Usage (commands, the log format, `jq` recipes) is in the README's [Hooks](https://github.com/abindran/skillctl#hooks) section.

## Artifact model

`artifact::Kind` is `Skill` or `Hook`. The kind is **detected from the folder**: `SKILL.md` makes a skill, `HOOK.md` makes a hook, both is an error (`Kind::detect`). It is also written to `meta.json` as `kind` so listings can read it without opening the folder; a missing `kind` means `skill`, which is how stores created before hooks existed keep working.

Everything else is shared: the same `store/<name>/<version>/<name>/` layout and flat namespace (a skill and a hook cannot share a name), the same content hash, staging, read-only sealing, drafts (`new --kind hook`, `edit`, `seal`), discovery on `install`, and the same `Skill` struct for the parsed document (`Skill { dir, kind, frontmatter, body }`, loaded with `load_kind`).

`HOOK.md` frontmatter:

| Field | Type | Read by |
|---|---|---|
| `name`, `description`, `metadata.version` | as for skills | K102–K104 |
| `events` | list of Claude Code event names | K105/K106, `hook add` |
| `matchers` | map event → tool-name regex | K111, `hook add` |
| `timeout` | seconds, 1–600, default 10 | K111, `hook add` |
| `entry` | script path relative to the folder | K107, `hook add` |

The hook-specific fields live in the frontmatter's `extra` map rather than typed fields, so skill validation is untouched (a skill with an `events:` key still gets H205). Accessors are in `validate::hook` (`events`, `entry`, `matchers`, `timeout`). Rules K101–K111 are listed on [Validation](Validation#hook-rules).

## Registration flow

```mermaid
flowchart TD
    A["skillctl hook add name[@pin]"] --> B["resolve_pin → store dir; Kind::of must be Hook"]
    B --> C["registry.global.hooks[name] = pin; save (backup + atomic)"]
    C --> D["reconcile_into(global.hooks, [~/.claude/hooks])\nlink ~/.claude/hooks/name → store/name/ver/name"]
    D --> E["declared(): read HOOK.md → events, matchers, timeout, entry"]
    E --> F["claude::load(~/.claude/settings.json)\nmissing → {} · invalid JSON → Error::Usage, stop"]
    F --> G["claude::register: one group per event not yet present"]
    G --> H{"anything added?"}
    H -- yes --> I["claude::save: settings.json.bak, then atomic write"]
    H -- no --> J["settings unchanged"]
```

The whole command runs under the exclusive registry lock (`pins::write_locked`). Order matters the same way it does for skills: the registry is saved before links and settings are touched, so a crash leaves a pin that `relink --all` or `doctor --fix` can complete.

### The settings merge (`harness::claude`)

`~/.claude/settings.json` is parsed as a generic `serde_json::Value`, never into a typed struct, so every key skillctl does not know about round-trips byte-for-byte in content (formatting is normalised to two-space pretty JSON). The shape Claude Code expects, and the entry skillctl adds:

```json
{
  "hooks": {
    "PostToolUse": [
      { "matcher": "Skill",
        "hooks": [ { "type": "command",
                     "command": "\"$HOME/.claude/hooks/skillctl-logger/hook.py\"",
                     "timeout": 10 } ] }
    ]
  }
}
```

- **Ownership.** An entry is skillctl's when its `command` contains `/.claude/hooks/<name>/`. That substring is the only marker: nothing is written anywhere else in the file, and `unregister` removes exactly the items that carry it, then prunes groups, events and the `hooks` object that became empty.
- **Idempotency.** `register` first collects the events under which the hook already has an entry (`registered_events`). For those it compares the group's `matcher` and the item's `timeout` and `command` with what `HOOK.md` now declares and rewrites only the fields that differ (`updated`), otherwise reports `present`; `save` is not called when nothing changed, which also means no new `.bak`.
- **Why `$HOME` and a link folder.** The command goes through `~/.claude/hooks/<name>/`, a symlink into the store, so upgrading the hook (`hook add name@newer`) repoints one link and never edits `settings.json`. `$HOME` is expanded by the shell the harness uses, which keeps a dotfiles-synced copy of the file portable between machines.
- **Refusals.** A file that fails to parse, or whose top level is not an object, or whose `hooks` key is not an object, is an `Error::Usage` with the reason; nothing is written. This is the price of merging a hand-edited file: skillctl would lose comments or unknown syntax if it rewrote one it could not parse.
- **Backup.** `save` copies the previous bytes to `settings.json.bak` before the atomic write, creating `~/.claude/` if the file did not exist yet (no backup is written in that case).

### Cursor

Cursor loads Claude Code hooks from `~/.claude/settings.json` (and from a project's `.claude/settings.json` and `.claude/settings.local.json`) when **Include Third-Party Plugins, Skills, and Other Configs** is on, which is the default. It translates event names itself:

| `HOOK.md` (Claude Code) | Cursor | Status |
|---|---|---|
| `PreToolUse`, `PostToolUse` | `preToolUse`, `postToolUse` | mapped |
| `UserPromptSubmit` | `beforeSubmitPrompt` | mapped |
| `Stop`, `SubagentStop` | `stop`, `subagentStop` | mapped |
| `SessionStart`, `SessionEnd` | `sessionStart`, `sessionEnd` | mapped |
| `PreCompact` | `preCompact` | mapped |
| `Notification`, `PermissionRequest` | — | not supported |
| everything else | — | Claude Code only |

Matchers and `timeout` carry over; Cursor's `loop_limit` does not apply to imported hooks (it is `null`), which is why a hook that may retry must count its own attempts. Precedence when several sources match is enterprise, team, `.cursor/hooks.json`, `~/.cursor/hooks.json`, Claude project-local, Claude project, Claude user, and **all matching hooks run** — so a hook present in both `~/.claude/settings.json` and `~/.cursor/hooks.json` runs twice in Cursor. skillctl therefore never writes `~/.cursor/hooks.json`, and `doctor` warns when it finds a skillctl hook there.

Whether Cursor rewrites stdin field names to Claude Code's for imported hooks is not documented, so hook scripts should accept both shapes; the logger does.

## The `skillctl-logger` hook

`hooks/skillctl-logger/` in the repository (installed from the checkout or from Git with `--path hooks/skillctl-logger`). It is a plain hook: a `HOOK.md` declaring nine events with `matchers: {PostToolUse: "Skill|mcp__.*"}` and `timeout: 10`, and `hook.py`.

Design constraints, in order:

1. **Never block the harness.** Every code path returns 0; `json.load` failures, non-object payloads and `OSError` on the log are swallowed. Claude Code treats exit 0 with empty stdout as "continue".
2. **Fast.** One Python start-up, one `json.dumps`, one append. No imports beyond the standard library.
3. **Both tools, one schema.** `tool` is `cursor` when the payload has `cursor_version` or the environment has `CURSOR_VERSION`; `session` is `session_id` or `conversation_id`; `cwd` is `cwd` or `workspace_roots[0]`; `event` is normalised to PascalCase so `preCompact` and `PreCompact` are the same line.
4. **Tool events are filtered at the source.** The `PostToolUse` matcher is `Skill|mcp__.*`, so only skill invocations and MCP tool calls reach the script; for `mcp__<server>__<tool>` names it records `server`, which `doctor`'s MCP audit uses to find configured servers nobody calls.
5. **Nothing sensitive.** The prompt is reduced to `prompt_chars`; tool output, `custom_instructions` and anything not on the explicit copy list (`COPIED`) is dropped.
6. **Validates clean** under K101–K111. In particular it reads stdin with `json.load(sys.stdin)` rather than `.read()`, because the shared script heuristic (O131/K108) treats a bare `read` token as a possible terminal read.

The log path is `$SKILLCTL_HOME/log/events.jsonl`, falling back to `~/.skills/log/events.jsonl`, so a test home (`SKILLCTL_HOME` set) logs into itself. Writes are single `write` calls of one line in append mode, which keeps concurrent hooks from interleaving within a line.

### Observed behaviour worth knowing

- Claude Code picks up new `settings.json` hook entries without a restart.
- Claude Code fires `SubagentStop` for its own auxiliary agents (for example the one that titles the session after each turn), a few seconds after each `Stop`, with an `agent_id` but **no `agent_type`**. The logger therefore writes no `agent` field for them. Delegation metrics must filter on `agent` being present, or they will count one phantom subagent per turn.
- The documented `PreCompact` payload carries `trigger` (and `custom_instructions`, which is dropped) but no token count; context size at compaction has to be derived from the transcript, whose path is logged.

## doctor

With at least one hook pinned, `doctor` adds to its usual checks:

| Check | Severity | Condition | Fix |
|---|---|---|---|
| `link-missing` / `link-wrong` / `link-dangling` / `link-conflict` | as for skills | `~/.claude/hooks/<name>` does not point at the pinned store version (scope "the global hooks") | `doctor --fix` or `relink --all` reconcile hook links through the same engine |
| `pin-missing` | error | a pinned hook version is not in the store | reinstall or `hook remove` |
| `settings-invalid` | error | `~/.claude/settings.json` is not valid JSON | fix by hand; skillctl never rewrites it |
| `hook-unregistered` | error | a declared event has no entry for the hook | `skillctl hook add <name>` registers only what is missing |
| `hook-duplicate` | warning | the hook's command appears more than once under an event, or also in `~/.cursor/hooks.json` | remove the extra entries |

`gc` counts `global.hooks` as pins, so a registered hook version is never collected.
