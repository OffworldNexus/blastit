#!/usr/bin/env bash
# Builds a glibc standalone archive **inside the oldest container it claims**,
# and refuses to package a host build as a portable one.
#
# The defect this exists for (`MB.3.60`): `scripts/build-standalone.sh` compiled
# on whatever host ran it, so the "portable" archive carried the GitHub runner's
# glibc — 2.39 on `ubuntu-24.04` — and would not start on Debian 12 (2.36) or
# RHEL 9 (2.34). Nothing in the build said which host it was for, because a build
# host is not a decision. An image is: this script builds inside one whose glibc
# *is* the floor, and `scripts/build-standalone.sh --max-glibc` measures the
# finished binary and refuses it if it outgrew that floor.
#
# Usage:
#   scripts/build-standalone-in-image.sh --image IMAGE --target TRIPLE [--max-glibc 2_28]
#       [--out DIR] [--shim-dir DIR] [--require-shims N]
#
# `--image` is the builder *and* the claim: the same tag is the first image
# `scripts/check-exec-env.sh` runs in CI, so "it was built there, and it runs
# there" is one number rather than two documents. For the shipped gnu legs that
# is `quay.io/pypa/manylinux_2_28_<arch>` — the family the wheels are tagged
# `manylinux_2_28` in — so one glibc floor covers both artifact families.
#
# The image supplies the C library, the C compiler and libclang (bindgen needs
# the clang C API for `pg_query`); the Rust toolchain comes from the host
# (`$RUSTUP_HOME`, mounted read-only) exactly as `scripts/build-wheel.sh` mounts
# it, so no leg downloads a second toolchain and the rest of the build keeps
# using one compiler.
#
# `CARGO_TARGET_DIR` is redirected per image under `target/`. That is the point
# of the script: a host `target/` is full of objects compiled against the host's
# glibc, and reusing one would put the runner's floor back into an artifact
# whose whole purpose is a lower one.
#
# **The container owns what it writes (`MB.3.63`).** The host cache is mounted
# read-only and *seeds* a writable `CARGO_HOME` under `/tmp` by copying, because
# cargo extracts `registry/src/<crate>` and `git/checkouts/<dep>` into it — a
# symlink to the mount made the first cold-cache extraction die with
# `failed to create directory ... Read-only file system (os error 30)` while a
# warm cache (already extracted on the host) hid it. Root installs the C
# toolchain and then `setpriv`s to the caller's uid, so one identity writes the
# cache, the target dir and `--out`; nothing is `chown`ed afterwards, and the
# build leaves no stray file in the repository.
#
# A musl archive does not need this: it has no glibc floor to hit, and the Alpine
# leg already builds it natively in `alpine:3.21` (the image it claims).
set -euo pipefail

usage() {
    echo "usage: $0 --image IMAGE --target TRIPLE [--max-glibc 2_28] [--out DIR]" >&2
    echo "          [--shim-dir DIR] [--require-shims N]" >&2
    echo "       (builds the gnu standalone archive inside IMAGE, whose glibc is the floor)" >&2
    exit 2
}

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

image=""
target=""
max_glibc=""
out="dist"
shim_dir=""
require_shims=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --image)
            [ "$#" -ge 2 ] || usage
            image=$2
            shift 2
            ;;
        --target)
            [ "$#" -ge 2 ] || usage
            target=$2
            shift 2
            ;;
        --max-glibc)
            [ "$#" -ge 2 ] || usage
            max_glibc=$2
            shift 2
            ;;
        --out)
            [ "$#" -ge 2 ] || usage
            out=$2
            shift 2
            ;;
        --shim-dir)
            [ "$#" -ge 2 ] || usage
            shim_dir=$2
            shift 2
            ;;
        --require-shims)
            [ "$#" -ge 2 ] || usage
            require_shims=$2
            shift 2
            ;;
        -h|--help)
            sed -n '2,/^set -euo/{ /^set -euo/d; s/^# \{0,1\}//; p }' "${BASH_SOURCE[0]}"
            exit 0
            ;;
        *) echo "$(basename "$0"): unknown argument '$1'" >&2; usage ;;
    esac
done

[ -n "$image" ] || { echo "$(basename "$0"): --image is required: it is the floor" >&2; usage; }
command -v docker >/dev/null 2>&1 || { echo "$(basename "$0"): 'docker' is not on PATH" >&2; exit 2; }

