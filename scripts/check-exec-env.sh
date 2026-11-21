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
# It does **not** decide an ELF whose own headers name no C library at all —
# neither an `INTERP` nor a `NEEDED` `libc.*` (a static-pie build, for instance).
# That file's libc cannot be read, so no version name in `.gnu.version_r` is
# evidence of a glibc floor, and the verdict is 2 "cannot decide": a floor
# compared from a name nobody attributed to glibc is the guess `MB.3.66` removed,
# and exit 1 would print "a glibc ELF cannot start where there is no glibc" beside
# "interpreter none" (`MB.3.67`). A file with musl evidence is still accepted, and
# so is a glibc one; point this at a dynamically linked artifact otherwise.
#
# **A `GLIBC_*` name is not proof of a glibc dependency.** `.gnu.version_r` lists
# the version nodes of *every* library the binary needs, including ones that are
# not glibc: Alpine's aarch64 `libgcc_s` declares a node spelled `GLIBC_2.0`, so
# the shipped `aarch64-unknown-linux-musl` artifact — `NEEDED
# libc.musl-aarch64.so.1`, interpreter `/lib/ld-musl-aarch64.so.1` — was reported as
# "needs glibc 2.0" and refused in the one image that can run it, while the
# x86_64 musl leg (whose `libgcc_s` needs only `GCC_*` nodes) passed the same
# check (`MB.3.63`). 2.0 is also a floor no aarch64 glibc has (2.17 is the
# architectural minimum), so the number was the parser's, not the file's. The
# libc is read from the ELF itself (`INTERP`, which `readelf` prints as an
# **absolute** path, and `NEEDED libc.*`); when it says musl, the glibc names are
# reported as ignored and never compared — and when the two readings contradict
# each other (a musl loader beside `NEEDED libc.so.6`, or a glibc loader beside
# `NEEDED libc.musl-*`, with or without any `GLIBC_*` name) the verdict is 2
# "cannot decide", never 1 "too old" and never a chosen half.
#
# The same holds for a `GLIBC_ABI_*` node: it names a glibc *feature*, not a
# version of anything, so a non-glibc version script may declare one without the
# file depending on glibc at all. A musl ELF carrying `GLIBC_ABI_DT_RELR` and no
# `GLIBC_<x.y>` name was read as "needs glibc 2.36" and refused in `alpine:3.21`
# (`MB.3.66`); the ABI-node loop now runs only for an ELF whose loader is glibc,
# and a glibc ELF that really needs the node keeps its 2.36 floor.
#
# A tag that exists only on this machine (an image a `docker build` just made) is
# refused for the same reason a wrong tag is: the pull is what proves the tag the
# user will actually get. Push it, or point the check at the public base image.
#
# Exit: 0 every image agrees, 1 an image is too old (or has no glibc for a glibc
# binary), 2 usage, a missing tool, an image that could not be pulled, an image
# whose C library could not be read, or a binary whose own headers name no C
# library to compare at all.
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
#
# Both readings are glibc *floors*, so they are read only from an ELF whose own
# headers say glibc (`INTERP`, or `NEEDED libc.so.6`): on a musl file the whole
# block is skipped and the names are reported as ignored (see the musl paragraph
# above), and a file that names no libc at all never reaches it — that is exit 2
# "cannot decide" (`MB.3.67`).
#
# **A file `readelf` cannot read is exit 2 ("cannot decide"), never exit 1
# ("too old").** `readelf --version-info README.md` writes "Error: Not an ELF
# file" to stderr and exits 1; under `set -euo pipefail` that used to abort the
# assignment with no message at all, so the caller saw the *wrong verdict* with
# no reason — the same class of false reason this check exists to remove
# (`MB.3.61`). The header documents 2 as "an image whose C library could not be
# read"; a binary readelf cannot parse has no C library requirement to read, and
# reporting one would be a guess.
read_version_info() {
    local output
    if ! output=$(readelf --version-info "$binary" 2>&1); then
        echo "check-exec-env: FAIL — cannot read symbol versions from '$(basename "$binary")', so its glibc requirement is unknown:" >&2
        printf '%s\n' "$output" | sed 's/^/  /' >&2
        echo "  a file readelf cannot parse has no version requirement to compare, and" >&2
        echo "  reporting one would be a guess. Pass the ELF binary this check is about." >&2
        exit 2
    fi
    printf '%s\n' "$output"
}

