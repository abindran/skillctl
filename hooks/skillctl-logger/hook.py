#!/usr/bin/env python3
"""skillctl-logger: append one JSON line per harness lifecycle event.

Runs under Claude Code and, through Cursor's third-party hook import, under
Cursor. It never blocks: every path exits 0, and malformed input or an
unwritable log file are ignored. Prompt text is never recorded, only its
length.
"""
import json
import os
import re
import shlex
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

# Bare test and build commands whose output is mostly noise on success; the
# log records only whether such a command ran raw or through the test-quiet
# wrapper, never the command text. Keep in step with hooks/quiet-commands.
TEST_COMMANDS = {
    "cargo": {"test", "build", "check", "clippy", "nextest"},
    "npm": {"test", "t", "run"},
    "pnpm": {"test", "t", "run", "build"},
    "yarn": {"test", "run", "build"},
    "bun": {"test", "run"},
    "npx": {"jest", "vitest", "mocha", "tsc", "eslint", "playwright"},
    "pytest": None,
    "tox": None,
    "go": {"test", "build", "vet"},
    "mvn": {"test", "verify", "package", "install", "compile"},
    "gradle": {"test", "build", "check", "assemble"},
    "gradlew": {"test", "build", "check", "assemble"},
    "make": {"test", "check", "build", "all"},
    "dotnet": {"test", "build"},
    "swift": {"test", "build"},
    "xcodebuild": None,
    "ctest": None,
    "ninja": None,
}
RUN_SCRIPTS = {"test", "build", "lint", "check", "typecheck", "coverage"}
COMPOUND = re.compile(r"(\|\||&&|[|;&<>`]|\$\(|\n)")


def classify_test_command(command):
    """'wrapped', 'raw' or None for a shell command line."""
    if not isinstance(command, str) or not command.strip():
        return None
    if "test-quiet" in command:
        return "wrapped"
    if COMPOUND.search(command):
        return None
    try:
        tokens = shlex.split(command)
    except ValueError:
        return None
    while tokens and re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", tokens[0]):
        tokens = tokens[1:]
    if not tokens:
        return None
    first = os.path.basename(tokens[0])
    if first in ("python", "python3") and tokens[1:3] == ["-m", "pytest"]:
        return "raw"
    allowed = TEST_COMMANDS.get(first, False)
    if allowed is False:
        return None
    if allowed is None:
        return "raw"
    if len(tokens) < 2 or tokens[1] not in allowed:
        return None
    if tokens[1] == "run" and (len(tokens) < 3 or tokens[2] not in RUN_SCRIPTS):
        return None
    return "raw"


# A slash command at the start of a prompt (`/checkpoint`, `/plugin:cmd`).
# Only the name is kept; the rest of the prompt never is.
SLASH = re.compile(r"\A\s*/([A-Za-z0-9][A-Za-z0-9:_.-]{0,63})(?=\s|\Z)")


def slash_command(text):
    if not isinstance(text, str):
        return None
    match = SLASH.match(text)
    return match.group(1).lower() if match else None


def log_path():
    home = os.environ.get("SKILLCTL_HOME") or os.path.join(
        os.path.expanduser("~"), ".skills"
    )
    return os.path.join(home, "log", "events.jsonl")


# Cursor's native event names. Imported Claude Code hooks normally receive
# the mapped names (preToolUse, postToolUse, …), which differ only by their
# first letter's case and are handled below; these entries are kept so the
# hook still files events correctly if a payload carries the native name.
CURSOR_EVENTS = {
    "beforeSubmitPrompt": "UserPromptSubmit",
    "beforeShellExecution": "PreToolUse",
    "afterShellExecution": "PostToolUse",
    "beforeMCPExecution": "PreToolUse",
    "afterMCPExecution": "PostToolUse",
}


def normalise(event_name):
    """Claude Code spells events in PascalCase, Cursor in camelCase."""
    if not isinstance(event_name, str) or not event_name:
        return "unknown"
    if event_name in CURSOR_EVENTS:
        return CURSOR_EVENTS[event_name]
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
    if isinstance(event.get("cursor_version"), str):
        record["harness_version"] = event["cursor_version"]
    if event.get("agent_type"):
        record["agent"] = event["agent_type"]
    if event.get("agent_id"):
        record["agent_id"] = event["agent_id"]
    for key in COPIED.get(name, []):
        if key in event:
            record[key] = event[key]
    if name == "UserPromptSubmit" and isinstance(event.get("prompt"), str):
        record["prompt_chars"] = len(event["prompt"])
        command = slash_command(event["prompt"])
        if command:
            record["command"] = command
    if name in ("PreToolUse", "PostToolUse"):
        tool_name = event.get("tool_name")
        record["tool_name"] = tool_name
        if isinstance(tool_name, str) and tool_name.startswith("mcp__"):
            # Claude Code names MCP tools mcp__<server>__<tool>.
            record["server"] = tool_name[len("mcp__"):].split("__", 1)[0]
        tool_input = event.get("tool_input")
        if not isinstance(tool_input, dict) and isinstance(event.get("command"), str):
            # Cursor's native shell events carry the command at the top level.
            tool_input = {"command": event["command"]}
            tool_name = tool_name or "Shell"
            record["tool_name"] = tool_name
        if isinstance(tool_input, dict) and tool_name == "Skill":
            skill = tool_input.get("skill") or tool_input.get("name")
            if isinstance(skill, str):
                record["skill"] = skill
        if isinstance(tool_input, dict) and tool_name == "SlashCommand":
            # The model running a custom command: tool_input.command is "/name args".
            command = slash_command(tool_input.get("command"))
            if command:
                record["command"] = command
        if isinstance(tool_input, dict) and tool_name in ("Bash", "Shell"):
            kind = classify_test_command(tool_input.get("command"))
            if kind:
                record["test_command"] = kind
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
