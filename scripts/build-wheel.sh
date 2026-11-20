#!/usr/bin/env bash
# Builds the wheel a project installs with `pip install blastit` / `uv add blastit`.
#
# Three artifacts, in this order:
#
#   1. `blastit-dbproxy`, the Python-free binary that holds the real credentials
#      (§2.2, §4). It is built from this workspace and copied into the wheel's
#      Python package at `blastit/_bin/blastit-dbproxy`, mode 0755, because that
#      is where stage 0 `execve`s it from.
#   2. the universal `blastit` binary (`bindings = "bin"`), which is the program
#      the wheel's console script `exec`s. It links no `libpython` and no
#      `libphp`: stage 1 `dlopen`s a per-minor shim and a host `libphp`.
#   3. `python/blastit/*.py`, the pure-stdlib package that ships with them.
#
# `MB.42.3` repacks this into the published `py3-none-*` layout, moving the
# binary and the shims under `blastit/_bin/`.
#
# Stage two (`MB.42.3.3`, design §6.1): `maturin build --bindings bin` writes
# `blastit-<ver>-cp3NN-cp3NN-<platform>.whl`, and
# `scripts/repack_universal_wheel.py` turns it into
# `blastit-<ver>-py3-none-<platform>.whl` — the binary out of `.data/scripts/`
# into `blastit/_bin/blastit`, `blastit-lsp` and the shims added beside it,
# `Root-Is-Purelib: false`, the `py3-none` tag, the three console scripts, and a
# fresh `RECORD`. One artifact then serves CPython 3.12, 3.13 and 3.14.
#
# The shims the wheel carries are staged in `$BLOB_DIR` (`--blobs`, default
# `target/wheel-blobs`):
#
#   * the four PHP shims come from `target/php-sapi` (a distro build — only a
#     distro has `php_config.h`), and they are the *only* embedded ELFs whose
#     floor is not this image's: `check-portable-elf.sh --max-glibc` measures
#     each one against the wheel's floor before it travels;
#   * `blastit-lsp` and the two CPython shims are compiled **inside** the
#     manylinux image, because they must meet the `2_28` floor the wheel claims
#     (built on a developer host they need `GLIBC_2.34`).
#
# `--require-shims py|all` fails closed when the staged set is incomplete (the
# release legs pass `all`: six shims, design §6.1). `make docs-test` passes `py`
# — its runnable fences install the wheel and boot a WSGI view, so the target
# builds the two CPython shims first (`docs-test: py-sapi`) — and the plain host
# path passes nothing: that wheel is a developer artifact.
#
# Platform tag & glibc floor:
#
#   * `--manylinux 2_28` (or `BLASTIT_MANYLINUX=2_28`, used by `make e2e`) builds
#     both `blastit-dbproxy` and the wheel inside `quay.io/pypa/manylinux_2_28_<arch>`,
#     passes `--compatibility manylinux_2_28` to maturin, and enforces `Tag <= 2_28`
#     on `*.dist-info/WHEEL` via `scripts/check_wheel_tag.py` — so a wheel built on
#     a glibc 2.41 developer host still installs on glibc >= 2.28.
#   * Without `--manylinux` (used by `make docs-test` inside `make check`, which
#     runs offline and without Docker, and inside container builds), both binaries
#     link against the ambient libc; `scripts/check_wheel_tag.py` still inspects
#     `*.dist-info/WHEEL` and prints the actual `Tag:`, and the unversioned
#     `linux_<arch>` developer wheel is the one it accepts with `--allow-host-tag`.
#   * The glibc floor is **parsed** from the tag, never prefix-stripped
#     (`MB.42.13`). maturin 1.16 tags a `bindings = "bin"` wheel
#     `manylinux_2_39_x86_64`, so `${tag#manylinux_}` is `2_39_x86_64` — not a
#     `<major>_<minor>` — and the floor parser refused the build, blaming the
#     shim it had just measured. `manylinux_floor` keeps the first two numeric
#     components and ignores a trailing `_<arch>` / `_<arch>_<libc>` suffix;
#     `--manylinux` and `BLASTIT_WHEEL_MAX_GLIBC` win over it, and a platform it
#     cannot read stops the build naming that platform. `--print-glibc-floor
#     <platform>` prints the derived floor and exits: the hook
#     `scripts/test_packaging_gates.py` runs, so the self-test exercises this
#     parse and not a paraphrase of it.
#
# **One platform tag, from one place (`MB.3.58`).** A tag reaches maturin through
# exactly one route: `--compatibility` here (this script enters
# `quay.io/pypa/manylinux_<tag>_<arch>` itself and invokes maturin directly), or
# `maturin-action`'s `manylinux:` input, which names the container maturin runs
# in. Supplying both — as `.github/actions/build-wheel` did — makes maturin see
# two platform tags and it refuses the build (`Expected only one platform tag but
# found 2`). So this script refuses `--compatibility` from its caller when
# `--manylinux` already chose one.
#
# Usage: scripts/build-wheel.sh [--manylinux 2_28] [extra arguments for `maturin build`]
#
# The README is rewritten to absolute links for the duration of the build and
# restored afterwards (see the `trap` below), so the tree is unchanged when this
# exits, successfully or not.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

