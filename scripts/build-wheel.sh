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
scratch=$(mktemp -d "${TMPDIR:-/var/tmp}/blastit-wheel.XXXXXX")
restore_readme() {
    cp "$readme" README.md
    rm -f "$readme"
    rm -rf "$scratch"
}
trap restore_readme EXIT
"${link_python[@]}" scripts/readme-absolute-links.py

# `blastit-py/extension-module` is deliberately not a default feature of that
# crate: it must be on for this build alone (see `crates/blastit-py/Cargo.toml`).
# `blastit/python` is the wheel's half of `ADR-0010` §4 (wheels are `python,
# php`): `blastit`'s default features are `php` only, so the wheel — the one
# artifact that *has* an interpreter — names the capability here, and
# `cargo build -p blastit --locked` stays the portable standalone binary.
"${maturin[@]}" build --release --out dist \
    --features blastit-py/extension-module,blastit/python "$@"

# The wheel is useless without the dbproxy binary — a blastit started from it
# refuses to boot — and maturin only puts it in because `pyproject.toml` names it
# in `include`. Fail here rather than at the pilot's first request: the file
# names are stored uncompressed in a wheel, so this needs no zip tool.
wheel=$(ls -t dist/blastit-*.whl | head -1)
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
    while IFS=$'\t' read -r member extracted; do
        echo "wheel: $member"
        scripts/check-portable-elf.sh "$extracted"
    done < "$scratch/members"
fi
