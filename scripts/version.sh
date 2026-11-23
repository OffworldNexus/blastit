#!/usr/bin/env bash
# The workspace version has exactly one source: `Cargo.toml`'s
# `[workspace.package] version` — the line `scripts/dev-version.sh` already reads.
#
#   source scripts/version.sh       # blastit_workspace_version
#   scripts/version.sh              # prints it
#   scripts/version.sh --help
#
# Everything executable that needs the version derives it from here:
# `packaging/package-set.sh` re-exports it as `blastit_package_version`, and
# `scripts/test_install.sh` names the fixture packages it fakes from it.
# `scripts/gen_language_blocks.py` runs this file too, so the download sample
# in `docs/install.md` carries the real file names of this version. The `sed`
# below is deliberately the one `scripts/dev-version.sh` uses: one parser for
# one fact, and a bump of the workspace version is a bump everywhere.
#
# A file that cannot run a shell script at its use site (the install page
# renders in a browser) gets a generated block instead, checked for staleness
# by the same generator.

BLASTIT_VERSION_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# Print the workspace version of the manifest in `$1` (default: the
# repository's `Cargo.toml`). A missing line is a hard error, never an empty
# string a caller could mistake for success.
blastit_workspace_version() {
    local cargo=${1:-$BLASTIT_VERSION_ROOT/Cargo.toml} version
    if [ ! -f "$cargo" ]; then
        printf 'version.sh: %s is missing\n' "$cargo" >&2
        return 1
    fi
    version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$cargo" | head -1)
    if [ -z "$version" ]; then
        printf 'version.sh: no [workspace.package] version in %s\n' "$cargo" >&2
        return 1
    fi
    printf '%s\n' "$version"
}

# --- CLI (only when executed, never when sourced) ---------------------------

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    set -euo pipefail
    case "${1:-}" in
        "") blastit_workspace_version ;;
        -h | --help)
            cat <<'USAGE'
usage: scripts/version.sh [MANIFEST]

Prints the workspace version: the first `version = "…"` line of Cargo.toml
(its `[workspace.package] version`), which is the one line
`scripts/dev-version.sh` bumps.

MANIFEST is a path read instead of the repository's Cargo.toml.

The file is also sourceable: `source scripts/version.sh` then
`blastit_workspace_version [MANIFEST]`, which prints the same value.
USAGE
            ;;
        -*)
            printf 'version.sh: unknown option %s\n' "$1" >&2
            exit 2
            ;;
        *) blastit_workspace_version "$1" ;;
    esac
fi