# The triple decides the libc; the floor only means something for gnu, and a musl
# target built here would be an archive whose wheels say `musllinux_1_2` and
# whose compiler was a glibc image.
if [ -z "$target" ]; then
    target=$("${RUSTC:-rustc}" -vV | sed -n 's/^host: //p')
fi
case "$target" in
    *-unknown-linux-gnu) ;;
    *-unknown-linux-musl)
        echo "$(basename "$0"): $target is musl: it needs no glibc and is built natively in" >&2
        echo "  alpine:3.21 by the workflow. Use scripts/build-standalone.sh inside that image." >&2
        exit 2
        ;;
    *) echo "$(basename "$0"): $target is not a Linux target this script builds" >&2; exit 2 ;;
esac

# Without a floor there is nothing to assert and the script would be back to "a
# container happened to be used" — the defect with extra steps.
if [ -z "$max_glibc" ]; then
    echo "$(basename "$0"): --max-glibc is required (e.g. 2_28): it is the number this archive" >&2
    echo "  promises, and the build refuses to package a binary that outgrew it" >&2
    exit 2
fi

arch=${target%%-*}
container_arch=$(docker version --format '{{.Server.Arch}}' 2>/dev/null || true)
case "$container_arch" in
    amd64|"") container_arch=x86_64 ;;
    arm64) container_arch=aarch64 ;;
esac
if [ "$container_arch" != "$arch" ]; then
    echo "$(basename "$0"): $target needs a $arch container and this host runs $container_arch images" >&2
    echo "  build the leg on its native runner: an emulated C compiler is not evidence" >&2
    exit 2
fi

