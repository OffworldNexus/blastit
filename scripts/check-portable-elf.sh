#!/usr/bin/env bash
# Fails when a build of `blastit` — or the wheel's `_blastit` extension — could
# not run anywhere but the machine that produced it (`MB.17`).
#
# A plain `cargo build -p blastit` is the standalone artifact (`ADR-0010` §4): it
# carries the `php` capability, and nothing it links needs an interpreter. Two
# things make an artifact checkout-bound instead, and both are visible in
# `readelf -d`:
#
#   * `NEEDED libpython3.X.so` — the file only loads where that CPython is, so a
#     server without Python gets "cannot open shared object file" before blastit
#     prints anything;
#   * `RUNPATH`/`RPATH` into a build host path — the uv interpreter cache
#     (`$HOME/.local/share/uv/python/...`) or any `$HOME`, which no other user,
#     container, or server has.
#
# A *system* RUNPATH is refused too, on purpose: the standalone artifact is
# `libc`/`libm`/`libgcc_s`/`ld-linux` and needs no search path at all, and "no
# RUNPATH" is the assertion a reviewer can check without judging a path.
#
# Usage: scripts/check-portable-elf.sh PATH [PATH...]
set -euo pipefail

if [ "$#" -eq 0 ]; then
    echo "usage: $0 PATH [PATH...]" >&2
    exit 2
fi

status=0
for artifact in "$@"; do
    artifact_status=0
    if [ ! -f "$artifact" ]; then
        echo "error: $artifact does not exist: build it first" >&2
        status=1
        continue
    fi
    dynamic="$(readelf -d "$artifact")"

    linked="$(printf '%s\n' "$dynamic" | grep -E 'NEEDED.*libpython' || true)"
    if [ -n "$linked" ]; then
        echo "error: $artifact links an interpreter, so it only runs where CPython is installed:" >&2
        printf '  %s\n' "$linked" >&2
        echo "  fix: build the standalone artifact (cargo build -p blastit --locked). The wheel enables" >&2
        echo "  the python capability explicitly through crates/blastit-py/Cargo.toml." >&2
        artifact_status=1
    fi

    runpath="$(printf '%s\n' "$dynamic" | grep -E '\((RUNPATH|RPATH)\)' || true)"
    if [ -n "$runpath" ]; then
        echo "error: $artifact carries a library search path from its build host:" >&2
        printf '  %s\n' "$runpath" >&2
        echo "  fix: a standalone artifact is built against the system C library and needs no RUNPATH;" >&2
        echo "  an interpreter path here means the python capability leaked into the default build." >&2
        artifact_status=1
    fi

    if [ "$artifact_status" -eq 0 ]; then
        needed="$(printf '%s\n' "$dynamic" \
            | sed -n 's/.*(NEEDED) *Shared library: \[\(.*\)\]/\1/p' \
            | paste -sd ' ' -)"
        echo "ok   $artifact: NEEDED ${needed:-none}; no RPATH/RUNPATH"
    fi
    if [ "$artifact_status" -ne 0 ]; then
        status=1
    fi
done
exit "$status"
