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
#   CODESIGN_IDENTITY="Developer ID Application: …" ui/SkillctlApp/Scripts/bundle.sh   # sign for other Macs
#
# The bundle's version comes from Cargo.toml, so app, CLI and release tag
# always agree. Without CODESIGN_IDENTITY the app is signed ad hoc, which runs
# on this Mac; another Mac needs a right-click Open on first launch (or a
# Developer ID signature plus notarization, see scripts/release/sign-and-notarize.sh).
# Scripts/package.sh then zips the app for the releases page.
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
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -1)"
build="$(git -C "$repo" rev-list --count HEAD 2>/dev/null || echo 1)"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" -c "Set :CFBundleVersion $build" "$app/Contents/Info.plist"
# Sign the whole bundle (the CLI inside it included) in one go.
codesign --force --deep ${CODESIGN_IDENTITY:+--options runtime --timestamp} --sign "${CODESIGN_IDENTITY:--}" "$app"
echo "Built $app ($version, build $build) with $("$app/Contents/Helpers/skillctl" --version)"