# A path the container cannot see is a path that silently packages nothing:
# only the repository (mounted at /src) and paths inside it mean anything here.
inside() {
    local path=$1
    case "$path" in
        /*)
            case "$path" in
                "$root"/*) printf '/src/%s\n' "${path#"$root"/}" ;;
                *)
                    echo "$(basename "$0"): '$path' is outside the repository, so the container cannot write it" >&2
                    echo "  pass a path inside $root (mounted at /src) instead" >&2
                    exit 2
                    ;;
            esac
            ;;
        *) printf '%s\n' "$path" ;;
    esac
}
out_in=$(inside "$out")
shim_in=""
[ -n "$shim_dir" ] && shim_in=$(inside "$shim_dir")

rustup_home=${RUSTUP_HOME:-$HOME/.rustup}
cargo_home=${CARGO_HOME:-$HOME/.cargo}
if [ ! -d "$rustup_home/toolchains" ]; then
    echo "$(basename "$0"): no Rust toolchain at $rustup_home/toolchains" >&2
    echo "  install one (rustup, or dtolnay/rust-toolchain in CI); this script mounts it" >&2
    echo "  read-only instead of downloading a second one per image (see scripts/build-wheel.sh)" >&2
    exit 2
fi

# One directory per (image, floor): a cached `target/` from a newer image must
# never be linked into this one.
target_subdir="standalone-${max_glibc//./_}"

# One identity writes and reads the build (`MB.3.63`). The container is handed
# `HOST_UID`/`HOST_GID` and drops to them *before the first cargo*, so the cache
# it extracts into, `CARGO_TARGET_DIR` and `--out` all belong to the caller:
# there is no root phase to `chown` back at the end, and no path cargo writes can
# be root-owned or read-only. That starts here — the two bind-mounted
# directories are created by the invoking user, so a directory an older,
# root-running build left behind is refused with the fix named instead of
# failing halfway through a compile.
for d in "$out" "target/$target_subdir"; do
    if ! mkdir -p "$d" 2>/dev/null || [ ! -w "$d" ]; then
        echo "$(basename "$0"): '$d' cannot be written by $(id -un) ($(id -u))" >&2
        echo "  the container builds as uid $(id -u) and writes cargo's target directory" >&2
        echo "  and the archive through this mount; a directory left behind by an older" >&2
        echo "  root-run build must be removed, not reused" >&2
        exit 2
    fi
done

# The trailing space inside the array is load-bearing: `CARGO_NET_OFFLINE` is
# parsed as a strict boolean downstream (maturin, not cargo), and a value that
# swallowed the closing `)` would be neither `true` nor `false`.
# `scripts/build-wheel.sh` spells the same flag the same way.
offline_env=()
if [ "${CARGO_NET_OFFLINE:-}" = "true" ]; then
    offline_env=(-e CARGO_NET_OFFLINE=true )
fi

echo "build-standalone-in-image: $target inside $image (floor $max_glibc)"
docker run --rm \
    -e HOST_UID="$(id -u)" \
    -e HOST_GID="$(id -g)" \
    -e TARGET="$target" \
    -e MAX_GLIBC="$max_glibc" \
    -e OUT="$out_in" \
    -e SHIM_DIR="$shim_in" \
    -e OWED="$require_shims" \
    -e BUILDER_IMAGE="$image" \
    -e RUSTUP_HOME=/host-rustup \
    -e CARGO_HOME=/tmp/build-cargo-home \
    -e CARGO_TARGET_DIR="/src/target/$target_subdir" \
    "${offline_env[@]}" \
    -v "$root:/src" \
    -v "$rustup_home:/host-rustup:ro" \
    -v "$cargo_home:/host-cargo:ro" \
    -w /src \
    "$image" \
    bash -euo pipefail -c '
        # Root does exactly two things in this container: install the C
        # toolchain the image does not carry (`pg_query` builds libpg_query and
        # its bindgen step needs the clang C API by name), and hand the build to
        # HOST_UID. Everything cargo writes afterwards belongs to one identity.
        if command -v apk >/dev/null 2>&1; then
            apk add --no-cache build-base musl-dev pkgconf clang llvm-dev clang19-libclang \
                binutils ca-certificates bash tar gzip xz git >/dev/null
            export LIBCLANG_PATH=/usr/lib/llvm19/lib
        elif command -v dnf >/dev/null 2>&1; then
            # `scripts/dnf-retry.sh` (`MB.74`) is the one retry: the mirror can
            # serve an index its own metadata does not match while it publishes.
            . /src/scripts/dnf-retry.sh
            dnf_install_retry clang binutils tar gzip git >/dev/null
        elif command -v apt-get >/dev/null 2>&1; then
            apt-get update >/dev/null
            apt-get install -y --no-install-recommends build-essential clang libclang-dev \
                pkg-config binutils ca-certificates tar gzip git >/dev/null
        else
            echo "error: $BUILDER_IMAGE has no package manager this script knows" >&2
            exit 1
        fi

        # Dropping to HOST_UID is the point, so an image without a way to do it
        # is refused by name rather than silently building as root.
        command -v setpriv >/dev/null 2>&1 || {
            echo "error: $BUILDER_IMAGE has no setpriv (util-linux), so this build cannot" >&2
            echo "  drop to uid $HOST_UID: one identity must write the cache, the target" >&2
            echo "  directory and the archive (MB.3.63)" >&2
            exit 1
        }

        # No second Rust toolchain: the host one is mounted read-only, and this
        # is the one PATH entry both the container and the dropped process use.
        export PATH="/host-rustup/toolchains/stable-$(uname -m)-unknown-linux-gnu/bin:$PATH"
        if ! command -v cargo >/dev/null 2>&1; then
            toolchain_bin=$(find /host-rustup/toolchains -maxdepth 2 -type d -name bin | head -1)
            export PATH="$toolchain_bin:$PATH"
        fi

        # The build body runs as HOST_UID, so it is a heredoc written straight to
        # a file rather than a quoted string. It is deliberately NOT wrapped in
        # `$(cat <<...)`: manylinux_2_28 is AlmaLinux 8 and ships Bash 4.4, whose
        # parser fails on a comment carrying a closing parenthesis inside a
        # command-substitution heredoc, with `unexpected EOF while looking for
        # matching )`, before a single byte of the build ran (PR #64). The
        # delimiter is backslash-quoted rather than `<<BUILD_BODY` so the body is
        # literal, and NOT apostrophe-quoted because this whole container script
        # is one single-quoted `bash -c` argument: a literal quote ends it early.
        # For the same reason the body below carries no single quote at all, not
        # even in a printf format: it splits the minors with positional parameters.
        # `CARGO_HOME` is a *writable* directory **seeded by copying** the
        # read-only host cache: cargo extracts `registry/src/<crate>` and
        # `git/checkouts/<dep>` into `CARGO_HOME`, so symlinking those at the
        # read-only mount is what made the first cold-cache extraction die with
        #
        #   failed to create directory `/tmp/cargo-home/registry/src/...`
        #   Read-only file system os error 30
        #
        # see MB.3.63. Nothing below writes outside `$CARGO_HOME` in /tmp,
        # `$CARGO_TARGET_DIR` and `$OUT`; the repository is only read.
        cat <<\BUILD_BODY > /tmp/blastit-inner-build.sh
set -euo pipefail
mkdir -p "$HOME" "$CARGO_HOME/registry" "$CARGO_HOME/git"
if [ -d /host-cargo/registry ]; then
    cp -a /host-cargo/registry/. "$CARGO_HOME/registry/"
fi
if [ -d /host-cargo/git ]; then
    cp -a /host-cargo/git/. "$CARGO_HOME/git/"
fi
if [ -f /host-cargo/config.toml ]; then
    cp /host-cargo/config.toml "$CARGO_HOME/config.toml"
fi
cargo --version
echo "build-standalone-in-image: building as uid $(id -u):$(id -g), cache $CARGO_HOME"
echo "build-standalone-in-image: glibc $(getconf GNU_LIBC_VERSION 2>/dev/null || ldd --version | head -1)"
build=(--target "$TARGET" --out "$OUT" --require-shims "$OWED"
       --max-glibc "$MAX_GLIBC" --builder-image "$BUILDER_IMAGE")
# `scripts/build-standalone.sh` owns the feature spelling: this script chooses
# *where* the archive is built, never *what* it carries. What it carries is the
# universal binary from the default features of `blastit` (`python`, `php`,
# `static`, `ADR-0017` decision 1): no `libpython` and no `libphp` in
# `DT_NEEDED`, and a WSGI app still served when the host has a declared CPython
# minor with its shim.
if [ -n "$SHIM_DIR" ]; then
    build+=(--shim-dir "$SHIM_DIR")
fi
# The per-minor CPython shims (`MB.47.2`): `scripts/build-standalone.sh` stages
# them beside the binary so a WSGI view boots out of the box, and they are built
# **here** so their glibc floor is the floor of this image. The minors are those
# in `packaging/languages.tsv` (mounted at /src): `manylinux_2_28` already
# ships `/opt/python/cp<digits>-cp<digits>` for the supported minors, so none is
# downloaded, and `make py-sapi` refuses a shim without `DT_SONAME`, an undefined
# `Py_Initialize*` entry point or the three exports.
# The triple decides the shim libc, not this image: `--target` is the claim, and
# a `-musl` triple here would otherwise get `-gnu`-named shims that staging then
# refuses without naming the libc. `PY_SAPI_REQUIRE_ALL=1` makes a missing
# `/opt/python/cp<digits>` name the interpreter instead of reporting "0 shims".
case "${TARGET##*-}" in
    gnu) shim_libc=gnu ;;
    musl) shim_libc=musl ;;
    *)
        echo "error: $TARGET names no libc this script builds a shim for (gnu, musl)" >&2
        exit 1
        ;;
esac
arch=${TARGET%%-*}
py_minors=$(bash /src/packaging/languages.sh --py-minors)
set -- $py_minors
py_vars=()
for minor in "$@"; do
    digits=${minor//./}
    py_vars+=("PY_SAPI_PYTHON_$digits=/opt/python/cp$digits-cp$digits/bin/python3")
done
[ "${#py_vars[@]}" -gt 0 ] || {
    echo "error: packaging/languages.tsv declares no CPython minor, so no shim can be built" >&2
    exit 1
}
make py-sapi PY_MINORS="$*" PY_SAPI_REQUIRE_ALL=1 \
    PY_SAPI_OUT=/src/target/py-sapi PY_SAPI_ARCH="$arch" PY_SAPI_LIBC="$shim_libc" \
    "${py_vars[@]}"
bash scripts/build-standalone.sh "${build[@]}"
BUILD_BODY
        # The heredoc is literal inside this single-quoted argument, so a stray
        # character in it can silently truncate the generated script. Parse it
        # before chmod (bash -n prints the file and line it rejects).
        if ! bash -n /tmp/blastit-inner-build.sh; then
            echo "error: the generated build body /tmp/blastit-inner-build.sh is not valid bash" >&2
            exit 1
        fi
        chmod 0755 /tmp/blastit-inner-build.sh

        exec setpriv --reuid "$HOST_UID" --regid "$HOST_GID" --clear-groups \
            env HOME=/tmp/build-home \
            bash -euo pipefail /tmp/blastit-inner-build.sh
    '

# The container can exit 0 after a truncated body: a stray quote in the
# single-quoted argument ends it before the archive is written, and the outer
# script then reported a path that does not exist (PR #66). The archive is the
# proof, so it is required here.
archive="$out/blastit-$target.tar.gz"
[ -s "$archive" ] || {
    echo "build-standalone-in-image: the builder exited 0 but wrote no $archive" >&2
    exit 1
}
echo "build-standalone-in-image: $archive"
