#!/usr/bin/env bash
# Builds the wheel a project installs with `pip install blastit` / `uv add blastit`.
#
# Two artifacts, in this order:
#
#   1. `blastit-dbproxy`, the Python-free binary that holds the real credentials
#      (§2.2, §4). It is built from this workspace and copied into the wheel's
#      Python package at `blastit/_bin/blastit-dbproxy`, mode 0755, because that
#      is where stage 0 `execve`s it from. It is a plain Rust binary, so it must
#      be built wherever the *target* glibc lives: CI runs this script inside the
#      manylinux_2_28 container, where the wheel has to be built anyway.
#   2. the `blastit._blastit` extension, with `pyo3/extension-module` enabled, so
#      it does not link `libpython` — the interpreter that loads it already has
#      it, and an auditwheel-compliant wheel may not depend on it.
#
# `PYO3_PYTHON` names the interpreter to build against (maturin's
# `--interpreter` does the same); it must be the interpreter the wheel will be
# installed into, because the extension is not abi3.
#
# Development versions. Every commit needs a distinct wheel version, and the
# version comes from Cargo (§ `dynamic = ["version"]`). One command, from the
# repository root:
#
#   sed -i '0,/^version = "0.1.0"$/s//version = "0.1.0.dev'"$N"'"/' Cargo.toml
#   cargo update --workspace
#
# The first line rewrites `[workspace.package] version`; the second brings
# `Cargo.lock`, which records every workspace member's version, back in sync —
# without it, a build with `--locked` fails.
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
