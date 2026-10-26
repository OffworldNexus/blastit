#!/usr/bin/env bash
# Refuses a release tag that doesn't name the version in Cargo.toml (E4).
#
#   scripts/check-release-tag.sh v0.1.0     # or, in CI, GITHUB_REF_NAME
#
# The published wheels carry the version from Cargo (`dynamic = ["version"]` in
# pyproject.toml), so a tag is only a name for a release if the two agree:
# `v0.1.0` publishes 0.1.0. A tag that says something else — `v0.1.1` on a
# workspace still at 0.1.0, say — would publish 0.1.0 under a release named
# `v0.1.1`, and since PyPI never lets a file be uploaded twice, the mistake
# would cost a version number rather than a minute.
#
# `scripts/dev-version.sh` moves the workspace version for the development
# wheels; this is the check that runs before a release build, and it deliberately
# runs no build at all.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

tag=${1:-${GITHUB_REF_NAME:-}}
if [[ -z $tag ]]; then
    echo "usage: scripts/check-release-tag.sh v<version> (or set GITHUB_REF_NAME)" >&2
    exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
if [[ -z $version ]]; then
    echo "no [workspace.package] version in Cargo.toml" >&2
    exit 1
fi

# `0.1.0-dev42` is a development wheel's version (dev-version.sh), never a
# release: the index it is published to lists it under a `wheels` pre-release,
# and PyPI must never see one.
if [[ $version == *-dev* ]]; then
    echo "Cargo.toml says $version: that is a development version, not a release" >&2
    exit 1
fi

if [[ $tag != "v$version" ]]; then
    echo "the tag is \`$tag\`, but Cargo.toml says $version: tag \`v$version\` instead" >&2
    exit 1
fi

echo "tag $tag names the workspace version $version"
