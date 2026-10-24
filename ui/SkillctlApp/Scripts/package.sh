#!/bin/sh
# Zip dist/Skillctl.app as dist/Skillctl-<version>-macos.zip with a SHA-256
# file, for the releases page. ditto keeps symlinks, executable bits and the
# signature, which a plain zip or a bare .app in a CI artifact would not.
#
# Usage: ui/SkillctlApp/Scripts/package.sh [<version>]   (default: the app's CFBundleShortVersionString)
set -eu
here="$(cd "$(dirname "$0")/.." && pwd)"
app="$here/dist/Skillctl.app"
test -d "$app" || { echo "$app is missing; run Scripts/bundle.sh first" >&2; exit 1; }
version="${1:-$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")}"
archive="Skillctl-$version-macos.zip"
rm -f "$here/dist/$archive" "$here/dist/$archive.sha256"
ditto -c -k --keepParent "$app" "$here/dist/$archive"
(cd "$here/dist" && shasum -a 256 "$archive" > "$archive.sha256")
echo "$here/dist/$archive"
cat "$here/dist/$archive.sha256"
