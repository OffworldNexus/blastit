#!/usr/bin/env bash
# Builds the wheel a project installs with `pip install blastit` / `uv add blastit`.
#
# Two artifacts, in this order:
#
#   1. `blastit-dbproxy`, the Python-free binary that holds the real credentials
#      (§2.2, §4). It is built from this workspace and copied into the wheel's
#      Python package at `blastit/_bin/blastit-dbproxy`, mode 0755, because that
#      is where stage 0 `execve`s it from.
#   2. the `blastit._blastit` extension, with `pyo3/extension-module` enabled, so
#      it does not link `libpython` — the interpreter that loads it already has
#      it, and an auditwheel-compliant wheel may not depend on it.
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
#     `*.dist-info/WHEEL`, prints the actual `Tag:`, refuses unversioned `linux_*`,
#     and enforces `--max-glibc ${BLASTIT_WHEEL_MAX_GLIBC:-2_41}`.
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
while [ "$#" -gt 0 ]; do
    case "$1" in
        --manylinux)
            manylinux_ver=${2:?--manylinux requires a version such as 2_28}
            shift 2
            ;;
        --manylinux=*)
            manylinux_ver=${1#--manylinux=}
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
        -e RUSTUP_HOME=/host-rustup \
        -e CARGO_HOME=/tmp/cargo-home \
        -e CARGO_TARGET_DIR="/src/target/manylinux_${manylinux_ver}" \
        "${offline_env[@]}" \
        -v "$root:/src" \
        -v "$out_abs:/wheel-out" \
        -v "$rustup_home:/host-rustup:ro" \
        -v "$cargo_home:/host-cargo:ro" \
        -v "$maturin_bin:/usr/local/bin/maturin:ro" \
        -w /src \
        "$builder_image" \
        bash -euo pipefail -c '
            mkdir -p /tmp/cargo-home
            ln -s /host-cargo/registry /tmp/cargo-home/registry 2>/dev/null || true
            ln -s /host-cargo/git /tmp/cargo-home/git 2>/dev/null || true
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
            maturin build --release --out /wheel-out \
                --interpreter "'"$container_py"'" \
                --compatibility "manylinux_'"${manylinux_ver}"'" \
                --features blastit-py/extension-module,blastit/python
        '
else
    mkdir -p "$out_dir"
    rm -f "$out_dir"/blastit-*.whl
    cargo build --release --locked -p blastit-dbproxy --bin blastit-dbproxy
    install -D -m 0755 target/release/blastit-dbproxy python/blastit/_bin/blastit-dbproxy

    # `blastit-py/extension-module` is deliberately not a default feature of that
    # crate: it must be on for this build alone (see `crates/blastit-py/Cargo.toml`).
    # `blastit/python` is the wheel's half of `ADR-0010` §4 (wheels are `python,
    # php`): `blastit`'s default features are `php` only, so the wheel — the one
    # artifact that *has* an interpreter — names the capability here, and
    # `cargo build -p blastit --locked` stays the portable standalone binary.
    "${maturin[@]}" build --release --out "$out_dir" \
        --features blastit-py/extension-module,blastit/python ${extra_args[@]+"${extra_args[@]}"}
fi

# The wheel is useless without the dbproxy binary — a blastit started from it
# refuses to boot — and maturin only puts it in because `pyproject.toml` names it
# in `include`. Fail here rather than at the pilot's first request: the file
# names are stored uncompressed in a wheel, so this needs no zip tool.
wheel=$(ls -t "$out_dir"/blastit-*.whl | head -1)
if ! grep -qa "blastit/_bin/blastit-dbproxy" "$wheel"; then
    echo "the wheel has no dbproxy binary in it: $wheel" >&2
    exit 1
fi

# `pyo3/extension-module` is what keeps the extension off `libpython`, but the
# artifact is only portable if that is true of the *file in the wheel*, not of the
# intent in `Cargo.toml` (`MB.17`). Extract every `blastit/_blastit*.so` and run
# the same `readelf` assertions the standalone binary gets: no `libpython`, no
# `RUNPATH`/`RPATH` into this machine's uv cache.
#
# A wheel is a zip by definition, and maturin always writes one. The only
# non-zip `*.whl` in reach is the stub maturin `python/tests/test_release_scripts.py`
# writes to exercise this script's README restore: there is no extension inside
# it to inspect, so the note below is that case and nothing else. It is printed
# (not silent) so a reader of a build log can see which path ran.
"${link_python[@]}" - "$wheel" "$scratch" <<'PY'
import sys
import zipfile
from pathlib import Path

wheel, scratch = Path(sys.argv[1]), Path(sys.argv[2])
if not zipfile.is_zipfile(wheel):
    print(f"note: {wheel} is not a zip archive (a stub build); no extension to inspect")
    sys.exit(0)
with zipfile.ZipFile(wheel) as archive:
    members = [
        name
        for name in archive.namelist()
        if name.startswith("blastit/") and "_blastit" in name and name.endswith(".so")
    ]
    if not members:
        sys.exit(f"{wheel} carries no blastit/_blastit*.so: there is no extension to check")
    with (scratch / "members").open("w", encoding="utf-8") as members_file:
        for index, name in enumerate(members):
            target = scratch / f"_blastit{index}.so"
            target.write_bytes(archive.read(name))
            members_file.write(f"{name}\t{target}\n")
PY
# `members` is written only by the zip path above, so an empty or absent file
# means the note already said why there is nothing to check.
if [ -s "$scratch/members" ]; then
    if [[ -n $manylinux_ver ]]; then
        "${link_python[@]}" scripts/check_wheel_tag.py \
            --max-glibc "$manylinux_ver" \
            --platform "manylinux_${manylinux_ver}" \
            "$wheel"
    else
        "${link_python[@]}" scripts/check_wheel_tag.py \
            --max-glibc "${BLASTIT_WHEEL_MAX_GLIBC:-2_41}" \
            "$wheel"
    fi
    while IFS=$'\t' read -r member extracted; do
        echo "wheel: $member"
        # `$ORIGIN`-relative search paths are how a musllinux wheel carries its
        # own objects (`blastit.libs/`, maturin's repair); absolute build-host
        # paths are still refused, and the standalone archive is checked with no
        # relaxation at all (`make rs-no-python`, `scripts/build-standalone.sh`).
        scripts/check-portable-elf.sh --allow-origin-rpath "$extracted"
    done < "$scratch/members"
fi
