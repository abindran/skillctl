#!/bin/sh
# test-quiet: run a command, keep its exit code, print little on success and
# the failing lines on failure. Everything comes from the arguments.
if [ "$#" -eq 0 ]; then
  echo "usage: test-quiet.sh <command> [args...]" >&2
  exit 64
fi
out=$(mktemp)
"$@" >"$out" 2>&1
status=$?
if [ "$status" -eq 0 ]; then
  printf 'ok: %s (%s lines suppressed)\n' "$*" "$(wc -l <"$out" | tr -d ' ')"
  tail -n 3 "$out"
else
  printf 'FAILED (exit %s): %s\n' "$status" "$*"
  grep -inE 'error|fail|panic|assert|exception' "$out" | head -n 40
  printf -- '--- last 20 lines ---\n'
  tail -n 20 "$out"
fi
rm -f "$out"
exit "$status"