manylinux_ver="${BLASTIT_MANYLINUX:-}"
out_dir="dist"
extra_args=()
requested_interp=""
blob_dir="${BLASTIT_WHEEL_BLOBS:-}"
require_shims="${BLASTIT_WHEEL_REQUIRE_SHIMS:-}"
shims_from="${BLASTIT_WHEEL_SHIMS_FROM:-}"
print_floor=""
# The glibc floor a wheel claims, derived from its platform (`MB.42.13`).
#
# A maturin `bindings = "bin"` wheel is tagged `manylinux_2_39_x86_64`, so the
# floor is the tag's first two numeric components; a trailing `_<arch>` (or a
# newer pip's `_<arch>_<libc>`) is not part of it. Stripping the `manylinux_`
# prefix instead handed `2_39_x86_64` to `--max-glibc`, which refused it and made
# the message blame the shim that had just been measured.
#
# `--manylinux` wins (it chose the tag), then `BLASTIT_WHEEL_MAX_GLIBC`. A
# platform this cannot read is an error naming that platform: a wheel whose glibc
# floor nothing can state must not be built.
manylinux_floor() {
    local platform=$1
    if [[ -n $manylinux_ver ]]; then
        printf '%s\n' "$manylinux_ver"
        return 0
    fi
    if [[ -n ${BLASTIT_WHEEL_MAX_GLIBC:-} ]]; then
        printf '%s\n' "$BLASTIT_WHEEL_MAX_GLIBC"
        return 0
    fi
    if [[ $platform =~ ^manylinux_([0-9]+)_([0-9]+)(_[A-Za-z0-9_]+)?$ ]]; then
        printf '%s\n' "${BASH_REMATCH[1]}_${BASH_REMATCH[2]}"
        return 0
    fi
    case "$platform" in
        musllinux_*)
            echo "error: platform '$platform' is musllinux: it names a musl version, not a glibc floor" >&2
            ;;
        *)
            echo "error: cannot derive a glibc floor from platform '$platform' (want manylinux_<major>_<minor>_<arch>); pass --manylinux 2_28" >&2
            ;;
    esac
    return 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --print-glibc-floor)
            print_floor=${2:?--print-glibc-floor requires a platform such as manylinux_2_28_x86_64}
            shift 2
            ;;
        --print-glibc-floor=*)
            print_floor=${1#--print-glibc-floor=}
            shift
            ;;
        --manylinux)
            manylinux_ver=${2:?--manylinux requires a version such as 2_28}
            shift 2
            ;;
        --manylinux=*)
            manylinux_ver=${1#--manylinux=}
            shift
            ;;
        --blobs)
            blob_dir=${2:?--blobs requires a directory}
            shift 2
            ;;
        --blobs=*)
            blob_dir=${1#--blobs=}
            shift
            ;;
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
        -o|--out)
            out_dir=${2:?$1 requires an output directory}
            shift 2
            ;;
        --out=*)
            out_dir=${1#--out=}
            shift
            ;;
        -i|--interpreter)
            requested_interp=${2:?$1 requires an interpreter}
            extra_args+=("$1" "$2")
            shift 2
            ;;
        --interpreter=*)
            requested_interp=${1#--interpreter=}
            extra_args+=("$1")
            shift
            ;;
        *)
            extra_args+=("$1")
            shift
            ;;
    esac
