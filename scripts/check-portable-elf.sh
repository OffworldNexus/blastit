#!/usr/bin/env bash
# Fails when a build of `blastit` — or the wheel's `_blastit` extension — could
# not run anywhere but the machine that produced it (`MB.17`).
#
# A plain `cargo build -p blastit` is the standalone artifact (`ADR-0010` §4): it
# carries the `php` and `static` capabilities, and nothing it links needs an
# interpreter. Two things make an artifact checkout-bound instead, and both are
# visible in `readelf -d`:
#
#   * `NEEDED libpython3.X.so` — the file only loads where that CPython is, so a
#     server without Python gets "cannot open shared object file" before blastit
#     prints anything;
#   * `RUNPATH`/`RPATH` into a build host path — the uv interpreter cache
#     (`$HOME/.local/share/uv/python/...`) or any `$HOME`, which no other user,
#     container, or server has.
#
# It also refuses `NEEDED libphp*` (`MB.36.6`): PHP is the host's NTS library a
# worker `dlopen`s through a verified descriptor (`ADR-0010` §1), so linking it
# would ship a second copy the discovery gate can neither pin nor verify.
#
# A *system* RUNPATH is refused too, on purpose: the standalone artifact is
# `libc`/`libm`/`libgcc_s`/`ld-linux` and needs no search path at all, and "no
# RUNPATH" is the assertion a reviewer can check without judging a path.
#
# `--allow-origin-rpath` relaxes exactly one thing, for one caller: a musllinux
# wheel. There is no auditwheel for musl, so maturin extracts the non-system
# objects into `blastit.libs/` and points `$ORIGIN/../blastit.libs` at them — a
# *relative* path that makes the wheel relocatable, and the opposite of the
# build-host path this script exists to refuse. Absolute paths stay refused
# everywhere, and the standalone artifact (the default, no flag) still carries no
# search path at all.
#
# # A Python shim is a `.so`, and the two shapes are not the same
#
# `target/py-sapi/libblastit_py3{13,14}-<arch>-<libc>.so` is the one artifact this
# script accepts that is *supposed* to reference an interpreter — and it does so
# with **undefined dynamic symbols**, which is precisely what lets stage 1
# `dlopen` the host's `libpython` first (`RTLD_GLOBAL`) and the shim second
# (`RTLD_LOCAL`). `nm -uD <shim>` therefore shows `Py_InitializeEx`, and that is
# the contract, not a leak: `readelf -d` shows no `NEEDED libpython`, because the
# shim binds nothing until the library is already loaded.
#
# The refusal this script owes is the *other* shape — a shim or a binary that
# carries `NEEDED libpython3.x.so` and would therefore only run on a machine with
# that CPython installed. The distinction is exactly the two `readelf` queries
# below, and `scripts/test_packaging_gates.py` builds one fixture of each shape to
# keep them from collapsing into "any mention of Python is refused".
#
# Usage: scripts/check-portable-elf.sh [--allow-origin-rpath] PATH [PATH...]
set -euo pipefail

allow_origin=0
if [ "${1:-}" = "--allow-origin-rpath" ]; then
    allow_origin=1
    shift
fi

if [ "$#" -eq 0 ]; then
    echo "usage: $0 [--allow-origin-rpath] PATH [PATH...]" >&2
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
        case "$artifact" in
            *.so)
                echo "  fix: build it with \`make py-sapi\` (\`--features extension-module\`). A Python shim" >&2
                echo "  binds the host's libpython at dlopen, so its DT_NEEDED list must never name one:" >&2
                echo "  its unresolved Py_* symbols are the contract, not a leak." >&2 ;;
            *)
                echo "  fix: build the standalone artifact (cargo build -p blastit --locked). The wheel enables" >&2
                echo "  the python capability explicitly through crates/blastit-py/Cargo.toml." >&2 ;;
        esac
        artifact_status=1
    fi

    # `libphp` is never linked either (`ADR-0010` §1): a worker `dlopen`s the
    # host's NTS library and its per-minor shim, so a `NEEDED libphp` is a
    # second, unfixed copy of PHP the artifact would carry — and one the
    # discovery gate cannot pin or verify.
    php="$(printf '%s\n' "$dynamic" | grep -E 'NEEDED.*libphp' || true)"
    if [ -n "$php" ]; then
        echo "error: $artifact links libphp, so it carries its own PHP instead of loading the host's:" >&2
        printf '  %s\n' "$php" >&2
        echo "  fix: build without a crate that links it; a view dlopens libphp (ADR-0010 §1)," >&2
        echo "  and the standalone archive is built by scripts/build-standalone.sh." >&2
        artifact_status=1
    fi

    runpath="$(printf '%s\n' "$dynamic" | grep -E '\((RUNPATH|RPATH)\)' || true)"
    if [ "$allow_origin" -eq 1 ] && [ -n "$runpath" ]; then
        # Keep only the entries that are not purely `$ORIGIN`-relative.
        filtered="$(printf '%s\n' "$runpath" | sed 's/\$ORIGIN[^ :]*//g')"
        if ! printf '%s' "$filtered" | grep -qE '\$|/'; then
            runpath=""
        fi
    fi
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
