"""blastit's PEP 517 backend: maturin's, with the dbproxy binary in front of it.

A blastit wheel is useless without `blastit/_bin/blastit-dbproxy` in it: stage 0
`execve`s that Python-free binary for the credentials-holding role (§2.2, §4),
and blastit refuses to start without it. The published wheels get it from
`scripts/build-wheel.sh` (or from the manylinux container's
`before-script-linux`, `.github/workflows/publish-pypi.yml`), which both build
dbproxy *before* they call `maturin build`. A PEP 517 build — `pip install .`,
`uv sync`, and every `pip install blastit` that falls back to the sdist — has no
such step, so this backend adds it: build dbproxy from the workspace right here,
put it where `[tool.maturin] include` looks for it, then hand over to maturin's
own backend and let it do the rest exactly as it does today.

The sdist on PyPI is built from this same file, so a source install on a platform
with no wheel (aarch64, an older glibc) ends up with the same working wheel.
"""
from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
from typing import Any, Mapping

import maturin as _maturin

_BIN = "blastit-dbproxy"
# Where stage 0 looks for it, inside the installed package.
_DEST = pathlib.Path("python/blastit/_bin") / _BIN

# maturin's `pep517` commands warn that `build-backend` is not the literal string
# `maturin`, and that "packaging tools such as pip will not use maturin to build
# this project". They will: this backend is maturin's, called a moment later.
# The warning is about the spelling of one pyproject.toml field, so it is off.
os.environ.setdefault("MATURIN_NO_MISSING_BUILD_BACKEND_WARNING", "1")


def _cargo_env() -> dict[str, str]:
    """The environment `cargo` needs.

    maturin installs a Rust toolchain into a temporary directory when `cargo`
    isn't on PATH (through `puccinialin`), which is what makes a bare
    `pip install blastit` work on a machine with no Rust at all. Its helper is
    private, hence the `getattr`: without it, the error below is what the user
    gets instead.
    """
    if shutil.which("cargo") is not None:
        return dict(os.environ)
    setup = getattr(_maturin, "_get_env", None)
    env = (setup() if setup is not None else None) or dict(os.environ)
    if shutil.which("cargo", path=env.get("PATH", os.defpath)) is None:
        raise RuntimeError(
            "building blastit from source runs `cargo`: install a Rust toolchain "
            "(https://rustup.rs) and build again"
        )
    return env


def _build_dbproxy() -> None:
    """Build dbproxy from the workspace in the current directory, into the
    package, mode 0755 — the same two commands `scripts/build-wheel.sh` runs."""
    subprocess.run(
        ["cargo", "build", "--release", "--locked", "-p", _BIN, "--bin", _BIN],
        check=True,
        env=_cargo_env(),
    )
    _DEST.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(pathlib.Path("target/release") / _BIN, _DEST)
    _DEST.chmod(0o755)


def build_wheel(
    wheel_directory: str,
    config_settings: Mapping[str, Any] | None = None,
    metadata_directory: str | None = None,
) -> str:
    _build_dbproxy()
    return _maturin.build_wheel(wheel_directory, config_settings, metadata_directory)


def build_editable(
    wheel_directory: str,
    config_settings: Mapping[str, Any] | None = None,
    metadata_directory: str | None = None,
) -> str:
    # An editable install points the interpreter straight at `python/`, so the
    # binary has to be at its real path there just the same.
    _build_dbproxy()
    return _maturin.build_editable(wheel_directory, config_settings, metadata_directory)


def __getattr__(name: str) -> Any:
    """Everything else PEP 517 asks a backend for — `build_sdist`,
    `prepare_metadata_for_build_wheel`, `get_requires_for_build_wheel`, … — is
    maturin's own, untouched."""
    return getattr(_maturin, name)