done

# `--print-glibc-floor` is the self-test hook (`scripts/test_packaging_gates.py`):
# it runs the shipped parser, prints the floor and touches nothing — no blob
# directory, no README rewrite, no build.
if [[ -n $print_floor ]]; then
    manylinux_floor "$print_floor"
    exit 0
fi

# The staged `blastit/_bin/` payload for the repack: `blastit-lsp` and the shims.
blob_dir=${blob_dir:-target/wheel-blobs}
mkdir -p "$blob_dir"
blob_abs=$(cd "$blob_dir" && pwd)
rm -f "$blob_abs"/blastit-lsp "$blob_abs"/libblastit_*.so

case "$require_shims" in
    ""|none|py|all) ;;
    *)
        echo "error: --require-shims takes py or all, not '$require_shims'" >&2
        exit 2
        ;;
esac

# A wheel carries the shims of **its own** `(arch, libc)`: stage 1 `dlopen`s
# them, so another target's library is a foreign ABI, not a spare.
stage_shims() {
    local want_arch=$1 want_libc=$2 dir shim staged=0
    for dir in target/py-sapi target/php-sapi; do
        [ -d "$dir" ] || continue
        for shim in "$dir"/libblastit_*"-$want_arch-$want_libc".so; do
            [ -f "$shim" ] || continue
            install -m 0755 "$shim" "$blob_abs/"
            staged=$((staged + 1))
        done
    done
    echo "build-wheel: staged $staged shim(s) from this host for $want_arch-$want_libc"
}

