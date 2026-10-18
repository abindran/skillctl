#!/bin/sh
# Package dist/skillctl as skillctl-<version>-macos-universal.tar.gz with a
# SHA-256 file.
#
# Usage: package.sh <version>
set -eu
cd "$(dirname "$0")/../.."
version="${1:?version, for example 0.1.0}"
archive="skillctl-$version-macos-universal.tar.gz"

test -x dist/skillctl || { echo "dist/skillctl is missing; run scripts/release/build-universal.sh first" >&2; exit 1; }

# Deterministic archive: fixed owner, no extended attributes or resource forks.
(cd dist && COPYFILE_DISABLE=1 tar --uid 0 --gid 0 --numeric-owner -czf "$archive" skillctl)
(cd dist && shasum -a 256 "$archive" > "$archive.sha256")

echo "dist/$archive"
cat "dist/$archive.sha256"
