#!/usr/bin/env bash
# Sets the development version one commit's wheels are published under (D2).
#
# A wheel's version comes from Cargo (`dynamic = ["version"]` in pyproject.toml),
# and every commit to `develop` needs its own: the index the pilot installs from
# is a moving target, and a client only picks the newest wheel up if the version
# moved.
#
#   scripts/dev-version.sh 42        # 0.1.0 → 0.1.0-dev42 → blastit-0.1.0.dev42-…whl
#
# Three things move together, and all three are needed:
#
# * the workspace version itself;
# * the crates' `path` dependencies on each other, which name the version they
#   require: cargo only matches a *pre-release* against a requirement that names
#   one, so `0.1.0` would no longer accept `0.1.0-dev42`;
# * Cargo.lock, which records every member's version — a build with `--locked`
#   fails while it disagrees with the manifests. It is rewritten line by line
#   rather than with `cargo update --workspace`, which also re-resolves
#   dependencies and would let the release build drift from the tested one.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

number=${1:?usage: scripts/dev-version.sh <build number>}
if [[ ! $number =~ ^[0-9]+$ ]]; then
    echo "the build number must be digits, not \`$number\`" >&2
    exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
if [[ -z $version ]]; then
    echo "no [workspace.package] version in Cargo.toml" >&2
    exit 1
fi
# Cargo versions are semver, so a development version is a *pre-release*:
# `0.1.0-dev42`. maturin normalises that to the PEP 440 spelling a wheel carries,
# `0.1.0.dev42`, which is what the filename shows.
dev="${version}-dev${number}"

sed -i "0,/^version = \"${version}\"$/s//version = \"${dev}\"/" Cargo.toml
for manifest in crates/*/Cargo.toml; do
    sed -i "s/version = \"${version}\", path/version = \"${dev}\", path/" "$manifest"
    member=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$manifest" | head -1)
    sed -i "/^name = \"${member}\"$/{n;s/^version = \"${version}\"$/version = \"${dev}\"/;}" Cargo.lock
done

# A lock that no longer matches the manifests is a build that stops at
# `--locked`; this is the cheapest way to find out now rather than in CI.
cargo metadata --locked --no-deps --format-version 1 >/dev/null
echo "$dev"
