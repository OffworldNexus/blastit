#!/usr/bin/env bash
# Refuses to run a binary in a container older than the one that built it.
#
#   scripts/check-exec-env.sh --image IMAGE --binary PATH
#
# The rule (`MB.3.58`): **a container that runs our binary must be the one that
# built it, or must have a C library at least as new as the builder's.** A
# binary built on an Ubuntu 24.04 runner (glibc 2.39) and executed in
# `debian:bookworm` (2.36) does not fail at the build; it fails at `execve`:
#
#   /lib/x86_64-linux-gnu/libc.so.6: weak version 'GLIBC_2.39' not found
#   (required by target/debug/blastit)
#
# which is why this reads the requirement out of the binary (`readelf
# --version-info` lists every `GLIBC_2.NN` symbol version it needs) instead of
# trusting that "the image is fairly new". Both numbers are printed either way,
# so a passing run says *why* it passed.
#
# Nothing here needs a build-tool container of its own: one `docker run` of the
# image answers with its own glibc, and `readelf` is on the host.
#
# What it deliberately accepts:
#
#   * a binary with no `GLIBC_*` requirement — a musl or fully static build. musl
#     does not version its symbols, and `libc.musl-<arch>.so.1` is ABI-stable
#     across musl 1.x, so the age of the image's libc cannot refuse it. The line
#     says so rather than staying silent.
#   * an image with a glibc at least as new as the binary needs, even when the
#     image is not the builder. That is the second half of the rule, and the only
#     reason a leg may run a runner-built binary at all.
#
# Exit: 0 the two agree, 1 the image is too old (or has no glibc for a glibc
# binary), 2 usage or a missing tool/image.
#
#   scripts/check-exec-env.sh --image debian:trixie-slim --binary target/debug/blastit
#   scripts/check-exec-env.sh --image "${{ matrix.runtime_image }}" \
#       --binary "/tmp/artifact/blastit-x86_64-unknown-linux-gnu/blastit"
set -euo pipefail

usage() {
    echo "usage: $0 --image IMAGE --binary PATH [--quiet]" >&2
    echo "       (the image must be able to run the binary that built it, or be newer)" >&2
    exit 2
}

image=""
binary=""
quiet=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --image)
            [ "$#" -ge 2 ] || usage
            image=$2
            shift 2
            ;;
        --image=*)
            image=${1#--image=}
            shift
            ;;
        --binary)
            [ "$#" -ge 2 ] || usage
            binary=$2
            shift 2
            ;;
        --binary=*)
            binary=${1#--binary=}
            shift
            ;;
        --quiet)
            quiet=1
            shift
            ;;
        -h|--help)
            sed -n '2,40p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "check-exec-env: unknown argument '$1'" >&2
            usage
            ;;
    esac
done
if [ -z "$image" ] || [ -z "$binary" ]; then
    echo "check-exec-env: --image and --binary are both required" >&2
    usage
fi
if [ ! -f "$binary" ]; then
    echo "check-exec-env: no binary at '$binary'" >&2
    exit 2
fi
for tool in docker readelf; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "check-exec-env: '$tool' is not on PATH" >&2
        exit 2
    fi
done

# The highest `GLIBC_<major>.<minor>` the binary requires, plus the `GLIBC_ABI_*`
# nodes, which name *ABI features* rather than a version and are the only floor a
# DT_RELR-linked binary may state: `-z pack-relative-relocs` gives an executable
# `GLIBC_ABI_DT_RELR` (glibc 2.36) and possibly nothing newer than `GLIBC_2.34`,
# so reading only `GLIBC_2.x` would call a 2.36 binary "2.34" — or, when every
# need is an ABI node, "requires no glibc at all", which is how a DT_RELR-only
# binary used to pass this check on an older image.
#
#   GLIBC_ABI_DT_RELR  -> 2.36 (relative relocations, glibc 2.36)
#   any other GLIBC_ABI_* -> refused: this check does not know that feature's
#                            floor, and guessing it is how a gate goes vacuous.
required=$(
    readelf --version-info "$binary" 2>/dev/null \
        | sed -n 's/.*Name: GLIBC_\([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' \
        | sort -V \
        | tail -n 1
)
abi_nodes=$(
    readelf --version-info "$binary" 2>/dev/null \
        | sed -n 's/.*Name: \(GLIBC_ABI_[A-Za-z0-9_]*\).*/\1/p' \
        | sort -u
)
abi_floor=""
while IFS= read -r node; do
    case "$node" in
        "") continue ;;
        GLIBC_ABI_DT_RELR) abi_floor=2.36 ;;
        *)
            echo "check-exec-env: FAIL — $(basename "$binary") needs $node, whose glibc floor this check does not know" >&2
            echo "  add it to scripts/check-exec-env.sh with the glibc release that introduced it" >&2
            exit 2
            ;;
    esac
