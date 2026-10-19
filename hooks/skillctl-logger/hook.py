#!/usr/bin/env python3
"""skillctl-logger: append one JSON line per harness lifecycle event.

Runs under Claude Code and, through Cursor's third-party hook import, under
Cursor. It never blocks: every path exits 0, and malformed input or an
unwritable log file are ignored. Prompt text is never recorded, only its
length.
"""
import json
import os
import sys
from datetime import datetime, timezone

# Event fields copied verbatim when present. Nothing else from the payload is
# kept, so prompts, tool output and custom compaction instructions stay out.
COPIED = {
    "SessionStart": ["source"],
    "SessionEnd": ["reason"],
    "Stop": ["stop_hook_active"],
    "SubagentStop": ["stop_hook_active"],
    "PreCompact": ["trigger"],
    "PostCompact": ["trigger"],
}


def log_path():
    home = os.environ.get("SKILLCTL_HOME") or os.path.join(
        os.path.expanduser("~"), ".skills"
    )
    return os.path.join(home, "log", "events.jsonl")


def normalise(event_name):
    """Claude Code spells events in PascalCase, Cursor in camelCase."""
    if not isinstance(event_name, str) or not event_name:
        return "unknown"
    return event_name[0].upper() + event_name[1:]


def first_root(event):
    roots = event.get("workspace_roots")
    if isinstance(roots, list) and roots:
        return roots[0]
    return None


def record_for(event):
    name = normalise(event.get("hook_event_name"))
    tool = (
        "cursor"
        if "cursor_version" in event or os.environ.get("CURSOR_VERSION")
        else "claude-code"
    )
    record = {
        "ts": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "tool": tool,
        "event": name,
        "session": event.get("session_id") or event.get("conversation_id"),
        "cwd": event.get("cwd") or first_root(event),
    }
    if event.get("transcript_path"):
        record["transcript"] = event["transcript_path"]
    if event.get("agent_type"):
        record["agent"] = event["agent_type"]
    if event.get("agent_id"):
        record["agent_id"] = event["agent_id"]
    for key in COPIED.get(name, []):
        if key in event:
            record[key] = event[key]
    if name == "UserPromptSubmit" and isinstance(event.get("prompt"), str):
        record["prompt_chars"] = len(event["prompt"])
    if name in ("PreToolUse", "PostToolUse"):
        record["tool_name"] = event.get("tool_name")
        tool_input = event.get("tool_input")
        if isinstance(tool_input, dict):
            skill = tool_input.get("skill") or tool_input.get("name")
            if isinstance(skill, str):
                record["skill"] = skill
    return record


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    if not isinstance(event, dict):
        return 0
    line = json.dumps(record_for(event), separators=(",", ":"), ensure_ascii=False)
    try:
        path = log_path()
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "a", encoding="utf-8") as handle:
            handle.write(line + "\n")
    except OSError:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
