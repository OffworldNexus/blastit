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
# below, and refusing on the first one alone would collapse them into "any
# mention of Python is refused".
#
# `--max-glibc <major>_<minor>` measures the C-library floor of everything it is
# given and refuses a file that needs a newer glibc than the one named. The floor
# is a *measurement* of `.gnu.version_r` (`readelf --version-info`), never a
# constant in a manifest: `scripts/build-{deb,rpm}.sh` stamp `Depends:`/`Requires:`
# from the number this prints, so a shim built in a newer image raises the package
# floor instead of shipping a binary the package manager thinks will install.
# `--max-glibc auto` only reports the floor (the caller has no ceiling of its own
# yet, but still wants the measured number).#
# The measurement line is stable and machine-readable:
#
#   max-glibc: 2.41 (/path/to/libblastit_php86-x86_64-gnu.so)
#   needed: libc.so.6 libgcc_s.so.1 libm.so.6 ld-linux-x86-64.so.2
#
# The `needed:` line exists because a package floor alone is not a dependency
# list: the artifact may link anything it likes and `Depends:` would still say
# `libc6`. Every `DT_NEEDED` entry must be the C library, libm, libgcc, the loader,
# or a glibc library that was a separate object before its 2.34 merge (`libdl`,
# `libpthread`, `librt` — all shipped by `libc6`/`glibc`, so they add no
# dependency), or musl's `libc.musl-*`. Anything else is refused, and the caller
# derives its package names from this line.
#
# Usage: scripts/check-portable-elf.sh [--allow-origin-rpath]
#                                      [--max-glibc <major>_<minor>|auto]
#                                      PATH [PATH...]
set -euo pipefail

usage() {
    echo "usage: $0 [--allow-origin-rpath] [--max-glibc <major>_<minor>|auto] PATH [PATH...]" >&2
}

allow_origin=0
glibc_ceiling=""
report_glibc=0
paths=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --allow-origin-rpath)
            allow_origin=1
            shift
            ;;
        --max-glibc)
            if [ "$#" -lt 2 ] || [ -z "${2:-}" ]; then
                echo "error: --max-glibc needs a version, e.g. --max-glibc 2_28 (or auto)" >&2
                exit 2
            fi
            report_glibc=1
            if [ "$2" != auto ]; then
                glibc_ceiling=${2//_/.}
                if ! printf '%s' "$glibc_ceiling" | grep -Eq '^[0-9]+\.[0-9]+$'; then
                    echo "error: --max-glibc '$2' is not <major>_<minor> (e.g. 2_28) or auto" >&2
                    exit 2
                fi
            fi
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        --*)
            echo "error: unknown option $1" >&2
            usage
            exit 2
            ;;
        *)
            paths+=("$1")
            shift
            ;;
    esac
done

if [ "${#paths[@]}" -eq 0 ]; then
    usage
    exit 2
fi