done <<EOF
$abi_nodes
EOF
# The two floors are compared as versions, not as strings: `2.4 < 2.36 < 2.39`.
abi_decides=0
if [ -n "$abi_floor" ]; then
    highest=$(printf '%s\n%s\n' "${required:-0}" "$abi_floor" | sort -V | tail -n 1)
    if [ "$highest" = "$abi_floor" ] && [ "${required:-0}" != "$abi_floor" ]; then
        abi_decides=1
    fi
    required=$highest
    [ "$required" = "0" ] && required=""
fi

# The image's own glibc, asked of the image. `getconf` is in glibc itself; the
# fallback executes the C library, which prints "GNU C Library (… GLIBC 2.41-…)
# stable release version 2.41." and works on images trimmed of coreutils. A musl
# image answers `musl`, which is not a glibc at all.
provided=$(
    docker run --rm --entrypoint sh "$image" -c '
        if command -v getconf >/dev/null 2>&1; then
            version=$(getconf GNU_LIBC_VERSION 2>/dev/null || true)
            case "$version" in
                glibc\ *) printf "%s\n" "${version#glibc }"; exit 0 ;;
            esac
        fi
        for lib in /lib/*/libc.so.6 /lib/libc.so.6 /usr/lib/*/libc.so.6; do
            [ -e "$lib" ] || continue
            version=$("$lib" 2>/dev/null | sed -n "s/.*version \([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p" | head -n 1)
            if [ -n "$version" ]; then printf "%s\n" "$version"; exit 0; fi
        done
        for loader in /lib/ld-musl-*.so.1; do
            [ -e "$loader" ] && { printf "musl\n"; exit 0; }
        done
        printf "unknown\n"
    ' 2>&1
) || {
    echo "check-exec-env: cannot inspect image '$image':" >&2
    printf '%s\n' "$provided" | sed 's/^/  /' >&2
    exit 2
}

if [ -z "$required" ]; then
    [ "$quiet" -eq 1 ] || echo "check-exec-env: ok — $(basename "$binary") requires no glibc symbol version (musl or static), image $image provides $provided"
    exit 0
fi
if [ "$provided" = "musl" ] || [ "$provided" = "unknown" ]; then
    echo "check-exec-env: FAIL — $(basename "$binary") needs glibc $required, and $image provides $provided" >&2
    echo "  a glibc ELF cannot start where there is no glibc: the loader is missing, not older" >&2
    echo "  fix: run this binary in a glibc image, or build it inside the musl image" >&2
    exit 1
fi

newest=$(printf '%s\n%s\n' "$required" "$provided" | sort -V | tail -n 1)
if [ "$newest" != "$provided" ]; then
    if [ "$abi_decides" -eq 1 ]; then
        loader_says="version '$(printf '%s\n' "$abi_nodes" | tr '\n' ' ' | sed 's/ *$//')' not found"
    else
        loader_says="weak version 'GLIBC_$required' not found"
    fi
    echo "check-exec-env: FAIL — $(basename "$binary") needs glibc $required, $image has $provided" >&2
    echo "  running it there fails at execve with:" >&2
    echo "    $loader_says (required by $(basename "$binary"))" >&2
    echo "  fix: build the binary inside $image (or a builder no older than it), or" >&2
    echo "       raise the image to glibc >= $required — never the binary's floor" >&2
    exit 1
fi

[ "$quiet" -eq 1 ] || echo "check-exec-env: ok — $(basename "$binary") needs glibc $required, $image provides $provided"
exit 0
