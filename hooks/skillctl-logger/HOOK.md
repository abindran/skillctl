---
name: skillctl-logger
description: Appends one JSON line per harness lifecycle event to ~/.skills/log/events.jsonl, tagged with the tool that fired it, so sessions, prompts, compactions, subagent runs, skill activations, MCP tool calls and raw versus wrapped test commands can be counted and compared between Claude Code and Cursor.
metadata:
  version: "1.2.1"
events: [SessionStart, SessionEnd, UserPromptSubmit, PostToolUse, Stop, SubagentStart, SubagentStop, PreCompact, PostCompact]
matchers:
  PostToolUse: "Skill|Bash|mcp__.*"
timeout: 10
entry: hook.py
---

# skillctl-logger

The observability baseline for the LLM optimisation strategy (work item W02).
Install it once per machine:

```
skillctl install <path or URL of this folder>
skillctl hook add skillctl-logger
```

`hook add` links the hook under `~/.claude/hooks/skillctl-logger/` and registers
`hook.py` for every event above in `~/.claude/settings.json`. Cursor reads that
file through its third-party imports (on by default), so the same registration
serves both tools. Events Cursor does not map (`SubagentStart`, `PostCompact`)
simply never fire there.

## What gets logged

One line per event, appended to `$SKILLCTL_HOME/log/events.jsonl`
(`~/.skills/log/events.jsonl` by default):

```json
{"ts":"2026-10-19T18:40:12Z","tool":"claude-code","event":"PreCompact","session":"abc123","cwd":"/Users/me/work/crm-web","trigger":"auto"}
```

| Field | Source |
|---|---|
| `ts` | UTC time the hook ran |
| `tool` | `cursor` when `cursor_version` is in the payload or `CURSOR_VERSION` is set, else `claude-code` |
| `event` | `hook_event_name`, normalised to Claude Code's spelling (`preCompact` → `PreCompact`) |
| `session` | `session_id` (Claude Code) or `conversation_id` (Cursor) |
| `cwd` | `cwd`, or the first of `workspace_roots` |
| `transcript` | `transcript_path`, when present |
| `agent`, `agent_id` | `agent_type` and `agent_id` on subagent events |
| `trigger`, `reason`, `source`, `stop_hook_active` | copied from the event when present |
| `prompt_chars` | length of the prompt on `UserPromptSubmit`; the text is never recorded |
| `tool_name`, `skill` | on tool events: the tool, and `tool_input.skill` when the tool is `Skill` |
| `server` | on MCP tool events (`mcp__<server>__<tool>`): the server name, so `doctor` can tell which configured servers are actually used |

The script never blocks the harness: every path exits 0, and malformed input or
an unwritable log are ignored.
