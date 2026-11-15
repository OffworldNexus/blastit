#!/usr/bin/env bash
# Refuses to run a binary in a container older than the one that built it.
#
#   scripts/check-exec-env.sh --image IMAGE [--image IMAGE ...] --binary PATH
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
# **The image is pulled first, and a pull that fails is a failure.** `docker run`
# on an image that is not local prints
#
#   Unable to find image 'debian:trixie-slim' locally
#
# on stderr and then fails; the pre-`MB.3.60` version of this script folded that
# message into the "image's glibc" slot and reported it as the version. That
# was a false negative in CI, and the same fold on a cached-but-stale image
# would have been a false **pass**. So: pull (loudly failing with docker's own
# message when the tag is wrong or the registry is unreachable), and then accept
# only a glibc `<major>.<minor>` or `musl` as the answer — anything else is a
# refusal, never a comparison.
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
#   * several `--image` values: every image the artifact claims to run in is
#     checked, and the first one that is too old fails the run. A duplicate is
#     inspected once.
#
# A tag that exists only on this machine (an image a `docker build` just made) is
# refused for the same reason a wrong tag is: the pull is what proves the tag the
# user will actually get. Push it, or point the check at the public base image.
#
# Exit: 0 every image agrees, 1 an image is too old (or has no glibc for a glibc
# binary), 2 usage, a missing tool, an image that could not be pulled, or an
# image whose C library could not be read.
#
#   scripts/check-exec-env.sh --image debian:trixie-slim --binary target/debug/blastit
#   scripts/check-exec-env.sh --image debian:bookworm-slim --image debian:trixie-slim \
#       --binary "/tmp/artifact/blastit-x86_64-unknown-linux-gnu/blastit"
set -euo pipefail

usage() {
    echo "usage: $0 --image IMAGE [--image IMAGE ...] --binary PATH [--quiet]" >&2
    echo "       (every image must be able to run the binary that built it, or be newer)" >&2
    exit 2
}

images=()
binary=""
quiet=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --image)
            [ "$#" -ge 2 ] || usage
            images+=("$2")
            shift 2
            ;;
        --image=*)
            images+=("${1#--image=}")
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
            sed -n '2,/^set -euo/{ /^set -euo/d; s/^# \{0,1\}//; p }' "${BASH_SOURCE[0]}"
            exit 0
            ;;
        *)
            echo "check-exec-env: unknown argument '$1'" >&2
            usage
            ;;
    esac
done
if [ "${#images[@]}" -eq 0 ] || [ -z "$binary" ]; then
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

# One image, one verdict: prints the passing line or the refusal and returns
# 0 (agrees), 1 (too old / no glibc for a glibc binary) or 2 (cannot decide), so
# the caller only aggregates.
inspect_image() {
    local image=$1 provided err_file status=0
    err_file=$(mktemp "${TMPDIR:-/tmp}/check-exec-env.XXXXXX")
    # `docker pull` first, for the reason in the header: a run on a missing image
    # prints docker's "Unable to find image" into the answer slot. A pull that
    # cannot complete means this check has *no* version to compare, which is a
    # refusal — never a silent "the image is not old".
    if ! docker pull --quiet "$image" >/dev/null 2>"$err_file"; then
        echo "check-exec-env: FAIL — cannot pull image '$image', so its C library version is unknown:" >&2
        sed 's/^/  /' "$err_file" >&2
        echo "  an image this check cannot obtain has no glibc to compare: reporting a version here" >&2
        echo "  would be a guess. Fix the tag or the registry access, or pre-pull '$image'" >&2
        rm -f "$err_file"
        return 2
    fi

    # The image's own glibc, asked of the image. `getconf` is in glibc itself; the
    # fallback executes the C library, which prints "GNU C Library (… GLIBC 2.41-…)
    # stable release version 2.41." and works on images trimmed of coreutils. A musl
    # image answers `musl`, which is not a glibc at all.
    #
    # stdout is captured alone: docker's own chatter on stderr must never reach the
    # version slot, and `set -e`-safe because the failure is handled by `status`.
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
            printf "unreadable\n"
        ' 2>"$err_file"
    ) || status=$?
    if [ "$status" -ne 0 ]; then
        echo "check-exec-env: FAIL — cannot inspect image '$image' (docker run exited $status), so its C library version is unknown:" >&2
        sed 's/^/  /' "$err_file" >&2
        rm -f "$err_file"
        return 2
    fi
    # Only a real answer is compared. `unreadable`, an empty line, or anything else
    # the image printed is a refusal: a gate that treats "no answer" as a version
    # is a gate that has already passed something it did not read.
    if [ "$provided" != "musl" ] && ! printf '%s' "$provided" | grep -qE '^[0-9]+\.[0-9]+$'; then
        echo "check-exec-env: FAIL — image '$image' did not answer with a C library version, so this check cannot decide:" >&2
        sed 's/^/  /' "$err_file" >&2
        echo "  it answered: ${provided:-<nothing>}" >&2
        echo "  an unreadable version is never compared (a missing image once read as a version)" >&2
        rm -f "$err_file"
        return 2
    fi
    rm -f "$err_file"

    if [ -z "$required" ]; then
        [ "$quiet" -eq 1 ] || echo "check-exec-env: ok — $(basename "$binary") requires no glibc symbol version (musl or static), image $image provides $provided"
        return 0
    fi
    if [ "$provided" = "musl" ]; then
        echo "check-exec-env: FAIL — $(basename "$binary") needs glibc $required, and $image provides musl" >&2
        echo "  a glibc ELF cannot start where there is no glibc: the loader is missing, not older" >&2
        echo "  fix: run this binary in a glibc image, or build it inside the musl image" >&2
        return 1
    fi

    local newest
    newest=$(printf '%s\n%s\n' "$required" "$provided" | sort -V | tail -n 1)
    if [ "$newest" != "$provided" ]; then
        local loader_says
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
        return 1
    fi

    [ "$quiet" -eq 1 ] || echo "check-exec-env: ok — $(basename "$binary") needs glibc $required, $image provides $provided"
    return 0
}

overall=0
seen=" "
for image in "${images[@]}"; do
    # One inspection per distinct image, in the order given: the same image twice
    # (a libc whose floor and smoke image coincide) says nothing new.
    case "$seen" in
        *" $image "*) continue ;;
    esac
    seen="$seen$image "
    status=0
    inspect_image "$image" || status=$?
    if [ "$status" -eq 2 ]; then
        overall=2
    elif [ "$status" -ne 0 ] && [ "$overall" -eq 0 ]; then
        overall=1
    fi
done
exit "$overall"
