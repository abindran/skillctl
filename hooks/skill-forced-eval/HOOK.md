---
name: skill-forced-eval
description: Before each prompt in Claude Code, shortlists the skills the agent can see whose name, trigger phrasings or file paths match the prompt, and injects a mandatory YES/NO check so the agent decides explicitly whether to invoke each candidate. Silent in Cursor, in folders without skills, and for prompts that match nothing.
metadata:
  version: "1.0.0"
events: [UserPromptSubmit]
timeout: 10
entry: hook.py
---

# skill-forced-eval

The activation hook of the LLM optimisation strategy (work items W13 and W14).
In community measurements a forced YES/NO evaluation per candidate skill took
activation from about 50% to 100% and stayed silent on prompts that needed no
skill, where an LLM-based classifier invented skill names. Directive
descriptions come first (rules O140–O145); this hook is the second layer.

```
skillctl install <path or URL of this folder>
skillctl hook add skill-forced-eval
```

`hook add` warns if any pinned skill still fails the directive-description
rules: a prompt hook on top of weak descriptions measured worse than no hook.

## What it does on each prompt

1. **Exits silently when it should not act:** under Cursor (`CURSOR_VERSION`
   is set; Cursor gets the same check as an always-apply rule because its
   prompt hook cannot inject text), or when the folder has no skills the agent
   can see.
2. **Reads the skills the agent sees**, exactly as the harness does: every
   `SKILL.md` under `.claude/skills/` and `.agents/skills/` in the working
   folder (a skillctl instance links the project's pins there) plus
   `~/.claude/skills/` (global skills).
3. **Shortlists candidates** by matching the prompt against each skill's name
   and description: quoted trigger phrasings from the description that appear
   in the prompt, file paths or extensions mentioned by both, and shared
   keywords. A skill needs a phrase or path hit, or at least two shared
   keywords. At most three candidates.
4. **Injects the check** as additional context when there are candidates:

   ```
   MANDATORY SKILL CHECK. Candidates: db-migrations, org-code-review
   For each: [name] - YES/NO - [reason]. Call Skill(name) for every YES before any other tool.
   ```

   Nothing is printed otherwise, so prompts that match no skill cost nothing.

The script never blocks the prompt: every path exits 0 and malformed input
is ignored. Set `SKILLCTL_FORCED_EVAL_DEBUG=1` to have it log its shortlist
decisions to stderr (shown in the transcript as a hook message).