version_info=$(read_version_info)

# The libc is a fact in the binary's own headers, and only that decides whether
# a `GLIBC_*` name below means a glibc dependency (`MB.3.63`). All three reads
# are guarded: an ELF `readelf --version-info` could parse may still have no
# program headers, and under `set -euo pipefail` an unguarded pipeline would end
# the run with no verdict at all.
elf_machine=""
elf_interp=""
elf_needed=""
if header_out=$(readelf -h "$binary" 2>/dev/null); then
    elf_machine=$(printf '%s\n' "$header_out" | sed -n 's/^ *Machine: *//p' | head -n 1 || true)
fi
if program_out=$(readelf -l "$binary" 2>/dev/null); then
    elf_interp=$(
        printf '%s\n' "$program_out" \
            | sed -n 's/.*Requesting program interpreter: \(.*\)\]$/\1/p' \
            | head -n 1 || true
    )
fi
if dynamic_out=$(readelf -d "$binary" 2>/dev/null); then
    elf_needed=$(printf '%s\n' "$dynamic_out" | sed -n 's/.*(NEEDED).*\[\(.*\)\]$/\1/p' || true)
fi
musl_evidence=""
# `readelf` prints the interpreter as an **absolute** path (`/lib/ld-musl-x86_64.so.1`,
# `/lib64/ld-linux-x86-64.so.2`), so the match has to allow the directory: a bare
# `ld-musl-*` case never fired on a real binary, which is how a musl artifact kept
# being read as glibc (`MB.3.63` review).
interp_musl=0
interp_glibc=0
case "$elf_interp" in
    */ld-musl-*|ld-musl-*) interp_musl=1 ;;
    */ld-linux*|ld-linux*) interp_glibc=1 ;;
esac
needs_musl_libc=0
needs_glibc=0
if printf '%s\n' "$elf_needed" | grep -q '^libc\.musl-'; then
    needs_musl_libc=1
fi
if printf '%s\n' "$elf_needed" | grep -qx 'libc\.so\.6'; then
    needs_glibc=1
fi

# One file cannot load two C libraries, and the evidence for that is in `INTERP`
# and `NEEDED` alone — never in a version *name*, which is what made the first
# version of this rule pass impossible files: gated on a `GLIBC_*` node, a glibc
# loader beside `NEEDED libc.musl-*` printed "links musl" and exited 0, and a musl
# loader beside `NEEDED libc.so.6` exited 0 when no version node was there to
# trigger it. Both directions are exit 2 here, before any name is believed.
contradiction=""
if [ "$interp_musl" -eq 1 ] && [ "$needs_glibc" -eq 1 ]; then
    contradiction="a musl interpreter ($elf_interp) beside NEEDED libc.so.6"
fi
if [ "$interp_glibc" -eq 1 ] && [ "$needs_musl_libc" -eq 1 ]; then
    contradiction="a glibc interpreter ($elf_interp) beside NEEDED libc.musl-*"
fi
if [ "$needs_glibc" -eq 1 ] && [ "$needs_musl_libc" -eq 1 ]; then
    contradiction="NEEDED libc.so.6 and NEEDED libc.musl-* in one file"
fi
if [ -n "$contradiction" ]; then
    echo "check-exec-env: FAIL — $(basename "$binary") contradicts itself: $contradiction" >&2
    echo "  machine ${elf_machine:-unknown}; interpreter ${elf_interp:-none}; NEEDED: $(printf '%s ' $elf_needed)" >&2
    echo "  one file cannot load two C libraries, so its requirement cannot be read;" >&2
    echo "  rebuild the artifact (a glibc and a musl input were mixed)" >&2
    exit 2
fi

# Now that the readings agree, the libc is whichever they name.
if [ "$interp_musl" -eq 1 ]; then
    musl_evidence="interpreter $elf_interp"
elif [ "$needs_musl_libc" -eq 1 ]; then
    musl_evidence="NEEDED $(printf '%s\n' "$elf_needed" | grep -m 1 '^libc\.musl-')"
fi

