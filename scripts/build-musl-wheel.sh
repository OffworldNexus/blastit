#!/usr/bin/env bash
# Builds one **musllinux** wheel, natively, inside an Alpine container.
#
#   apk-based host (CI: a job `container: alpine:3.21`, locally: `docker run … alpine:3.21`)
#   scripts/build-musl-wheel.sh 3.13
#
# A musl wheel cannot come out of the manylinux container the glibc legs use:
# there is no glibc, and the interpreter it must link against is a musl CPython.
# So this leg provisions its own: `uv python install` fetches the
# python-build-standalone musl build for the requested version (the same
# interpreter `python:3.13-alpine` ships is *not* enough — the extension is not
# abi3, so the wheel must be built against the exact CPython the tag names).
#
# Everything else is `scripts/build-wheel.sh`, unchanged: the same dbproxy
# installation, the same README rewrite, the same `readelf` assertions. Only the
# toolchain is provisioned here, so the two libcs cannot drift apart.
#
# Usage: scripts/build-musl-wheel.sh {3.13|3.14} [extra maturin arguments]
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

python=${1:?usage: build-musl-wheel.sh 3.13-or-3.14 [maturin args]}
shift || true

if ! command -v apk >/dev/null 2>&1; then
    echo "error: this script builds a musl wheel and must run on Alpine (no apk found)" >&2
    echo "  locally: docker run --rm -v \"\$PWD\":/src -w /src alpine:3.21 bash scripts/build-musl-wheel.sh $python" >&2
    exit 1
fi

# The C toolchain the shim, dbproxy and pg_query bindgen need. `musl-dev` is the
# libc headers; `clang`/`llvm-dev` are bindgen's libclang. Installed every run so
# a bare `alpine:` image and a pre-baked CI image behave the same.
# `clang19-libclang` is the one package that carries `libclang.so` on Alpine
# (`llvm-dev` ships only `libclang-cpp.so`), and `pg_query`'s bindgen needs the C
# API by name — without it the build dies in bindgen with "Unable to find libclang".
apk add --no-cache \
    build-base musl-dev pkgconf clang llvm-dev clang19-libclang binutils \
    ca-certificates curl bash tar xz patchelf >/dev/null
export LIBCLANG_PATH=/usr/lib/llvm19/lib

# uv (for the musl CPython and for maturin) and rustup, each from its own
# installer: the packaged Alpine cargo is older than the workspace's MSRV (1.88).
if ! command -v uv >/dev/null 2>&1; then
    curl -LsSf https://astral.sh/uv/install.sh | sh >/dev/null
fi
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

if ! command -v cargo >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | bash -s -- -y --profile minimal --default-toolchain stable >/dev/null
fi

# Both CPython minors the wheel ships shims for (`MB.42.3.3`, design §6.1): the
# wheel's `blastit/_bin/` carries `libblastit_py313-…musl.so` and the 3.14 one,
# and `--require-shims py` below refuses a wheel with fewer.
for minor in 3.13 3.14; do
    uv python install "$minor" >/dev/null
done
PY_SAPI_PYTHON_313=$(uv python find 3.13)
PY_SAPI_PYTHON_314=$(uv python find 3.14)
export PY_SAPI_PYTHON_313 PY_SAPI_PYTHON_314
interpreter=$(uv python find "$python")
echo "build-musl-wheel: $("$interpreter" -c 'import sys, sysconfig; print(sys.version.split()[0], sysconfig.get_platform())')"

# No `--target`: on a musl host the default target *is* the musl triple, and
# maturin tags the wheel `musllinux_1_2` from the interpreter's own musl.
# The rustc flag must reach *every* compiler invocation, build scripts included:
# a static-pie build script cannot `dlopen` libclang, and `pg_query`'s bindgen is
# a build script. Exported here (not passed as `--target`) for that reason.
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=-crt-static"
rm -f dist/blastit-*.whl
# `BLASTIT_WHEEL_BUILD_PY_SHIMS=1` makes the host path compile the two CPython
# shims into the same `_bin/` staging directory (`make py-sapi`, no `--target`:
# this *is* the musl triple). `--require-shims py` then fails closed if either is
# missing, so a musllinux wheel can never ship zero shims (the review found one
# that shipped none).
BLASTIT_WHEEL_BUILD_PY_SHIMS=1 PY_MINORS="3.14 3.13" \
PYO3_PYTHON="$interpreter" MATURIN="uvx maturin" \
    bash scripts/build-wheel.sh --interpreter "$interpreter" \
        --compatibility musllinux_1_2 --require-shims py "$@"

# The tag is the promise: a wheel built on glibc and named musllinux installs and
# then fails at `import` (that is the whole reason this leg exists). Verify the
# internal `*.dist-info/WHEEL` `Tag:` lines as well as the filename.
wheel=$(ls -t dist/blastit-*.whl | head -1)
"$interpreter" scripts/check_wheel_tag.py --platform musllinux_1_2 "$wheel"
echo "build-musl-wheel: $(basename "$wheel")"
