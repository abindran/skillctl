---
name: log-errors
description: Replaces reading whole log files in agent sessions. Prints only the lines of a log that match error, warn, fail, panic or exception (or a pattern you pass), each with two lines of context, repeated lines collapsed with a count, capped at 200 lines, so a megabyte of log becomes a screen.
metadata:
  version: "1.0.0"
---

# log-errors

```
.claude/scripts/log-errors/log-errors.sh server.log
.claude/scripts/log-errors/log-errors.sh server.log 'Timeout|ECONNREFUSED'
```

Tell agents in `AGENTS.md` to read logs through this wrapper instead of `cat`
or `tail -n 1000`. It prints the matching lines with two lines of context
before and after, collapses consecutive duplicates with an `(xN)` count, and
stops after 200 lines with a note saying how many were cut.
