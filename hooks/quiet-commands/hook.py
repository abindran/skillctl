#!/usr/bin/env python3
"""quiet-commands: send raw test and build commands through the quiet wrapper.

A PreToolUse hook on Claude Code's Bash tool. When the project has the
test-quiet script bundle linked (`.claude/scripts/test-quiet/test-quiet.sh`)
and the agent runs a bare test or build command such as `cargo test` or
`npm test`, the hook blocks the call (exit 2) and hands the agent the exact
wrapped command to run instead, so a passing run costs a few tokens instead
of thousands. With SKILLCTL_QUIET_REWRITE=1 it rewrites the command in place
through `updatedInput` instead; Claude Code only accepts that together with
`permissionDecision: allow`, so the wrapped command then runs without a
permission prompt, which is why rewriting is opt-in.

Silent (exit 0, no output) whenever there is nothing to do: under Cursor,
whose shell commands do not reach imported PreToolUse hooks; in folders
without the wrapper; for commands that are already wrapped, are not test or
build commands, or are compound shell lines the wrapper cannot take whole.
"""
import json
import os
import re
import shlex
import sys

WRAPPER = os.path.join(".claude", "scripts", "test-quiet", "test-quiet.sh")

# (first token, acceptable second tokens or None for any) of commands whose
# output is mostly noise on success. Keep in step with the classifier in
# hooks/skillctl-logger/hook.py.
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
# Anything that makes the line more than one simple command.
COMPOUND = re.compile(r"(\|\||&&|[|;&<>`]|\$\(|\n)")


def debug(message):
    if os.environ.get("SKILLCTL_QUIET_COMMANDS_DEBUG"):
        sys.stderr.write("quiet-commands: %s\n" % message)


def classify(command):
    """'wrapped' when the line already goes through the wrapper, 'raw' when it
    is a bare test or build command, None otherwise."""
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
    # Skip leading VAR=value assignments.
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


def find_wrapper(cwd):
    """The wrapper linked in cwd or any folder above it, as a path relative to
    cwd when it lies below cwd and absolute otherwise."""
    folder = os.path.abspath(cwd)
    while True:
        candidate = os.path.join(folder, WRAPPER)
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            if folder == os.path.abspath(cwd):
                return WRAPPER
            return candidate
        parent = os.path.dirname(folder)
        if parent == folder:
            return None
        folder = parent


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    if not isinstance(event, dict):
        return 0
    if "cursor_version" in event or os.environ.get("CURSOR_VERSION"):
        debug("Cursor: shell commands do not reach imported PreToolUse hooks")
        return 0
    tool_input = event.get("tool_input")
    if not isinstance(tool_input, dict):
        return 0
    command = tool_input.get("command")
    kind = classify(command)
    if kind != "raw":
        debug("not a raw test command (%s)" % kind)
        return 0
    cwd = event.get("cwd") or os.getcwd()
    wrapper = find_wrapper(cwd)
    if wrapper is None:
        debug("no test-quiet wrapper linked above %s" % cwd)
        return 0
    wrapped = "%s %s" % (wrapper, command.strip())
    if os.environ.get("SKILLCTL_QUIET_REWRITE") == "1":
        updated = dict(tool_input)
        updated["command"] = wrapped
        print(
            json.dumps(
                {
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "allow",
                        "permissionDecisionReason": "quiet-commands: routed through the test-quiet wrapper",
                        "updatedInput": updated,
                    }
                }
            )
        )
        debug("rewrote to: %s" % wrapped)
        return 0
    sys.stderr.write(
        "Run tests and builds through the quiet wrapper so only failures reach the context. Run exactly:\n"
        "%s\n" % wrapped
    )
    return 2


if __name__ == "__main__":
    sys.exit(main())
