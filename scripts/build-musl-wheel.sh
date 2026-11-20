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
# Usage: scripts/build-musl-wheel.sh {3.13|3.14} [--require-shims py|all]
#        [--shims-from DIR] [extra scripts/build-wheel.sh arguments]
#
# `--require-shims all --shims-from dist/shims` is what a release leg passes
# (`publish-pypi.yml`): the four PHP shims must come from the distro legs, and
# no Alpine image can compile them. Both flags belong to `scripts/build-wheel.sh`
# and are parsed here only so this script's own default cannot override them.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

python=${1:?usage: build-musl-wheel.sh 3.13-or-3.14 [--require-shims py|all] [--shims-from DIR] [args]}
shift || true

# This script's own arguments, parsed **before** the `apk` guard: they are pure
# translation, so the self-test hook below can prove them on a non-Alpine host.
# `--require-shims`/`--shims-from` belong to `scripts/build-wheel.sh`; they are
# read here only so this script's default cannot silently override a caller that
# asks for the six-shim release wheel.
require_shims=py
shims_from=
print_build_wheel_args=
build_args=()
while (($#)); do
    case "$1" in
        --require-shims)
            require_shims=${2:?--require-shims requires py or all}
            shift 2
            ;;
        --require-shims=*)
            require_shims=${1#--require-shims=}
            shift
            ;;
        --shims-from)
            shims_from=${2:?--shims-from requires a directory}
            shift 2
            ;;
        --shims-from=*)
            shims_from=${1#--shims-from=}
            shift
            ;;
        --print-build-wheel-args)
            print_build_wheel_args=1
            shift
            ;;
        *)
            build_args+=("$1")
            shift
            ;;
    esac
done

# The command this script runs, assembled in one place: `$1` is the interpreter it
# resolved. Each flag is spelled once, so a caller's `--require-shims all` cannot
# depend on which of two occurrences the downstream parser happens to assign last.
# The array is what `--print-build-wheel-args` prints **and** what the build path
# below executes, so the asserted vector is the vector that runs.
build_wheel_command() {
    wheel_cmd=(bash scripts/build-wheel.sh
               --interpreter "$1" --compatibility musllinux_1_2
               --require-shims "$require_shims")
    if [[ -n $shims_from ]]; then
        wheel_cmd+=(--shims-from "$shims_from")
    fi
    wheel_cmd+=(${build_args[@]+"${build_args[@]}"})
}

# `--print-build-wheel-args` is the self-test hook (`scripts/test_packaging_gates.py`),
# mirroring `scripts/build-wheel.sh --print-glibc-floor`: it runs the shipped
# parser, prints the command and touches nothing — no `apk`, no toolchain, no
# build. The requested minor stands in for the interpreter it would resolve,
# because resolving that needs the `uv` the guard below installs.
if [[ -n $print_build_wheel_args ]]; then
    build_wheel_command "$python"
    printf '%s\n' "${wheel_cmd[*]}"
    exit 0
fi

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
# and the `--require-shims` value below (default `py`) refuses a wheel with fewer.
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
# this *is* the musl triple). The `--require-shims` value then fails closed if
# either is missing, so a musllinux wheel can never ship zero shims (the review
# found one that shipped none).
build_wheel_command "$interpreter"
BLASTIT_WHEEL_BUILD_PY_SHIMS=1 PY_MINORS="3.14 3.13" \
PYO3_PYTHON="$interpreter" MATURIN="uvx maturin" \
    "${wheel_cmd[@]}"

# The tag is the promise: a wheel built on glibc and named musllinux installs and
# then fails at `import` (that is the whole reason this leg exists). Verify the
# internal `*.dist-info/WHEEL` `Tag:` lines as well as the filename.
wheel=$(ls -t dist/blastit-*.whl | head -1)
"$interpreter" scripts/check_wheel_tag.py --platform musllinux_1_2 "$wheel"

# `MB.45`: the shims have to be **in the wheel**, not only in the `_bin/` staging
# directory `scripts/build-wheel.sh` checked before the repack — the repack is
# what moves them, and this leg has no composite-action step to read the zip back.
# `--libc musl` pins the tuple: an `x86_64-gnu` shim in a musllinux wheel is a
# foreign ABI, not a spare (`target/manylinux_*` and a stale `dist/shims` can both
# leave one there).
case "$require_shims" in
    py|all)
        "$interpreter" scripts/check_wheel_shims.py --require "$require_shims" \
            --libc musl "$wheel"
        ;;
    *)
        echo "error: --require-shims '$require_shims' would ship a wheel whose blastit/_bin/ is unchecked" >&2
        echo "  pass \`py\` or \`all\`: this script always ships shims, so an unchecked wheel is a caller bug, not a wildcard" >&2
        exit 2
        ;;
esac
echo "build-musl-wheel: $(basename "$wheel")"
