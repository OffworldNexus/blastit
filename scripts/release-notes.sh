#!/usr/bin/env bash
# Prints the CHANGELOG.md entry for a version, or fails if there is none (E4).
#
#   scripts/release-notes.sh v0.1.0 > notes.md                     # the notes of the GitHub release
#   scripts/release-notes.sh v0.1.0 >/dev/null                     # ... or just "is there one?"
#   scripts/release-notes.sh --require-dated v0.1.0 >/dev/null     # also require (YYYY-MM-DD), not (unreleased)
#
# The release's notes are written once, in the changelog, and the tag only names
# them; `Publish` runs this before it builds anything, so a release PR or tag
# with no dated entry stops the run instead of publishing an undescribed release.
#
# The entry is the lines under `## 0.1.0`, up to the next `## ` heading, so the
# changelog stays the one place a release is described:
#
#   ## 0.1.0 (2026-10-27)   ← this and everything below, until `## 0.0.9 (…)`
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

require_dated=0
if [[ ${1:-} == "--require-dated" ]]; then
    require_dated=1
    shift
fi

tag=${1:?usage: scripts/release-notes.sh [--require-dated] v<version>}
version=${tag#v}

heading=$(awk -v v="$version" '
    substr($0, 1, 3 + length(v)) == "## " v && (length($0) == 3 + length(v) || substr($0, 4 + length(v), 1) ~ /[[:space:]]/) {
        print
        exit
    }
' CHANGELOG.md)

if [[ -z $heading ]]; then
    echo "CHANGELOG.md has no \`## $version\` entry: write it before releasing" >&2
    exit 1
fi

if (( require_dated )) && [[ ! $heading =~ ^\#\#[[:space:]]+${version//./\\.}[[:space:]]+\([0-9]{4}-[0-9]{2}-[0-9]{2}\)[[:space:]]*$ ]]; then
    echo "CHANGELOG.md heading is \`$heading\`: stamp today's date as \`## $version (YYYY-MM-DD)\` before releasing" >&2
    exit 1
fi

notes=$(awk -v v="$version" '
    substr($0, 1, 3 + length(v)) == "## " v && (length($0) == 3 + length(v) || substr($0, 4 + length(v), 1) ~ /[[:space:]]/) { inside = 1; next }
    inside && /^## / { exit }
    inside && !start && /^[[:space:]]*$/ { next }
    inside { start = 1; print }
' CHANGELOG.md)

if [[ -z ${notes//[[:space:]]/} ]]; then
    echo "CHANGELOG.md has no body under \`## $version\`: write the release notes before releasing" >&2
    exit 1
fi

printf '%s\n' "$notes"
