#!/usr/bin/env bash
# Builds and asserts **one** `py_shims` leg from `scripts/platform_matrix.py`.
#
#   scripts/py_sapi_leg.sh --minor 3.13 --arch x86_64 --libc gnu
#   scripts/py_sapi_leg.sh --minor 3.14 --arch x86_64 --libc musl
#
# Why this script exists (`MB.42.3.3` review): `.github/workflows/py-sapi.yml`
# ran `make py-sapi … PY_SAPI_LIBC=musl` **on an Ubuntu runner**, and the Makefile
# builds no `--target`, so `libblastit_py313-x86_64-musl.so` was linked against
# `libc.so.6` — a glibc artifact wearing a musl name that every contract
# assertion happily accepted. The leg's `builder_image` is therefore part of the
# matrix: a musl leg is built **inside** `alpine:3.21`, where the default target
# is the musl triple, and the `NEEDED` line is asserted against the leg's own
# libc afterwards.
#
# The assertions are the frozen ABI's, on the artifact (never on the flags the
# build was asked for):
#
#   * `DT_SONAME` equals the file name — the loader finds the shim by it;
#   * `nm -D --defined-only` carries `blastit_py_abi_version`, `blastit_py_minor`
#     and `blastit_py_abi_v1` — the handshake stage 1 reads;
#   * `Py_Initialize*` stays **undefined** — the host interpreter's symbols are
#     resolved at `dlopen` (`R1`);
#   * zero `NEEDED libpython*`, and `NEEDED` names the libc the leg claims
#     (`libc.musl-<arch>.so.1` for musl, `libc.so.6` for gnu);
#   * `scripts/check-portable-elf.sh` passes with no relaxation.
#
# Usage: scripts/py_sapi_leg.sh --minor <3.13|3.14> --arch <x86_64|aarch64>
#                               --libc <gnu|musl> [--out DIR] [--image IMAGE]
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
minor=""
arch=""
libc=""
out="$root/target/py-sapi"
inside=""
image="alpine:3.21"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --minor) minor=${2:?--minor requires 3.13 or 3.14}; shift 2 ;;
        --arch) arch=${2:?--arch requires x86_64 or aarch64}; shift 2 ;;
        --libc) libc=${2:?--libc requires gnu or musl}; shift 2 ;;
        --out) out=${2:?--out requires a directory}; shift 2 ;;
        --image) image=${2:?--image requires an image}; shift 2 ;;
        --inside) inside=1; shift ;;
        *) echo "usage: $0 --minor <3.13|3.14> --arch <x86_64|aarch64> --libc <gnu|musl> [--out DIR]" >&2; exit 2 ;;
    esac
done
[ -n "$minor" ] && [ -n "$arch" ] && [ -n "$libc" ] ||
    { echo "usage: $0 --minor <3.13|3.14> --arch <x86_64|aarch64> --libc <gnu|musl>" >&2; exit 2; }

digits=${minor//./}
soname="libblastit_py${digits}-${arch}-${libc}.so"

# A musl leg is built in its own image; the host's toolchain is glibc and no
# `--target` flag can make the *linker* produce a musl binary.
if [ "$libc" = musl ] && [ -z "$inside" ] && [ ! -e "/lib/ld-musl-$(uname -m).so.1" ]; then
    command -v docker >/dev/null 2>&1 || {
        echo "error: a musl $minor shim needs its builder image ($image) and docker is not on PATH" >&2
        exit 1
    }
    echo "py-sapi-leg: $soname in $image (a musl binary cannot be linked from a glibc host)"
    # `alpine:3.21` ships `/bin/sh` and no `bash` before `apk add`, so the leg
    # starts under `sh`: it installs bash and runs the same script again with
    # `--inside`. The arguments arrive as `sh` positionals, so no leg value is
    # ever re-parsed as shell. `bash` is *not* `exec`ed: that would replace the
    # shell and drop the EXIT trap that hands `target/` back to the host uid.
    docker run --rm -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" -v "$root:/src" -w /src "$image" \
        sh -eu -c 'trap "chown -R $HOST_UID:$HOST_GID /src/target 2>/dev/null || true" EXIT; apk add --no-cache bash >/dev/null; bash /src/scripts/py_sapi_leg.sh --inside --minor "$1" --arch "$2" --libc "$3" --out /src/target/py-sapi' \
        sh "$minor" "$arch" "$libc" || {
        echo "error: py-sapi-leg: $soname could not run in $image (bash bootstrap failed)" >&2
        exit 1
    }
    exit 0
fi

