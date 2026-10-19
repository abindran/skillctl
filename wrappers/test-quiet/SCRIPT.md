---
name: test-quiet
description: Replaces raw test and build commands in agent sessions. Runs the command you give it, keeps its exit code, prints one line and the last three lines of output on success, and only the failing lines plus the last twenty lines on failure, so a passing test run costs a few tokens instead of thousands.
metadata:
  version: "1.0.0"
---

# test-quiet

```
.claude/scripts/test-quiet/test-quiet.sh cargo test
.claude/scripts/test-quiet/test-quiet.sh npm test -- --silent
```

Tell agents in `AGENTS.md` to run tests and builds through this wrapper. On
success it prints `ok: <command> (N lines suppressed)` and the last three
lines; on failure it prints the lines matching error, fail, panic, assert or
exception (at most 40) and the last 20 lines, then exits with the command's
own status so callers behave exactly as before.