status=0
measured_glibc=""
measured_from=""
measured_needed=""
for artifact in "${paths[@]}"; do
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
                echo "  fix: build the standalone artifact (cargo build -p blastit --locked). The wheel ships" >&2
                echo "  that same binary (pyproject.toml: bindings = \"bin\"), so its capability set is the" >&2
                echo "  binary's: python, php and static, with libpython/libphp only dlopen'ed." >&2 ;;
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

    # This artifact's own floor. A musl ELF names no `GLIBC_` symbol at all, and
    # that is not a failure — it simply contributes nothing to the maximum.
    #
    # The version is any number of components (`GLIBC_2.2.5` is real), because a
    # two-component regex silently skips the oldest needs and reports `none`.
    #
    # `Flags: WEAK` entries are **not** a floor. GNU ld marks a version need weak
    # when every reference to it is weak, and glibc's loader is explicit that such
    # a need is advisory: a binary built on glibc 2.41 carries
    # `pidfd_spawnp@GLIBC_2.39` weakly (the `libc` crate probes for it) and prints
    # only `weak version 'GLIBC_2.39' not found` on 2.34 — then runs. Stamping
    # `libc6 (>= 2.39)` from that would refuse Debian 12 and RHEL 9 for a symbol
    # nothing calls.
    if [ "$report_glibc" -eq 1 ]; then
        artifact_floor="$(readelf --version-info "$artifact" 2>/dev/null \
            | sed -n 's/.*Name: GLIBC_\([0-9][0-9]*\(\.[0-9][0-9]*\)\+\)[[:space:]]\+Flags:[[:space:]]\+\([A-Za-z]*\).*/\1 \3/p' \
            | awk '$2 != "WEAK" { print $1 }' \
            | sort -V \
            | tail -n 1)"
        if [ -n "$artifact_floor" ]; then
            if [ -z "$measured_glibc" ] ||
                [ "$(printf '%s\n%s\n' "$artifact_floor" "$measured_glibc" | sort -V | tail -n 1)" = "$artifact_floor" ]; then
                measured_glibc="$artifact_floor"
                measured_from="$artifact"
            fi
        fi
    fi

    # What the artifact may link, and what the package therefore has to declare.
    # `check-portable-elf.sh` used to constrain only interpreters and RUNPATH, so
    # `/bin/ls` passed: a package whose ELF needed `libselinux` would have shipped
    # with `Depends: libc6`. The allowlist is the runtime contract `ADR-0010` §4
    # states — the C library, libm, libgcc and the loader, plus the glibc libraries
    # that were separate objects before their 2.34 merge (`libdl`, `libpthread`,
    # `librt`): those are still *part of* `libc6`/`glibc`, so they add no package
    # requirement, and a shim compiled against 2.28 headers legitimately needs
    # `libdl.so.2` for `dlopen`. The caller derives its dependency names from the
    # same list.
    #
    # `libgcc_s-<hash>.so*` is auditwheel's vendored name for libgcc (maturin
    # repairs a repaired wheel's `libgcc_s.so.1` that way and points the artefact
    # at it with an `$ORIGIN`-relative RPATH, which is why `--allow-origin-rpath`
    # exists). It is still libgcc — the one library besides libc the contract
    # allows — so a wheel that ships it is within the contract, not outside it
    # (`MB.42.3.3` review: a musllinux wheel was refused for naming its own copy).
    needed="$(printf '%s\n' "$dynamic" \
        | sed -n 's/.*(NEEDED) *Shared library: \[\(.*\)\]/\1/p' \
        | paste -sd ' ' -)"
    for library in $needed; do
        case "$library" in
            libc.so.6 | libm.so.6 | libgcc_s.so.1 | libgcc_s-*.so* | libdl.so.2 | libpthread.so.0 | librt.so.1 | ld-linux-*.so* | libc.musl-*.so*) ;;
            *)
                echo "error: $artifact needs $library, which no supported distribution guarantees:" >&2
                echo "  fix: link only the C library, libm, libgcc and their libc6-provided companions;" >&2
                echo "  a view dlopens its own runtime (libpython/libphp/libz/libssl are never DT_NEEDED)" >&2
                artifact_status=1
                ;;
        esac
        measured_needed="$measured_needed $library"
    done

    if [ "$artifact_status" -eq 0 ]; then
        echo "ok   $artifact: NEEDED ${needed:-none}; no RPATH/RUNPATH"
    fi
    if [ "$artifact_status" -ne 0 ]; then
        status=1
    fi
done

# The caller's package metadata comes from exactly this line, so it names both the
# number and the file that owns it: "libc6 (>= 2.41)" is a claim about
# `libblastit_php86-x86_64-gnu.so`, not about the host that ran the script. The
# `needed:` line is the second half of the contract: the caller maps those names to
# `libc6`/`libgcc-s1` (or `glibc`/`libgcc`) and does not guess which package owns a
# library.
if [ "$report_glibc" -eq 1 ]; then
    if [ -z "$measured_glibc" ]; then
        echo "max-glibc: none (no shipped artifact names a GLIBC_ symbol)"
    else
        echo "max-glibc: $measured_glibc ($measured_from)"
    fi
    echo "needed: $(printf '%s\n' $measured_needed | sort -u | paste -sd ' ' -)"
    if [ -n "$glibc_ceiling" ] && [ -n "$measured_glibc" ]; then
        if [ "$(printf '%s\n%s\n' "$measured_glibc" "$glibc_ceiling" | sort -V | tail -n 1)" != "$glibc_ceiling" ]; then
            echo "error: $measured_from needs glibc $measured_glibc, above the $glibc_ceiling this artifact must run on" >&2
            echo "  (weak version needs are excluded: they do not raise the floor)" >&2
            echo "  build that shim against an older sysroot, or raise --max-glibc and say so in the package docs" >&2
            status=1
        fi
    fi
fi
exit "$status"
