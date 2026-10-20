#!/bin/sh
# Build the SwiftUI front end and the skillctl CLI, and wrap both as
# Skillctl.app in ui/SkillctlApp/dist. The app runs the CLI it carries
# (Contents/Helpers/skillctl; not MacOS/, where the case-insensitive default
# filesystem would make it collide with the Skillctl executable), so
# installing the app needs no separate CLI;
# people who want only the command line install the CLI on its own.
#
#   ui/SkillctlApp/Scripts/bundle.sh            # release app, release CLI for this Mac's architecture
#   ui/SkillctlApp/Scripts/bundle.sh debug      # debug app, same CLI
#   UNIVERSAL=1 ui/SkillctlApp/Scripts/bundle.sh          # CLI for Apple silicon and Intel (both rustup targets)
#   SKILLCTL_CLI=/path/to/skillctl ui/SkillctlApp/Scripts/bundle.sh   # use an already built CLI
#
# The app is unsigned; `open dist/Skillctl.app` runs it locally. Sign with
# `codesign --deep --force --sign "<identity>" dist/Skillctl.app` to hand it on.
set -eu
config="${1:-release}"
here="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(cd "$here/../.." && pwd)"
cd "$here"
swift build -c "$config" 2>&1 | grep -v "^\[" || true
bin="$(swift build -c "$config" --show-bin-path)/SkillctlApp"
test -x "$bin"
if [ -n "${SKILLCTL_CLI:-}" ]; then
  cli="$SKILLCTL_CLI"
elif [ "${UNIVERSAL:-0}" = 1 ]; then
  (cd "$repo" && scripts/release/build-universal.sh >/dev/null)
  cli="$repo/dist/skillctl"
else
  (cd "$repo" && cargo build --release --locked 2>&1 | grep -E "^error" || true)
  cli="$repo/target/release/skillctl"
fi
test -x "$cli"
app="$here/dist/Skillctl.app"
# `open` only activates an app that is already running, so quit the old copy
# first; otherwise a rebuild followed by `open` keeps showing the stale binary.
pkill -x Skillctl 2>/dev/null || true
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Helpers" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/Skillctl"
cp "$cli" "$app/Contents/Helpers/skillctl"
chmod 755 "$app/Contents/MacOS/Skillctl" "$app/Contents/Helpers/skillctl"
cp "$here/Resources/Info.plist" "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
echo "Built $app with $("$app/Contents/Helpers/skillctl" --version)"
