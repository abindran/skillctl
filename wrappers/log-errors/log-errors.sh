#!/bin/sh
# log-errors: the lines of a log that matter, with context, deduplicated, capped.
if [ "$#" -eq 0 ]; then
  echo "usage: log-errors.sh <logfile> [pattern]" >&2
  exit 64
fi
file=$1
pattern=${2:-'error|warn|fail|panic|exception|traceback'}
if [ ! -r "$file" ]; then
  echo "log-errors: cannot read $file" >&2
  exit 66
fi
grep -iE -B2 -A2 "$pattern" "$file" | uniq -c | awk '
  { n = $1; sub(/^ *[0-9]+ /, ""); line = $0 }
  n > 1 { line = line " (x" n ")" }
  { shown++ }
  shown <= 200 { print line }
  END { if (shown > 200) printf("... %d more lines cut; narrow the pattern\n", shown - 200) }
'
