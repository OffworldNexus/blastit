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

# The trailing space inside the array is load-bearing: `test_release_scripts.py`
# scans every `CARGO_NET_OFFLINE=<value>` in the scripts and requires a strict
# boolean, and its `\S*` capture would otherwise take the closing `)` as part of
# the value. `scripts/build-wheel.sh` spells the same flag the same way.
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
    -e CARGO_HOME=/tmp/cargo-home \
    -e CARGO_TARGET_DIR="/src/target/$target_subdir" \
    -e TARGET_SUBDIR="$target_subdir" \
    "${offline_env[@]}" \
    -v "$root:/src" \
    -v "$rustup_home:/host-rustup:ro" \
    -v "$cargo_home:/host-cargo:ro" \
    -w /src \
    "$image" \
    bash -euo pipefail -c '
        cleanup_owner() {
            chown -R "$HOST_UID:$HOST_GID" "/src/$OUT" "/src/target/$TARGET_SUBDIR" 2>/dev/null || true
        }
        trap cleanup_owner EXIT

        # The C toolchain: the image supplies the C library (that is why it was
        # chosen), the compiler, and libclang — `pg_query` builds libpg_query
        # from source and its bindgen step needs the clang C API by name.
        if command -v apk >/dev/null 2>&1; then
            apk add --no-cache build-base musl-dev pkgconf clang llvm-dev clang19-libclang \
                binutils ca-certificates bash tar gzip xz git >/dev/null
            export LIBCLANG_PATH=/usr/lib/llvm19/lib
        elif command -v dnf >/dev/null 2>&1; then
            dnf install -y --setopt=install_weak_deps=False clang binutils tar gzip git \
                >/dev/null
        elif command -v apt-get >/dev/null 2>&1; then
            apt-get update >/dev/null
            apt-get install -y --no-install-recommends build-essential clang libclang-dev \
                pkg-config binutils ca-certificates tar gzip git >/dev/null
        else
            echo "error: $BUILDER_IMAGE has no package manager this script knows" >&2
            exit 1
        fi

        mkdir -p /tmp/cargo-home
        ln -s /host-cargo/registry /tmp/cargo-home/registry 2>/dev/null || true
        ln -s /host-cargo/git /tmp/cargo-home/git 2>/dev/null || true
        [ -f /host-cargo/config.toml ] && cp /host-cargo/config.toml /tmp/cargo-home/config.toml || true
        export PATH="/host-rustup/toolchains/stable-$(uname -m)-unknown-linux-gnu/bin:$PATH"
        if ! command -v cargo >/dev/null 2>&1; then
            toolchain_bin=$(find /host-rustup/toolchains -maxdepth 2 -type d -name bin | head -1)
            export PATH="$toolchain_bin:$PATH"
        fi
        cargo --version
        echo "build-standalone-in-image: glibc $(getconf GNU_LIBC_VERSION 2>/dev/null || ldd --version | head -1)"

        build=(--target "$TARGET" --out "$OUT" --require-shims "$OWED"
               --max-glibc "$MAX_GLIBC" --builder-image "$BUILDER_IMAGE")
        [ -n "$SHIM_DIR" ] && build+=(--shim-dir "$SHIM_DIR")
        bash scripts/build-standalone.sh "${build[@]}"
    '

echo "build-standalone-in-image: $out/blastit-$target.tar.gz"
