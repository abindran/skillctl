#!/usr/bin/env python3
"""subagent-schema: validate a subagent's final message against its schema.json.

Runs on SubagentStop. Finds the agent's schema through the project's
.claude/agents/<agent>.md link, checks the first JSON object in the agent's
last message against it (type, required, property types, enums), and exits 2
with the problems on stderr so the agent is sent back to fix its output. A
counter in $TMPDIR caps that at three attempts per agent run, after which the
hook exits 0 with a systemMessage. Every other path exits 0 silently.
"""
import json
import os
import re
import sys
import tempfile

MAX_ATTEMPTS = 3
FENCE = re.compile(r"```(?:json)?\s*(\{.*?\})\s*```", re.S)
TYPES = {
    "object": dict,
    "array": list,
    "string": str,
    "boolean": bool,
    "integer": int,
    "number": (int, float),
    "null": type(None),
}


def debug(message):
    if os.environ.get("SKILLCTL_SUBAGENT_SCHEMA_DEBUG"):
        sys.stderr.write(f"subagent-schema: {message}\n")


def schema_for(cwd, agent_type):
    """The schema.json beside the agent definition the link points at."""
    agents_dir = os.path.join(cwd, ".claude", "agents")
    try:
        entries = os.listdir(agents_dir)
    except OSError:
        return None
    wanted = f"{agent_type}.md".lower()
    for entry in entries:
        if entry.lower() != wanted:
            continue
        link = os.path.join(agents_dir, entry)
        target = os.path.realpath(link)
        path = os.path.join(os.path.dirname(target), "schema.json")
        try:
            with open(path, encoding="utf-8") as handle:
                schema = json.load(handle)
        except (OSError, ValueError):
            return None
        return schema if isinstance(schema, dict) else None
    return None


def first_object(message):
    """The first JSON object in the message, fenced or bare."""
    match = FENCE.search(message)
    candidates = [match.group(1)] if match else []
    start = message.find("{")
    if start >= 0:
        candidates.append(message[start : message.rfind("}") + 1])
    for text in candidates:
        try:
            value = json.loads(text)
        except ValueError:
            continue
        if isinstance(value, dict):
            return value
    return None


def problems(value, schema, path="$"):
    """Violations of the schema subset: type, required, properties, enum."""
    found = []
    expected = schema.get("type")
    if expected and not isinstance(value, TYPES.get(expected, object)):
        return [f"{path} must be {expected}"]
    if expected == "boolean" and isinstance(value, bool):
        pass
    elif expected in ("integer", "number") and isinstance(value, bool):
        return [f"{path} must be {expected}, not a boolean"]
    if "enum" in schema and value not in schema["enum"]:
        found.append(f"{path} must be one of {schema['enum']}")
    if isinstance(value, dict):
        for key in schema.get("required", []):
            if key not in value:
                found.append(f"{path}.{key} is required")
        for key, sub in schema.get("properties", {}).items():
            if key in value and isinstance(sub, dict):
                found.extend(problems(value[key], sub, f"{path}.{key}"))
    return found


def counter_path(agent_id):
    safe = re.sub(r"[^A-Za-z0-9_.-]", "_", agent_id)[:80]
    return os.path.join(tempfile.gettempdir(), f"skillctl-subagent-schema-{safe}.count")


def attempts(agent_id):
    try:
        with open(counter_path(agent_id), encoding="utf-8") as handle:
            return int(handle.readline().strip() or 0)
    except (OSError, ValueError):
        return 0


def record_attempt(agent_id, count):
    try:
        with open(counter_path(agent_id), "w", encoding="utf-8") as handle:
            handle.write(str(count))
    except OSError:
        pass


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    if not isinstance(event, dict):
        return 0
    agent_type = event.get("agent_type")
    agent_id = event.get("agent_id") or "unknown"
    message = event.get("last_assistant_message")
    if not isinstance(agent_type, str) or not isinstance(message, str):
        debug("no agent_type or last_assistant_message; silent")
        return 0
    cwd = event.get("cwd") or os.getcwd()
    schema = schema_for(cwd, agent_type)
    if schema is None:
        debug(f"no schema for {agent_type}; silent")
        return 0
    value = first_object(message)
    found = ["the final message contains no JSON object"] if value is None else problems(value, schema)
    if not found:
        debug(f"{agent_type} output matches its schema")
        return 0
    count = attempts(agent_id) + 1
    if count <= MAX_ATTEMPTS:
        record_attempt(agent_id, count)
        sys.stderr.write(
            "Your final message must be ONLY the JSON object described in your instructions. "
            f"Problems (attempt {count} of {MAX_ATTEMPTS}): "
            + "; ".join(found)
            + ". Reply with the corrected JSON object and nothing else.\n"
        )
        return 2
    sys.stdout.write(
        json.dumps(
            {
                "systemMessage": (
                    f"subagent-schema: {agent_type} returned output that did not match its "
                    f"schema after {MAX_ATTEMPTS} attempts ({'; '.join(found)}); treat the result as partial."
                )
            }
        )
        + "\n"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
