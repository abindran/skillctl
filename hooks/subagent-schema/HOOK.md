---
name: subagent-schema
description: When a subagent finishes, checks its final message against the schema.json shipped with that agent and, when it does not match, sends the agent back to produce the required JSON, at most three times per run. Silent for agents without a schema.
metadata:
  version: "1.0.1"
events: [SubagentStop]
timeout: 10
entry: hook.py
---

# subagent-schema

Output validation for delegated work (strategy work item W20). An agent that
returns a schema-validated summary can be trusted by the lead without
re-reading its transcript; one that returns prose, or JSON with the wrong
shape, is asked to try again, and after three attempts the lead is told the
result is unvalidated instead of being blocked forever.

```
skillctl install <path or URL of this folder>
skillctl hook add subagent-schema
```

## What it does on each SubagentStop

1. Reads `agent_type`, `agent_id` and `last_assistant_message` from the event.
   Without a message, or when the agent is one of Claude Code's internal
   auxiliary agents (no `agent_type`), it exits 0 silently.
2. Finds the agent's schema: the agent file in `.claude/agents/` of the working
   folder, then in `~/.claude/agents/` (global agents). The file is
   `<agent_type>.md` (case-insensitively, as skillctl links it) or, because
   Claude Code identifies an agent by its `name` field and not its file name,
   any `.md` file whose frontmatter `name` equals `agent_type`. It is resolved
   through its symlink and `schema.json` is read beside it. No schema, no check.
3. Extracts the first JSON object from the message (fenced or bare) and checks
   it against the schema: `type`, `required`, per-property `type` and `enum`.
   This is the subset the bundled agents use, not a full JSON Schema
   implementation.
4. On failure, if fewer than three attempts have been counted for this
   `agent_id` (a counter file under `$TMPDIR`), it increments the counter and
   exits **2** with the problems on stderr, which Claude Code and Cursor both
   turn into an instruction for the subagent to continue. The counter lives
   here so the cap is three in both tools; Cursor documents a `loop_limit`
   for its own stop hooks, but not whether it applies to imported ones.
5. After three failures it exits 0 and prints a `systemMessage` saying the
   output stayed invalid, so the lead treats it as `partial`.

Set `SKILLCTL_SUBAGENT_SCHEMA_DEBUG=1` to log decisions to stderr on the
non-blocking paths.
