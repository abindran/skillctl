#!/bin/sh
# End-to-end scenario for a built binary on a clean machine: init, author and
# seal a skill, attach two clones of a project, survive git clean, repair links,
# diagnose, validate and detach without touching committed project content.
#
# Usage: smoke.sh [path-to-skillctl]   (default: skillctl on PATH)
set -eu
bin="${1:-skillctl}"
work="$(mktemp -d)"
export SKILLCTL_HOME="$work/home"
export HOME="$work/user-home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
# The store is sealed read-only; make it writable again before removing it.
cleanup() { chmod -R u+w "$work" 2>/dev/null || true; rm -rf "$work" || true; }
trap cleanup EXIT

fail() { echo "smoke: $*" >&2; exit 1; }

"$bin" --version | grep -q "registry schema" || fail "--version lacks the registry schema"
"$bin" init
"$bin" new demo
"$bin" validate demo@draft --strict
"$bin" seal demo
test -f "$SKILLCTL_HOME/store/demo/0.1.0/meta.json" || fail "seal did not publish meta.json"
test ! -w "$SKILLCTL_HOME/store/demo/0.1.0/demo/SKILL.md" || fail "sealed SKILL.md is writable"

repo="$work/web"
mkdir "$repo" && git -C "$repo" init -q
echo "# web" > "$repo/README.md"
git -C "$repo" -c user.name=smoke -c user.email=smoke@example.test add README.md
git -C "$repo" -c user.name=smoke -c user.email=smoke@example.test commit -q -m "initial"

(cd "$repo" && "$bin" attach --new web --skills demo)
test -L "$repo/.agents/skills/demo" || fail "missing .agents link"
test -L "$repo/.claude/skills/demo" || fail "missing .claude link"
test -z "$(git -C "$repo" status --porcelain)" || fail "attach left git status dirty"
grep -q "^# >>> skillctl " "$repo/.git/info/exclude" || fail "exclude block missing"

copy="$work/web-copy"
mkdir "$copy" && git -C "$copy" init -q
(cd "$copy" && "$bin" attach --project web)
test -L "$copy/.agents/skills/demo" || fail "second clone did not receive the link"

"$bin" edit demo
"$bin" seal demo
(cd "$copy" && "$bin" add demo@0.1.1)
[ "$(readlink "$repo/.agents/skills/demo")" = "$SKILLCTL_HOME/store/demo/0.1.1/demo" ] || fail "add did not update every instance"

git -C "$repo" clean -fdxq
test ! -e "$repo/.agents/skills/demo" || fail "git clean should have removed the links"
(cd "$repo" && "$bin" relink)
test -L "$repo/.agents/skills/demo" || fail "relink did not restore the links"

(cd "$repo" && "$bin" status)
"$bin" doctor
"$bin" --yes gc
"$bin" validate --strict
"$bin" skills
"$bin" projects

(cd "$repo" && "$bin" detach)
test ! -e "$repo/.agents" || fail "detach left .agents behind"
test ! -e "$repo/.claude" || fail "detach left .claude behind"
test -z "$(git -C "$repo" status --porcelain)" || fail "detach left git status dirty"
test -f "$repo/README.md" || fail "detach touched committed content"
! grep -q "skillctl" "$repo/.git/info/exclude" || fail "exclude block not removed"

echo "smoke: OK ($bin)"
