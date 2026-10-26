#!/usr/bin/env bash
# Prints the CHANGELOG.md entry for a version, or fails if there is none (E4).
#
#   scripts/release-notes.sh v0.1.0 > notes.md    # the notes of the GitHub release
#   scripts/release-notes.sh v0.1.0 >/dev/null    # ... or just "is there one?"
#
# The release's notes are written once, in the changelog, and the tag only names
# them; `Publish` runs this before it builds anything, so a tag with no entry
# stops the run instead of publishing a release nobody described.
#
# The entry is the lines under `## 0.1.0`, up to the next `## ` heading, so the
# changelog stays the one place a release is described:
#
#   ## 0.1.0 (unreleased)   ← this and everything below, until `## 0.0.9 (…)`
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

tag=${1:?usage: scripts/release-notes.sh v<version>}
version=${tag#v}

notes=$(awk -v v="$version" '
    substr($0, 1, 3 + length(v)) == "## " v { inside = 1; next }
    inside && /^## / { exit }
    inside && !start && /^[[:space:]]*$/ { next }
    inside { start = 1; print }
' CHANGELOG.md)

if [[ -z ${notes//[[:space:]]/} ]]; then
    echo "CHANGELOG.md has no \`## $version\` entry: write it before tagging" >&2
    exit 1
fi

printf '%s\n' "$notes"