if [ "$libc" = musl ]; then
    # Same provisioning as `scripts/build-musl-wheel.sh`: the C toolchain plus a
    # rust recent enough for the workspace's MSRV (`1.88`; Alpine's package is
    # older), and a musl CPython of *this* minor so the shim is compiled with
    # that minor's headers and SONAME.
    if ! command -v apk >/dev/null 2>&1; then
        echo "error: --libc musl must run on Alpine (no apk found)" >&2
        exit 1
    fi
    apk add --no-cache build-base musl-dev pkgconf clang llvm-dev clang19-libclang \
        binutils ca-certificates curl bash tar xz patchelf >/dev/null
    export LIBCLANG_PATH=/usr/lib/llvm19/lib
    if ! command -v uv >/dev/null 2>&1; then
        curl -LsSf https://astral.sh/uv/install.sh | sh >/dev/null
    fi
    export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
    if ! command -v cargo >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
            bash -s -- -y --profile minimal --default-toolchain stable >/dev/null
    fi
    uv python install "$minor" >/dev/null
    export "PY_SAPI_PYTHON_$digits=$(uv python find "$minor")"
fi

# The gnu legs run on a runner whose oldest CPython may be 3.12: fetch the minor
# the leg names when nothing names an interpreter for it yet.
# `PY_SAPI_PYTHON_<digits>` is the Makefile's first stop, and the `dlopen` probe
# proves the shim against *that* interpreter's shared `libpython`.
py_var="PY_SAPI_PYTHON_$digits"
if [ "$libc" = gnu ] && [ -z "${!py_var:-}" ] && command -v uv >/dev/null 2>&1; then
    if ! interpreter=$(uv python find --no-project "$minor" 2>/dev/null); then
        uv python install "$minor" >/dev/null
        interpreter=$(uv python find --no-project "$minor")
    fi
    export "$py_var=$interpreter"
fi

# `PY_SAPI_OUT` must be absolute: the `dlopen` probe hands the path to a test
# binary whose working directory is the crate's, so a relative path there reads as
# "not a file" (found while proving this script).
mkdir -p "$out"
out=$(cd "$out" && pwd)

# `PY_SAPI_PROBE=0` for musl: python-build-standalone ships no shared libpython,
# so the `dlopen` probe cannot run there. The contract above is what the shim
# owes, and it is asserted below; the probe runs on the gnu legs.
probe=1
if [ "$libc" = musl ]; then probe=0; fi
make -C "$root" py-sapi PY_MINORS="$minor" PY_SAPI_OUT="$out" \
    PY_SAPI_ARCH="$arch" PY_SAPI_LIBC="$libc" PY_SAPI_REQUIRE_ALL=1 \
    PY_SAPI_PROBE="$probe"

shim="$out/$soname"
[ -f "$shim" ] || { echo "error: py-sapi-leg: $shim was not built" >&2; exit 1; }
echo "--- $soname"
readelf -d "$shim" | sed -n 's/.*(SONAME).*/&/p'
# No `grep -q` on a pipeline under `set -o pipefail`: `-q` closes the pipe on the
# first match, so the producer's stdout must be consumed instead (`> /dev/null`).
readelf -d "$shim" | grep -E "SONAME).*\[$soname\]" >/dev/null ||
    { echo "error: $shim does not carry DT_SONAME $soname" >&2; exit 1; }
exports=$(nm -D --defined-only "$shim" | awk '{ print $2" "$3 }')
for symbol in blastit_py_abi_version blastit_py_minor blastit_py_abi_v1; do
    printf '%s\n' "$exports" | grep -x "T $symbol" >/dev/null ||
        { echo "error: $shim does not export $symbol" >&2; exit 1; }
    echo "exported: T $symbol"
done
nm -uD "$shim" | grep -E ' Py_Initialize(Ex|FromConfig)$' >/dev/null ||
    { echo "error: $shim carries no undefined Py_Initialize entry point" >&2; exit 1; }
echo "undefined: Py_Initialize* (resolved by the host interpreter)"
if readelf -d "$shim" | grep -E 'NEEDED.*libpython' >/dev/null; then
    echo "error: $shim links a libpython: the shim must use the host interpreter's" >&2
    readelf -d "$shim" | grep NEEDED >&2
    exit 1
fi
needed=$(readelf -d "$shim" | sed -n 's/.*NEEDED.*\[\(.*\)\]/\1/p' | tr '\n' ' ')
case "$libc" in
    musl)
        printf '%s\n' "$needed" | grep -F "libc.musl-${arch}.so.1" >/dev/null ||
            { echo "error: $shim claims musl but needs: $needed" >&2
              echo "  a glibc artifact wearing a musl name is what the review found" >&2
              exit 1; }
        ;;
    gnu)
        printf '%s\n' "$needed" | grep -F 'libc.so.6' >/dev/null ||
            { echo "error: $shim claims gnu but needs: $needed" >&2; exit 1; }
        ;;
esac
echo "NEEDED: $needed(zero libpython, $libc libc)"
scripts/check-portable-elf.sh "$shim" >/dev/null
echo "portable-elf: ok ($soname)"
echo "py-sapi-leg: ok — $soname ($arch-$libc, CPython $minor)"
