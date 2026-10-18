#!/bin/sh
# Package dist/skillctl as skillctl-<version>-macos-universal.tar.gz with a
# SHA-256 file and a rendered Homebrew formula pointing at the GitHub release.
#
# Usage: package.sh <version>
# Environment: SKILLCTL_REPOSITORY (owner/name, default abindran/skillctl)
set -eu
cd "$(dirname "$0")/../.."
version="${1:?version, for example 0.1.0}"
repository="${SKILLCTL_REPOSITORY:-abindran/skillctl}"
archive="skillctl-$version-macos-universal.tar.gz"

test -x dist/skillctl || { echo "dist/skillctl is missing; run scripts/release/build-universal.sh first" >&2; exit 1; }

# Deterministic archive: fixed owner, no extended attributes or resource forks.
(cd dist && COPYFILE_DISABLE=1 tar --uid 0 --gid 0 --numeric-owner -czf "$archive" skillctl)
(cd dist && shasum -a 256 "$archive" > "$archive.sha256")
sha256="$(cut -d' ' -f1 "dist/$archive.sha256")"
url="https://github.com/$repository/releases/download/v$version/$archive"

sed -e "s|@VERSION@|$version|g" \
    -e "s|@URL@|$url|g" \
    -e "s|@SHA256@|$sha256|g" \
    -e "s|@REPOSITORY@|$repository|g" \
    packaging/homebrew/skillctl.rb.in > dist/skillctl.rb

echo "dist/$archive"
cat "dist/$archive.sha256"
echo "dist/skillctl.rb -> $url"