# A file whose own headers name no C library has no libc to compare, and so no
# floor to read from any version name (`MB.3.67`). Without this, a static-pie ELF
# (no `INTERP`, no `NEEDED libc.*`) fell through to the glibc branch below, took
# its `GLIBC_ABI_DT_RELR` name as a 2.36 floor, and refused `alpine:3.21` with
# "a glibc ELF cannot start where there is no glibc" printed beside "interpreter
# none" — two sentences that cannot both be true, and a refusal about a libc the
# file never loaded. Exit 2 says what the evidence supports: nothing.
#
# The check that follows is the evidence for the branch below: past this point a
# floor is compared only for an ELF that names glibc (`INTERP`) or links it
# (`NEEDED libc.so.6`), which is the gate `MB.3.66`'s ABI-node loop needs too.
if [ -z "$musl_evidence" ] \
    && [ "$interp_glibc" -eq 0 ] \
    && [ "$needs_glibc" -eq 0 ]; then
    echo "check-exec-env: FAIL — cannot decide which C library $(basename "$binary") needs, so no image can be judged:" >&2
    echo "  its own headers name none: machine ${elf_machine:-unknown}, interpreter ${elf_interp:-none}, NEEDED: $(printf '%s ' $elf_needed)" >&2
    echo "  a GLIBC_* or GLIBC_ABI_* name in .gnu.version_r is only a floor for a file whose" >&2
    echo "  INTERP or NEEDED says glibc; with neither, comparing one would be a guess, and" >&2
    echo "  refusing the file for a glibc it never named would be a false refusal." >&2
    echo "  point this check at the dynamically linked artifact the image will actually exec" >&2
    exit 2
fi

required=$(
    printf '%s\n' "$version_info" \
        | sed -n 's/.*Name: GLIBC_\([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' \
        | sort -V \
        | tail -n 1
)
# A musl binary needs no glibc whatever names its version needs carry: those
# belong to the version script of `libgcc_s`/`libunwind`, which on aarch64
# spells one of them `GLIBC_2.0`. The slot stays empty so a musl image is not
# refused for it, and the ignored names are printed rather than dropped.
#
# A non-glibc version script can also declare an ABI *node* (`GLIBC_ABI_DT_RELR`),
# which is the name glibc gives a feature and not a version of anything: on a musl
# ELF it belongs to a library that is not glibc, so it is no more a floor than
# `GLIBC_2.0` is, and the ABI-node loop below is skipped for the same reason
# (`MB.3.66`). Both spellings are ignored together, because the question they
# would answer — "which glibc does this file need?" — was already answered by the
# loader and `NEEDED`: none.
ignored_names=""
abi_nodes=""
abi_floor=""
abi_decides=0
if [ -n "$musl_evidence" ]; then
    ignored_names=$(
        {
            printf '%s\n' "$version_info" \
                | sed -n 's/.*Name: \(GLIBC_[0-9][0-9]*\.[0-9][0-9]*\).*/\1/p'
            printf '%s\n' "$version_info" \
                | sed -n 's/.*Name: \(GLIBC_ABI_[A-Za-z0-9_]*\).*/\1/p'
        } | sort -u | tr '\n' ' ' | sed 's/ *$//'
    )
    required=""
    if [ -n "$ignored_names" ] && [ "$quiet" -eq 0 ]; then
        echo "check-exec-env: $(basename "$binary") links musl ($musl_evidence), so $ignored_names in .gnu.version_r is not a glibc requirement"
        echo "  those names are the version script of a library that is not glibc (libgcc_s/libunwind); machine ${elf_machine:-unknown}"
    fi
else
    abi_nodes=$(
        printf '%s\n' "$version_info" \
            | sed -n 's/.*Name: \(GLIBC_ABI_[A-Za-z0-9_]*\).*/\1/p' \
            | sort -u
    )
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
    if [ -n "$abi_floor" ]; then
        highest=$(printf '%s\n%s\n' "${required:-0}" "$abi_floor" | sort -V | tail -n 1)
        if [ "$highest" = "$abi_floor" ] && [ "${required:-0}" != "$abi_floor" ]; then
            abi_decides=1
        fi
        required=$highest
        [ "$required" = "0" ] && required=""
    fi
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
        echo "  the binary's own ELF: machine ${elf_machine:-unknown}, interpreter ${elf_interp:-none}, NEEDED: $(printf '%s ' $elf_needed)" >&2
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
