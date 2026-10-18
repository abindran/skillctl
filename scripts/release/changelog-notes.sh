#!/bin/sh
# Print the CHANGELOG.md section for one version (the release notes).
# Exits 1 when the section is missing or empty so a release cannot ship
# without change notes.
#
# Usage: changelog-notes.sh <version>
set -eu
cd "$(dirname "$0")/../.."
version="${1:?version, for example 0.1.0}"

notes="$(awk -v version="$version" '
  /^## \[/ {
    if (found) exit
    found = index($0, "## [" version "]") == 1
    next
  }
  found { print }
' CHANGELOG.md)"

# Drop the link reference definitions at the end of the file and blank edges.
notes="$(printf '%s\n' "$notes" | grep -v '^\[[^]]*\]: ' | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')"

if [ -z "$notes" ]; then
  echo "CHANGELOG.md has no '## [$version]' section with content" >&2
  exit 1
fi
printf '%s\n' "$notes"
