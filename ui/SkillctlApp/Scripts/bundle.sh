#!/bin/sh
# Build the SwiftUI front end and wrap it as Skillctl.app in ui/SkillctlApp/dist.
#
#   ui/SkillctlApp/Scripts/bundle.sh            # release build
#   ui/SkillctlApp/Scripts/bundle.sh debug      # debug build
#
# The app is unsigned; `open dist/Skillctl.app` runs it locally. Sign with
# `codesign --deep --force --sign "<identity>" dist/Skillctl.app` to hand it on.
set -eu
config="${1:-release}"
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"
swift build -c "$config" 2>&1 | grep -v "^\[" || true
bin="$(swift build -c "$config" --show-bin-path)/SkillctlApp"
test -x "$bin"
app="$here/dist/Skillctl.app"
# `open` only activates an app that is already running, so quit the old copy
# first; otherwise a rebuild followed by `open` keeps showing the stale binary.
pkill -x Skillctl 2>/dev/null || true
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/Skillctl"
cp "$here/Resources/Info.plist" "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
echo "Built $app"
