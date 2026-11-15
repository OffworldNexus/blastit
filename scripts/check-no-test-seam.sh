#!/usr/bin/env bash
# Measures, on a real file, whether the test-only boot seam is in it (`MB.3.64`).
#
# `BLASTIT_TEST_SWAP_POINT` pauses a boot between stage 0's inode record and
# stage 1's re-open, so `critical_php_embed` can rename a pinned object inside
# that window deterministically. It is a **test seam**: in a production binary an
# environment variable that holds a boot open before the listener binds is a
# denial-of-service surface, which is why the whole rendezvous — and this name —
# is compiled only under `debug_assertions`.
#
# `#[cfg(debug_assertions)]` is an assertion until someone checks the artifact,
# so this script checks two directions. Either alone can pass vacuously:
#
#   * `--absent` (the default): the name must NOT appear. Run on the release
#     standalone binary `scripts/build-standalone.sh` packages — the file a user
#     installs — and by `make seam-check` on a release artifact when this
#     checkout has one.
#   * `--present`: the name MUST appear. Run on the debug build (`cargo test`,
#     `make rs-no-python`), so a renamed variable, or a seam deleted from the
#     source, fails here instead of letting `--absent` report a green that means
#     "the string is gone everywhere".
#
# The scan is `grep -a -o -F` over the raw file. The name is one contiguous ASCII
# literal in `.rodata`, so a byte search cannot miss it, and unlike `strings` it
# needs no minimum run length and no binutils — which matters because the
# standalone archives are also built inside `manylinux` images.
#
# Usage: scripts/check-no-test-seam.sh [--absent|--present] PATH
# Exit:  0 the direction holds, 1 it does not, 2 usage or a missing file.
set -euo pipefail

# The one name, spelled once. `crates/blastit/src/stage0.rs` (`TEST_SWAP_POINT_VAR`)
# is where it is defined; the fixture in `scripts/test_packaging_gates.py` fails
# if this script cannot be made to go red, so the two cannot silently drift.
name="BLASTIT_TEST_SWAP_POINT"

mode=absent
case "${1:-}" in
    --absent) mode=absent; shift ;;
    --present) mode=present; shift ;;
    --) shift ;;
esac

if [ "$#" -ne 1 ]; then
    echo "usage: $0 [--absent|--present] PATH" >&2
    exit 2
fi

artifact=$1
if [ ! -f "$artifact" ]; then
    echo "error: $artifact does not exist: point this at a built binary" >&2
    exit 2
fi

# `|| true`: with `pipefail`, a `grep` that finds nothing is a *result* (zero
# hits), not an error — without this the script would exit 1 silently on exactly
# the case it exists to report.
hits=$(grep -a -o -F -- "$name" "$artifact" | wc -l | tr -d ' ' || true)

if [ "$mode" = absent ]; then
    if [ "$hits" -ne 0 ]; then
        echo "error: $artifact contains $name ($hits occurrence(s)), so a shipped boot could be" >&2
        echo "  paused by an environment variable before it binds a listener — a denial-of-service" >&2
        echo "  surface that must exist only in a test build." >&2
        echo "  the seam is compiled under \`debug_assertions\` alone: build the artifact with" >&2
        echo "  \`--release\` (scripts/build-standalone.sh does), and never with" >&2
        echo "  \`-C debug-assertions=yes\` or \`RUSTFLAGS\` that turns it back on." >&2
        exit 1
    fi
    echo "ok   $artifact: no $name (absent, as shipped)"
    exit 0
fi

if [ "$hits" -eq 0 ]; then
    echo "error: $artifact does not contain $name, so the seam is not in a debug build either:" >&2
    echo "  either the rendezvous was deleted from \`crates/blastit/src\` (and" >&2
    echo "  \`critical_php_embed\`'s inode-pin legs no longer place anything), or the name" >&2
    echo "  changed here and in one place only. Both are the drift this direction catches." >&2
    exit 1
fi
echo "ok   $artifact: $name present ($hits occurrence(s)), as a debug build must"
