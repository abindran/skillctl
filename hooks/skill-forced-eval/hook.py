#!/usr/bin/env python3
"""skill-forced-eval: inject a mandatory skill check for prompts that match a skill.

Runs on UserPromptSubmit in Claude Code. It reads the skills the agent can see
from the working folder and the user's global skills folder, shortlists the
ones whose name, trigger phrasings or file paths match the prompt, and prints a
YES/NO check for the agent. It prints nothing when nothing matches, in Cursor,
and in folders without skills. Every path exits 0.
"""
import json
import os
import re
import sys

MAX_CANDIDATES = 3
MIN_KEYWORD_HITS = 2
SKILL_DIRS = (".claude/skills", ".agents/skills")
STOPWORDS = {
    "the", "and", "for", "with", "this", "that", "from", "into", "when", "user",
    "asks", "ask", "any", "all", "are", "you", "your", "use", "using", "skill",
    "always", "invoke", "not", "without", "directly", "like", "including",
    "indirect", "phrasing", "such", "example", "please", "can", "could", "would",
    "should", "need", "want", "make", "add", "change", "new", "file", "files",
    "code", "also", "just", "how", "what", "where", "which", "about", "then",
    "them", "they", "their", "there", "here", "have", "has", "had", "does", "did",
    "its", "our", "out", "one", "two", "via", "per", "but", "than", "too", "very",
}
QUOTED = re.compile(r'"([^"\n]{3,})"|“([^”\n]{3,})”')
PATHISH = re.compile(r"[\w./*-]*(?:/[\w.*-]+|\.[a-z]{1,5})\b")
GENERIC_PARTS = {"src", "app", "apps", "lib", "test", "tests", "main", "index", "docs", "readme"}
FRONTMATTER = re.compile(r"\A---\r?\n(.*?)\r?\n---", re.S)


def debug(message):
    if os.environ.get("SKILLCTL_FORCED_EVAL_DEBUG"):
        sys.stderr.write(f"skill-forced-eval: {message}\n")


def words(text):
    return {w for w in re.findall(r"[a-z0-9][a-z0-9_-]{2,}", text.lower()) if w not in STOPWORDS}


def paths(text):
    """Path components and extensions mentioned in text: jsapps/x/y.hbs gives
    jsapps, x, y.hbs and .hbs, so a description naming jsapps/ or .hbs matches
    a prompt that names a file inside it."""
    found = set()
    for match in PATHISH.findall(text):
        token = match.lower().strip("*")
        if "/" not in token and not token.startswith("."):
            continue
        for part in token.split("/"):
            part = part.strip("*")
            if len(part) >= 3 and part not in GENERIC_PARTS:
                found.add(part)
            if "." in part.strip("."):
                ext = "." + part.rsplit(".", 1)[-1]
                if 2 <= len(ext) <= 6:
                    found.add(ext)
            elif part.startswith(".") and len(part) >= 3:
                found.add(part)
    return found


def frontmatter(text):
    """name and description from SKILL.md without a YAML library."""
    match = FRONTMATTER.match(text)
    if not match:
        return None, None
    name = description = None
    for line in match.group(1).splitlines():
        if line.startswith("name:"):
            name = line[5:].strip().strip("'\"")
        elif line.startswith("description:"):
            description = line[12:].strip()
            if description[:1] in "'\"" and description[-1:] == description[:1]:
                description = description[1:-1]
    return name, description


def visible_skills(cwd):
    """(name, description) for every SKILL.md the harness would list here."""
    roots = [os.path.join(cwd, d) for d in SKILL_DIRS]
    roots.append(os.path.join(os.path.expanduser("~"), ".claude", "skills"))
    seen = {}
    for root in roots:
        try:
            entries = sorted(os.listdir(root))
        except OSError:
            continue
        for entry in entries:
            path = os.path.join(root, entry, "SKILL.md")
            try:
                with open(path, encoding="utf-8", errors="replace") as handle:
                    # Only the frontmatter is needed: stop at its closing line.
                    lines = []
                    for index, line in enumerate(handle):
                        lines.append(line)
                        if index > 0 and line.rstrip("\r\n") == "---" or index > 200:
                            break
                    text = "".join(lines)
            except OSError:
                continue
            name, description = frontmatter(text)
            if name and description and name not in seen:
                seen[name] = description
    return seen


def shortlist(prompt, skills):
    """Skills whose triggers match the prompt, best first."""
    prompt_lower = prompt.lower()
    prompt_words = words(prompt)
    prompt_paths = paths(prompt)
    scored = []
    for name, description in skills.items():
        reasons = []
        for groups in QUOTED.findall(description):
            phrase = next(g for g in groups if g).lower()
            if phrase in prompt_lower:
                reasons.append(f'phrase "{phrase}"')
        shared_paths = prompt_paths & paths(description)
        if shared_paths:
            reasons.append("path " + ", ".join(sorted(shared_paths)))
        keywords = prompt_words & (words(description) | words(name.replace("-", " ")))
        if len(keywords) >= MIN_KEYWORD_HITS:
            reasons.append("keywords " + ", ".join(sorted(keywords)))
        if reasons:
            score = len(keywords) + 3 * sum(r.startswith("phrase") for r in reasons) + 2 * (
                1 if shared_paths else 0
            )
            scored.append((score, name, "; ".join(reasons)))
    scored.sort(key=lambda item: (-item[0], item[1]))
    return scored[:MAX_CANDIDATES]


def main():
    if os.environ.get("CURSOR_VERSION"):
        return 0  # Cursor gets the same check as an always-apply rule
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    if not isinstance(event, dict):
        return 0
    prompt = event.get("prompt")
    if not isinstance(prompt, str) or not prompt.strip():
        return 0
    cwd = event.get("cwd") or os.getcwd()
    skills = visible_skills(cwd)
    if not skills:
        debug("no skills visible; silent")
        return 0
    candidates = shortlist(prompt, skills)
    if not candidates:
        debug(f"no candidate among {len(skills)} skill(s); silent")
        return 0
    for score, name, why in candidates:
        debug(f"{name} ({score}): {why}")
    names = ", ".join(name for _, name, _ in candidates)
    sys.stdout.write(
        f"MANDATORY SKILL CHECK. Candidates: {names}\n"
        "For each: [name] - YES/NO - [reason]. Call Skill(name) for every YES before any other tool.\n"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