# Every embedded ELF meets the wheel's floor, or this wheel is not shippable: a
# `manylinux_2_28` tag on a file that needs 2.34 is a lie `pip` cannot detect.
check_blob_floors() {
    local floor=$1 shim
    [ -n "$floor" ] || return 0
    for shim in "$blob_abs"/*.so; do
        [ -f "$shim" ] || continue
        scripts/check-portable-elf.sh --max-glibc "$floor" "$shim" >/dev/null || {
            echo "error: $(basename "$shim") outgrew the wheel's glibc floor ($floor)" >&2
            echo "  build it inside the builder image (py shims: manylinux_2_28 has no shared libpython, so \`make py-sapi PY_SAPI_PROBE=0\` there)" >&2
            exit 1
        }
    done
}

assert_shims() {
    # `py` counts the *CPython* shims and `all` the design's six: a count over
    # every `libblastit_*.so` let two PHP shims satisfy `--require-shims py`
    # (review of `MB.42.3.3`), so a wheel that shipped no CPython shim at all
    # still passed.
    local py php
    py=$(find "$blob_abs" -maxdepth 1 -name 'libblastit_py3*.so' | wc -l)
    php=$(find "$blob_abs" -maxdepth 1 -name 'libblastit_php8*.so' | wc -l)
    case "$require_shims" in
        ""|none) return 0 ;;
        py)
            if [ "$py" -lt 2 ]; then
                echo "error: --require-shims py needs 2 CPython shims, $blob_abs holds $py" >&2
                echo "  build them first (\`make py-sapi\`, or let the builder image compile them)" >&2
                exit 1
            fi
            ;;
        all)
            if [ "$py" -lt 2 ] || [ "$php" -lt 4 ]; then
                echo "error: --require-shims all needs 2 CPython + 4 PHP shims, $blob_abs holds $py + $php" >&2
                echo "  PHP shims come from a distro leg (\`make php-sapi\`, or --shims-from a php-sapi artifact); a manylinux/CI build cannot compile them" >&2
                exit 1
            fi
            ;;
    esac
}

# `--shims-from DIR` stages shims a previous job built (the `shim-*` artifacts
# of `php-sapi.yml`, a release leg's `target/php-sapi`): the only way a wheel
# gets its four PHP shims, because no PHP headers exist in the wheel builder.
read_shims() {
    local dir=$1 shim staged=0
    [ -n "$dir" ] || return 0
    [ -d "$dir" ] || { echo "error: --shims-from $dir is not a directory" >&2; exit 2; }
    for shim in "$dir"/*.so; do
        [ -f "$shim" ] || continue
        install -m 0755 "$shim" "$blob_abs/"
        staged=$((staged + 1))
    done
    echo "build-wheel: staged $staged shim(s) from $dir"
}

# `--manylinux` already chose the platform, and the path below passes it to
# maturin as `--compatibility manylinux_<tag>`. A caller that also passed
# `--compatibility` would hand maturin two tags and it refuses the build
# (`MB.3.58`: `Expected only one platform tag but found 2`), after a full compile.
if [[ -n $manylinux_ver ]]; then
    for arg in ${extra_args[@]+"${extra_args[@]}"}; do
        case "$arg" in
            --compatibility|--compatibility=*)
                echo "error: --manylinux $manylinux_ver already sets the platform tag;" >&2
                echo "  do not also pass $arg — maturin refuses two platform tags" >&2
                exit 2
                ;;
        esac
    done
fi

# `MATURIN` overrides how maturin is found: `uvx maturin` by default (nothing to
# install), `maturin` in a build image that already has it on PATH.
maturin=(uvx maturin)
if [[ -n ${MATURIN:-} ]]; then
    read -r -a maturin <<<"$MATURIN"
fi

if command -v uv >/dev/null; then
    link_python=(uv run --no-project python)
elif [[ -n ${PYO3_PYTHON:-} ]]; then
    link_python=("$PYO3_PYTHON")
else
    echo "the README rewrite needs uv or PYO3_PYTHON: install uv, or point PYO3_PYTHON at an interpreter" >&2
    exit 1
fi

# Disk-backed default scratch, like the mutants scratch: /tmp is tmpfs (RAM) on this
# host, and this rewrite copies README.md aside for the duration of the build.
readme=$(mktemp "${TMPDIR:-/var/tmp}/blastit-README.XXXXXX")
cp README.md "$readme"
scratch=$(mktemp -d "${TMPDIR:-/var/tmp}/blastit-wheel.XXXXXX")
restore_readme() {
    cp "$readme" README.md
    rm -f "$readme"
    rm -rf "$scratch"
}
trap restore_readme EXIT
"${link_python[@]}" scripts/readme-absolute-links.py

if [[ -n $manylinux_ver ]]; then
    raw_arch=$(uname -m)
    case "$raw_arch" in
        x86_64|amd64) arch=x86_64 ;;
        aarch64|arm64) arch=aarch64 ;;
        *)
            echo "error: unsupported host architecture '$raw_arch' for --manylinux $manylinux_ver" >&2
            exit 1
            ;;
    esac

    # Match the CPython minor the caller asked for (`--interpreter`, `PYO3_PYTHON`,
    # or uv's default >= 3.13) so `/opt/python/cp3NN-cp3NN/bin/python` inside the
    # manylinux container produces the wheel `e2e/` expects.
    probe_py=${requested_interp:-${PYO3_PYTHON:-}}
    if [[ -n $probe_py ]] && command -v "$probe_py" >/dev/null 2>&1; then
        py_minor=$("$probe_py" -c 'import sys; print(f"{sys.version_info[0]}.{sys.version_info[1]}")')
    elif command -v uv >/dev/null 2>&1; then
        py_bin=$(uv python find --no-project '>=3.13')
        py_minor=$("$py_bin" -c 'import sys; print(f"{sys.version_info[0]}.{sys.version_info[1]}")')
    else
        py_minor="3.13"
    fi
    py_digits=${py_minor//./}
    container_py="/opt/python/cp${py_digits}-cp${py_digits}/bin/python"

    base_image="quay.io/pypa/manylinux_${manylinux_ver}_${arch}"
    builder_image="blastit-manylinux-${manylinux_ver}:${arch}"
    # The PHP shims only a distro can compile (`php_config.h`) travel into the
    # build from this host; `blastit-lsp` and the CPython shims are compiled
    # inside the image below, so their floor is the floor the wheel claims.
    stage_shims "$arch" gnu
    wheel_platform="manylinux_${manylinux_ver}_${arch}"
    if ! docker image inspect "$builder_image" >/dev/null 2>&1; then
        echo "build-wheel: provisioning $builder_image from $base_image (+ clang for bindgen)"
        docker build -t "$builder_image" - <<DOCKER
FROM ${base_image}
RUN dnf install -y --setopt=install_weak_deps=False clang && dnf clean all
DOCKER
    fi

    rustup_home=${RUSTUP_HOME:-$HOME/.rustup}
    cargo_home=${CARGO_HOME:-$HOME/.cargo}
    # Resolve the maturin executable on the host first (it is itself a
    # manylinux_2_17 binary), so the container needs neither network nor a
    # writable uv cache lock.
    "${maturin[@]}" --version >/dev/null
    if [[ ${maturin[0]} != "uvx" ]] && command -v "${maturin[0]}" >/dev/null 2>&1; then
        maturin_bin=$(readlink -f "$(command -v "${maturin[0]}")")
    else
        maturin_bin=$(find "$(uv cache dir)" -name maturin -type f -executable | head -1)
    fi
    if [[ -z $maturin_bin || ! -x $maturin_bin ]]; then
        echo "error: could not locate standalone maturin executable for --manylinux build" >&2
        exit 1
    fi
    mkdir -p "$out_dir" "target/manylinux_${manylinux_ver}" python/blastit/_bin
    out_abs=$(cd "$out_dir" && pwd)
    # Clean older wheels from `$out_dir` so prebuilt e2e consumers (`e2e/prebuilt_wheel.py`)
    # never see both an old host `2_39` wheel and the new `manylinux_2_28` wheel.
    rm -f "$out_abs"/blastit-*.whl

    offline_env=(-e CARGO_NET_OFFLINE=false )
    if [[ ${CARGO_NET_OFFLINE:-} == "true" ]]; then
        offline_env=(-e CARGO_NET_OFFLINE=true )
    fi

    docker run --rm \
        -e HOST_UID="$(id -u)" \
        -e HOST_GID="$(id -g)" \
        -e BLOB_DIR=/blobs \
        -e RUSTUP_HOME=/host-rustup \
        -e CARGO_HOME=/tmp/cargo-home \
        -e CARGO_TARGET_DIR="/src/target/manylinux_${manylinux_ver}" \
        "${offline_env[@]}" \
        -v "$root:/src" \
        -v "$out_abs:/wheel-out" \
        -v "$blob_abs:/blobs" \
        -v "$rustup_home:/host-rustup:ro" \
        -v "$cargo_home:/host-cargo:ro" \
        -v "$maturin_bin:/usr/local/bin/maturin:ro" \
        -w /src \
        "$builder_image" \
        bash -euo pipefail -c '
            # `MB.3.63`: cargo extracts `registry/src/<crate>` and
            # `git/checkouts/<dep>` into CARGO_HOME, so CARGO_HOME has to be a
            # writable *copy* of the read-only host cache. Symlinking those two
            # at the mount made the first cold-cache extraction die with
            # `failed to create directory ... Read-only file system (os error 30)`
            # on a runner whose cache held the tarballs but not the extraction.
            mkdir -p /tmp/cargo-home/registry /tmp/cargo-home/git
            if [ -d /host-cargo/registry ]; then
                cp -a /host-cargo/registry/. /tmp/cargo-home/registry/
            fi
            if [ -d /host-cargo/git ]; then
                cp -a /host-cargo/git/. /tmp/cargo-home/git/
            fi
            [ -f /host-cargo/config.toml ] && cp /host-cargo/config.toml /tmp/cargo-home/config.toml || true
            export PATH="/host-rustup/toolchains/stable-'"${arch}"'-unknown-linux-gnu/bin:$PATH"
            if ! command -v cargo >/dev/null 2>&1; then
                toolchain_bin=$(find /host-rustup/toolchains -maxdepth 2 -type d -name bin | head -1)
                export PATH="${toolchain_bin}:$PATH"
            fi
            cleanup_owner() {
                chown -R "$HOST_UID:$HOST_GID" /wheel-out "/src/target/manylinux_'"${manylinux_ver}"'" /src/python/blastit/_bin 2>/dev/null || true
            }
            trap cleanup_owner EXIT
            cargo build --release --locked -p blastit-dbproxy --bin blastit-dbproxy
            install -D -m 0755 "/src/target/manylinux_'"${manylinux_ver}"'/release/blastit-dbproxy" /src/python/blastit/_bin/blastit-dbproxy
            # The editor binary and the per-minor CPython shims are compiled
            # **here**, inside the image whose glibc is the floor the wheel names
            # (`MB.42.3.3`, design §6.1): built on the host they need 2.34, and a
            # `manylinux_2_28` tag over a 2.34 binary is a promise `pip` cannot
            # check. `PY_SAPI_PROBE=0` skips only the `dlopen` probe, which needs
            # a shared libpython manylinux does not ship; `.github/workflows/
            # py-sapi.yml` is where that probe runs.
            cargo build --release --locked -p blastit-lsp
            install -D -m 0755 "/src/target/manylinux_'"${manylinux_ver}"'/release/blastit-lsp" "$BLOB_DIR/blastit-lsp"
            ( cd /src && make py-sapi PY_MINORS="3.14 3.13" PY_SAPI_OUT="$BLOB_DIR" \
                PY_SAPI_PROBE=0 PY_SAPI_ARCH='"${arch}"' PY_SAPI_LIBC=gnu \
                PY_SAPI_PYTHON_313=/opt/python/cp313-cp313/bin/python3 \
                PY_SAPI_PYTHON_314=/opt/python/cp314-cp314/bin/python3 )
            maturin build --release --out /wheel-out \
                --interpreter "'"$container_py"'" \
                --compatibility "manylinux_'"${manylinux_ver}"'" \
                --features blastit/python
        '
else
    mkdir -p "$out_dir"
    rm -f "$out_dir"/blastit-*.whl
    cargo build --release --locked -p blastit-dbproxy --bin blastit-dbproxy
    install -D -m 0755 target/release/blastit-dbproxy python/blastit/_bin/blastit-dbproxy

    # `blastit`'s default features carry `python`, `php` and `static`; naming the
    # capability here is what keeps the wheel and `cargo build -p blastit`
    # identical artifacts (`MB.42.2.2.2` deleted the per-minor extension, so
    # there is no crate feature left to add).
    "${maturin[@]}" build --release --out "$out_dir" \
        --features blastit/python ${extra_args[@]+"${extra_args[@]}"}
fi

# --- stage two: `py3-none` (design §6.1, `MB.42.3.3`) ------------------------

# A release wheel is `py3-none-<platform>`: the repack below relocates the binary
# out of `.data/scripts/` into `blastit/_bin/`, adds `blastit-lsp` and the staged
# shims beside it, writes the three console scripts, sets `Root-Is-Purelib:
# false` and retags the file. The host path (no `--manylinux`) keeps the
# unversioned `linux_<arch>` tag: it is a developer wheel, and the tag- checker
# refuses it as a release artifact unless `--allow-host-tag` says so out loud.
if [[ -z ${wheel_platform:-} ]]; then
    raw_arch=$(uname -m)
    case "$raw_arch" in
        x86_64|amd64) host_arch=x86_64 ;;
        aarch64|arm64) host_arch=aarch64 ;;
        *)
            echo "error: unsupported host architecture '$raw_arch'" >&2
            exit 1
            ;;
    esac
    if [ -e "/lib/ld-musl-${host_arch}.so.1" ]; then host_libc=musl; else host_libc=gnu; fi
    cargo build --release --locked -p blastit-lsp
    install -m 0755 target/release/blastit-lsp "$blob_abs/blastit-lsp"
    stage_shims "$host_arch" "$host_libc"
    # A musl host can compile its own CPython shims (no `--target`: the default
    # triple *is* musl, so `NEEDED` names `libc.musl-<arch>.so.1`). The dlopen
    # probe is skipped: python-build-standalone ships no shared libpython, and
    # `scripts/py_sapi_leg.sh` is where a shim is asserted with the probe.
    if [ "$host_libc" = musl ] && [ "${BLASTIT_WHEEL_BUILD_PY_SHIMS:-}" = 1 ]; then
        make py-sapi PY_MINORS="${PY_MINORS:-3.14 3.13}" PY_SAPI_OUT="$blob_abs" \
            PY_SAPI_ARCH="$host_arch" PY_SAPI_LIBC=musl PY_SAPI_PROBE=0
    fi
fi

# `python/tests/test_release_scripts.py` copies this script **alone** into a tree
# of stubs to exercise the README restore, so a sibling script being absent is
# that fixture and nothing else; say which path ran instead of failing obscurely.
if [ ! -f scripts/repack_universal_wheel.py ]; then
    echo "note: scripts/repack_universal_wheel.py is not in this tree; the maturin wheel stays as it is"
    exit 0
fi

built=$(ls -t "$out_dir"/blastit-*.whl | head -1)

# The repack platform is the **wheel's own** `Tag:` (`MB.42.3.3` review). A host
# build that maturin tagged `musllinux_1_2` — what `scripts/build-musl-wheel.sh`
# asks for with `--compatibility musllinux_1_2` — was repacked as the host's
# `linux_<arch>`, so `build-musl-wheel.sh` refused it and the workflows globbed
# for a name that never existed. The tag is also what a caller's promise is
# checked against: disagreeing is a build that lies about its libc, and it stops
# here instead of at `build-musl-wheel.sh` or `pip install`.
tag_platform=$("${link_python[@]}" - "$built" <<'PY'
import sys
import zipfile

with zipfile.ZipFile(sys.argv[1]) as archive:
    for name in archive.namelist():
        if name.endswith(".dist-info/WHEEL"):
            for line in archive.read(name).decode("utf-8").splitlines():
                if line.startswith("Tag:"):
                    # `<python>-<abi>-<platform...>`; the platform is the rest.
                    print(line.split(":", 1)[1].strip().split("-")[-1])
                    raise SystemExit(0)
raise SystemExit(f"{sys.argv[1]} carries no Tag: line")
PY
) || {
    echo "error: $built carries no readable Tag: line, so the repack cannot name its platform" >&2
    exit 1
}
if [[ -n ${wheel_platform:-} && $wheel_platform != "$tag_platform" ]]; then
    echo "error: this build promised $wheel_platform and $built is tagged $tag_platform" >&2
    echo "  a wheel tagged for one libc and named for the other installs and then dies at execve" >&2
    exit 1
fi
wheel_platform=$tag_platform
if [[ $wheel_platform == manylinux_* ]]; then
    # The floor this tag claims, parsed once (`manylinux_floor`, `MB.42.13`): the
    # tag carries the architecture too, and passing `2_39_x86_64` to the floor
    # parser made the build refuse with a message about the shim, not the parse.
    wheel_floor=$(manylinux_floor "$wheel_platform") || exit 1
    # Before the repack: an embedded ELF that outgrew the floor must never reach
    # a wheel whose tag claims that floor.
    check_blob_floors "$wheel_floor"
fi
read_shims "$shims_from"
assert_shims
repack_log=$("${link_python[@]}" scripts/repack_universal_wheel.py \
    --wheel "$built" \
    --out "$out_dir" \
    --platform "$wheel_platform" \
    --blobs "$blob_abs")
printf '%s\n' "$repack_log"
wheel=$(printf '%s\n' "$repack_log" | sed -n 's/^repack: \(.*\.whl\)$/\1/p' | tail -1)
if [ -z "$wheel" ] || [ ! -f "$wheel" ]; then
    echo "error: the repack produced no wheel (it printed: $repack_log)" >&2
    exit 1
fi
if [ "$built" != "$wheel" ]; then
    rm -f "$built"
fi

# The wheel is useless without the dbproxy binary — a blastit started from it
# refuses to boot — and the payloads the repack promises are named *in* the zip
# (file names are stored uncompressed, so this needs no zip tool).
for required in blastit/_bin/blastit blastit/_bin/blastit-dbproxy blastit/_bin/blastit-lsp \
    blastit/__main__.py .dist-info/entry_points.txt; do
    if ! grep -qa "$required" "$wheel"; then
        echo "error: $wheel carries no $required" >&2
        exit 1
    fi
done
# Every ELF the repacked wheel carries — `blastit`, `blastit-lsp`,
# `blastit-dbproxy` and each shim, all in `blastit/_bin/` — is measured on the
# *file in the wheel* (`MB.17`, `MB.42.3.3`): zero `libpython`/`libphp`
# `DT_NEEDED`, no `RUNPATH` into this machine's uv cache, and (for a `manylinux`
# wheel) nothing above the floor the tag claims. (The call lives inside the `-s`
# guard rather than above it: the release-hygiene fixture in
# `python/tests/test_release_scripts.py` copies this script alone into a tree of
# stubs where a sibling script may be "No such file" — and its stub wheel is not
# a zip, so there is no ELF to measure either way.)
#
# A wheel is a zip by definition, and maturin always writes one. The only
# non-zip `*.whl` in reach is the stub `python/tests/test_release_scripts.py`
# writes to exercise this script's README restore: there is no `_bin/` inside it,
# so the note below is that case and nothing else. It is printed (not silent) so a
# reader of a build log can see which path ran.
"${link_python[@]}" - "$wheel" "$scratch" <<'PY'
import sys
import zipfile
from pathlib import Path

wheel, scratch = Path(sys.argv[1]), Path(sys.argv[2])
if not zipfile.is_zipfile(wheel):
    print(f"note: {wheel} is not a zip archive (a stub build); no extension to inspect")
    sys.exit(0)
with zipfile.ZipFile(wheel) as archive:
    # Everything the repack relocated into `blastit/_bin/`: the binary, the
    # editor binary, dbproxy and each shim. One directory, one loop.
    prefix = "blastit/_bin/"
    members = [
        name
        for name in archive.namelist()
        if name.startswith(prefix) and "/" not in name[len(prefix) :]
    ]
    if not members:
        print(f"error: {wheel} carries nothing under {prefix}")
        sys.exit(1)
    with (scratch / "members").open("w", encoding="utf-8") as members_file:
        for index, name in enumerate(members):
            target = scratch / f"_bin{index}"
            target.write_bytes(archive.read(name))
            target.chmod(0o755)
            members_file.write(f"{name}\t{target}\n")
PY
# `members` is written only by the zip path above, so an empty or absent file
# means the note already said why there is nothing to check.
if [ -s "$scratch/members" ]; then
    # The tag of the *repacked* wheel: `py3-none-<platform>`. A release leg hands
    # the platform it built in; the host path asks for its unversioned
    # `linux_<arch>` tag to be allowed, which is how a developer wheel is
    # distinguishable from a shippable one.
    if [[ $wheel_platform == manylinux_* ]]; then
        "${link_python[@]}" scripts/check_wheel_tag.py \
            --max-glibc "$wheel_floor" \
            --platform "manylinux_${wheel_floor}" \
            "$wheel"
    elif [[ $wheel_platform == musllinux_* ]]; then
        # `${wheel_platform%_*}` strips the arch: `musllinux_1_2_x86_64` becomes
        # `musllinux_1_2`, which is the prefix `--platform` takes.
        "${link_python[@]}" scripts/check_wheel_tag.py \
            --platform "${wheel_platform%_*}" \
            "$wheel"
    else
        "${link_python[@]}" scripts/check_wheel_tag.py --allow-host-tag "$wheel"
    fi
    while IFS=$'\t' read -r member extracted; do
        echo "wheel: $member"
        # `$ORIGIN`-relative search paths are how a musllinux wheel carries its
        # own objects (`blastit.libs/`, maturin's repair); absolute build-host
        # paths are still refused, and the standalone archive is checked with no
        # relaxation at all (`make rs-no-python`, `scripts/build-standalone.sh`).
        scripts/check-portable-elf.sh --allow-origin-rpath "$extracted"
        # The floor, on the same file: `manylinux_…#` is a promise about glibc,
        # and an embedded shim that outgrew it would make the tag a lie.
        if [[ $wheel_platform == manylinux_* ]]; then
            scripts/check-portable-elf.sh --allow-origin-rpath \
                --max-glibc "$wheel_floor" "$extracted"
        fi
        # `MB.3.67`: the seam measurement, on the same file the portable-ELF
        # assertion just read. A release binary that an environment variable can
        # pause before it binds is the denial-of-service surface the archive's
        # binary is checked for in `scripts/build-standalone.sh`.
        scripts/check-no-test-seam.sh "$extracted"
    done < "$scratch/members"
fi
