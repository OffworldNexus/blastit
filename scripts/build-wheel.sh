#!/usr/bin/env bash
# Builds the wheel a project installs with `pip install blastit` / `uv add blastit`.
#
# Two artifacts, in this order:
#
#   1. `blastit-dbproxy`, the Python-free binary that holds the real credentials
#      (§2.2, §4). It is built from this workspace and copied into the wheel's
#      Python package at `blastit/_bin/blastit-dbproxy`, mode 0755, because that
#      is where stage 0 `execve`s it from. It is a plain Rust binary linked
#      against the system glibc, so build it wherever the wheel's *target* glibc
#      lives: a runner (ci.yml) is fine for a wheel that stays on that runner's
#      distribution, and `release.yml` builds the published manylinux wheels
#      inside the manylinux_2_28 container instead of calling this script.
#   2. the `blastit._blastit` extension, with `pyo3/extension-module` enabled, so
#      it does not link `libpython` — the interpreter that loads it already has
#      it, and an auditwheel-compliant wheel may not depend on it.
#
# `PYO3_PYTHON` names the interpreter to build against (maturin's
# `--interpreter` does the same); it must be the interpreter the wheel will be
# installed into, because the extension is not abi3.
#
# The wheels CI builds for a commit to `develop` carry a version of their own,
# from Cargo: `scripts/dev-version.sh <build number>` moves the workspace version
# and everything that depends on it, so a wheel downloaded from a run can't be
# mistaken for the release at the same commit. This script is happy to run after
# it.
#
# Usage: scripts/build-wheel.sh [extra arguments for `maturin build`]
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

# `MATURIN` overrides how maturin is found: `uvx maturin` by default (nothing to
# install), `maturin` in a build image that already has it on PATH.
maturin=(uvx maturin)
if [[ -n ${MATURIN:-} ]]; then
    read -r -a maturin <<<"$MATURIN"
fi

cargo build --release --locked -p blastit-dbproxy --bin blastit-dbproxy
install -D -m 0755 target/release/blastit-dbproxy python/blastit/_bin/blastit-dbproxy

# `blastit-py/extension-module` is deliberately not a default feature of that
# crate: it must be on for this build alone (see `crates/blastit-py/Cargo.toml`).
"${maturin[@]}" build --release --out dist --features blastit-py/extension-module "$@"

# The wheel is useless without the dbproxy binary — a blastit started from it
# refuses to boot — and maturin only puts it in because `pyproject.toml` names it
# in `include`. Fail here rather than at the pilot's first request: the file
# names are stored uncompressed in a wheel, so this needs no zip tool.
wheel=$(ls -t dist/blastit-*.whl | head -1)
if ! grep -qa "blastit/_bin/blastit-dbproxy" "$wheel"; then
    echo "the wheel has no dbproxy binary in it: $wheel" >&2
    exit 1
fi
