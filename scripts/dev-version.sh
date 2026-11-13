#!/usr/bin/env bash
# Sets the version a development wheel carries (D2).
#
# A wheel's version comes from Cargo (`dynamic = ["version"]` in pyproject.toml).
# Every commit to `develop` builds a wheel and attaches it to that run as an
# artifact, and that wheel must not look like a release: it carries
# `<version>-dev<run number>` instead, so an artifact downloaded from a run —
# or a wheel installed from it — is never mistaken for the release on PyPI.
# Nothing publishes it (E4).
#
#   scripts/dev-version.sh 42        # 0.1.1 -> 0.1.1-dev42 -> blastit-0.1.1.dev42-...whl
#
# Three things move together, and all three are needed:
#
# * the workspace version itself;
# * the crates' `path` dependencies on each other, which name the version they
#   require: cargo only matches a *pre-release* against a requirement that names
#   one, so `0.1.1` would no longer accept `0.1.1-dev42`. The rewrite tolerates
#   a lagging requirement — it matches any release version before `, path =
#   "../`, not the workspace version alone — and afterwards it checks that every
#   intra-workspace requirement names the dev version, naming the offending
#   `file:line` instead of leaving a manifest no build can resolve;
# * Cargo.lock, which records every member's version — a build with `--locked`
#   fails while it disagrees with the manifests. It is rewritten line by line
#   rather than with `cargo update --workspace`, which also re-resolves
#   dependencies and would let the release build drift from the tested one.
set -euo pipefail
shopt -s nullglob

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
# `0.1.1-dev42`. maturin normalises that to the PEP 440 spelling a wheel carries,
# `0.1.1.dev42`, which is what the filename shows.
dev="${version}-dev${number}"

manifests=(crates/*/Cargo.toml)
if (( ${#manifests[@]} == 0 )); then
    echo "no crate manifests found under crates/*/Cargo.toml" >&2
    exit 1
fi

sed -i "0,/^version = \"${version}\"$/s//version = \"${dev}\"/" Cargo.toml
if ! grep -qx "version = \"${dev}\"" Cargo.toml; then
    echo "Cargo.toml has no \`version = \"${dev}\"\` line: the workspace version was not rewritten" >&2
    exit 1
fi

for manifest in "${manifests[@]}"; do
    # Any release version, not just `${version}`: these requirements have
    # lagged the workspace version before, and a rewrite keyed on one exact
    # string then matched nothing and still exited 0.
    sed -i "s/version = \"[0-9][0-9.]*\", path/version = \"${dev}\", path/" "$manifest"
    member=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$manifest" | head -1)
    sed -i "/^name = \"${member}\"$/{n;s/^version = \"${version}\"$/version = \"${dev}\"/;}" Cargo.lock
done

# The only proof that matters, because `cargo metadata --no-deps` below does not
# resolve requirement compatibility: every intra-workspace requirement must now
# name the dev version. A leftover release version (`0.1.1`, or any other) makes
# every build of this dev version stop at dependency resolution with
# "candidate versions found which didn't match".
if ! offenders=$(awk -v dev="$dev" '
    /path = "\.\.\// {
        seen++
        if (index($0, "version = \"" dev "\"") == 0) {
            printf "%s:%d: %s\n", FILENAME, FNR, $0
            bad++
        }
    }
    END {
        if (seen == 0) {
            print "no intra-workspace requirements found under crates/*/Cargo.toml"
            exit 1
        }
        exit (bad > 0 ? 1 : 0)
    }
' "${manifests[@]}"); then
    echo "intra-workspace requirements not rewritten to \`${dev}\`:" >&2
    printf '%s\n' "${offenders:-no intra-workspace requirements found under crates/*/Cargo.toml}" >&2
    echo "a requirement naming a release version does not accept the pre-release \`${dev}\`, so no build of it can resolve" >&2
    exit 1
fi

# A lock that no longer matches the manifests is a build that stops at
# `--locked`; this is the cheapest way to find out now rather than in CI.
cargo metadata --locked --no-deps --format-version 1 >/dev/null
echo "$dev"
