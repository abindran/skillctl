---
name: quiet-commands
description: Before each Bash call in Claude Code, catches bare test and build commands such as cargo test or npm test in projects that link the test-quiet wrapper, blocks them and hands the agent the exact wrapped command, so passing runs cost a few tokens instead of thousands. Silent in Cursor, in projects without the wrapper, and for commands that are already wrapped, compound, or not tests or builds.
metadata:
  version: "1.0.0"
events: [PreToolUse]
matchers:
  PreToolUse: Bash
timeout: 10
entry: hook.py
---

# quiet-commands

The command rewrite of the LLM optimisation strategy (work item W24), for
when agents keep bypassing the quiet wrappers that `AGENTS.md` tells them to
use. Register it only once the event log shows raw test commands next to
wrapped ones (`skillctl log summary` reports both); until then the
instruction in `AGENTS.md` is cheaper.

```
skillctl install <path or URL of this folder>
skillctl hook add quiet-commands
```

## What it does on each Bash call

1. **Exits silently when it should not act:** under Cursor (`CURSOR_VERSION`
   is set or the payload carries `cursor_version`; Cursor's shell commands run
   through its own `beforeShellExecution` hook, not imported `PreToolUse`
   hooks), when no `.claude/scripts/test-quiet/test-quiet.sh` is linked in the
   working folder or any folder above it, and when the command is not a bare
   test or build command: already wrapped, compound (`&&`, `|`, `;`,
   redirections, `$(…)`), or something else entirely.
2. **Recognises the noisy commands:** `cargo test|build|check|clippy|nextest`,
   `npm|pnpm|yarn|bun test` and `… run test|build|lint|check|typecheck|coverage`,
   `npx jest|vitest|mocha|tsc|eslint|playwright`, `pytest` and
   `python -m pytest`, `tox`, `go test|build|vet`, `mvn`, `gradle`/`gradlew`,
   `make test|check|build|all`, `dotnet test|build`, `swift test|build`,
   `xcodebuild`, `ctest`, `ninja`. Leading `VAR=value` assignments are
   skipped.
3. **Blocks and redirects** (the default): exits 2 with the wrapped command on
   stderr, which Claude Code shows the agent as the reason the call was
   refused:

   ```
   Run tests and builds through the quiet wrapper so only failures reach the context. Run exactly:
   .claude/scripts/test-quiet/test-quiet.sh cargo test
   ```

   The agent re-issues that command, which goes through the normal permission
   flow like any other.
4. **Rewrites in place** when `SKILLCTL_QUIET_REWRITE=1` is set in the hook's
   environment: returns `updatedInput` with the wrapped command. Claude Code
   accepts `updatedInput` only together with `permissionDecision: allow`, so
   the wrapped command then runs **without a permission prompt**. Turn this on
   only where test and build commands are already allow-listed.

Every other path exits 0 without output, and malformed input is ignored. Set
`SKILLCTL_QUIET_COMMANDS_DEBUG=1` to have it log its decisions to stderr.
