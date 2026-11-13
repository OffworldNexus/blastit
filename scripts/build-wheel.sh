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
#      distribution; CI builds the published wheels inside the manylinux_2_28
#      container instead of calling this script
#      (`.github/workflows/wheels.yml` for a commit to `develop`,
#      `.github/workflows/publish-pypi.yml` for a release).
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
#
# The README is rewritten to absolute links for the duration of the build and
# restored afterwards (see the `trap` below), so the tree is unchanged when this
# exits, successfully or not.
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

# The README is the wheel's long description, and PyPI renders it with no
# repository around it: a GitHub-relative link is a 404 there and a relative
# image does not render at all. A published wheel gets its rewrite from
# `.github/workflows/publish-pypi.yml`; this script does it here so a wheel built
# on a developer's machine is the same artifact. The rewrite is in place, so the
# README is put back on *every* exit path — `make check` runs this from
# `docs-test` and must not end on a dirty tree, not even when the build fails.
#
# `readme-absolute-links.py` is standard library only. `uv` is how this repo runs
# Python, but the CI jobs that build an e2e wheel install maturin with pipx and
# name `PYO3_PYTHON` instead, so that interpreter is the fallback; failing here
# beats a wheel whose description links 404 on PyPI.
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
restore_readme() {
    cp "$readme" README.md
    rm -f "$readme"
}
trap restore_readme EXIT
"${link_python[@]}" scripts/readme-absolute-links.py

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
