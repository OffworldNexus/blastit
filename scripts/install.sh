#!/bin/sh
# The installer is `docs/install.sh`, because that is the file the Docs workflow
# publishes at https://offworldnexus.github.io/blastit/install.sh. This wrapper
# is only for a checkout — `sh scripts/install.sh --help` — so there is one
# installer to maintain, never a copy that drifts.
#
# It `exec`s `sh` rather than the file directly: `docs/install.sh` is piped into
# a shell on the site, and `sh <file>` is the same execution either way.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
exec sh "$root/docs/install.sh" "$@"
