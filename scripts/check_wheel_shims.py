#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Prove a built wheel carries the shims it promised (`MB.45`).

    uv run --no-project scripts/check_wheel_shims.py [--require {py,all}] \\
        [--libc {gnu,musl}] dist/*.whl

`scripts/build-wheel.sh` fails closed on the *staging* directory before the
repack, and `scripts/repack_universal_wheel.py` is what moves the payload into
`blastit/_bin/`. Neither of them reads the finished zip, so a repack that dropped
a shim (or a wheel leg that staged the wrong tuple) shipped a wheel `uv add
blastit` installs and then cannot load PHP from — the defect `MB.45` exists for.
This reads each wheel with `zipfile` and asserts the payload is in the archive:

* the two CPython shims, `blastit/_bin/libblastit_py313-*` and `py314-*`, always
  (a `py3-none` wheel that cannot `dlopen` its CPython is not a wheel);
* with `--require all`, the four PHP SAPI shims `php83`–`php86` as well
  (`ADR-0017` decision 4);
* with `--libc`, the shim must be this wheel's own `(arch, libc)` tuple — the
  arch is read from the wheel's `py3-none-<platform>_<arch>.whl` filename, because
  another target's shim is a foreign ABI, not a spare. A filename that names none
  of the arches this release ships is refused rather than guessed at.

A missing shim fails with the wheel, the shim and the fix for its kind: a `py*`
shim is compiled by the wheel builder, a `php*` one can only be staged with
`--shims-from` (the `shim-*` artifacts of the `shims` job). `publish-pypi.yml`'s
`verify` job runs it with `--require all` over both libcs before anything reaches
PyPI, and `.github/actions/build-wheel` runs it over the wheel it just built.
"""

from __future__ import annotations

import argparse
import sys
import zipfile
from pathlib import Path, PurePosixPath

# The canonical lists are `scripts/platform_matrix.py`'s: `ARCHES` (the arches an
# artifact exists for) and `PY_MINORS`/`PHP_MINORS` (one shim per minor). Spelling
# them here a third time is the copy that goes stale the day PHP 8.7 or CPython
# 3.15 lands, while the matrix file is import-safe (`main` is under `__main__`).
# The scripts directory goes on the path for a caller that imports this module.
sys.path.insert(0, str(Path(__file__).resolve().parent))

from platform_matrix import ARCHES, PHP_MINORS, PY_MINORS  # noqa: E402

#: The `<tag>` half of `libblastit_<tag>-<arch>-<libc>.so`, from the matrix.
ALWAYS = tuple(f"py{digits}" for digits in PY_MINORS.values())
PHP = tuple(f"php{digits}" for digits in PHP_MINORS.values())
SHIM_ARCHES = tuple(ARCHES)
PAYLOAD = "blastit/_bin/"
PHP_REMEDY = (
    "stage it with `--shims-from <dir>` (the `shim-*` artifacts of the `shims` "
    "job) before the repack"
)
PY_REMEDY = (
    "the wheel builder compiles the CPython shims itself (`make py-sapi`, or the "
    "builder image's own `make py-sapi`); the `shim-*` artifacts never carry one"
)


def remedy(tag: str) -> str:
    """Where a missing `tag` shim can actually come from."""
    return PY_REMEDY if tag.startswith("py") else PHP_REMEDY


def wheel_arch(filename: str) -> str:
    """The `<arch>` of a `blastit-<version>-py3-none-<platform>_<arch>.whl` name."""
    stem = filename[:-4] if filename.endswith(".whl") else filename
    for arch in SHIM_ARCHES:
        if stem.endswith(f"_{arch}"):
            return arch
    return ""


def payload_names(wheel: Path) -> list[str]:
    """Every `blastit/_bin/` entry in the wheel, in archive order."""
    with zipfile.ZipFile(wheel) as archive:
        return [name for name in archive.namelist() if name.startswith(PAYLOAD)]


def shims_for(names: list[str], tag: str) -> list[str]:
    """`blastit/_bin/libblastit_<tag>-*.so` entries, sorted."""
    prefix = f"{PAYLOAD}libblastit_{tag}-"
    return sorted(
        name for name in names if name.startswith(prefix) and name.endswith(".so")
    )


def tuple_suffix(arch: str, libc: str) -> str:
    """The `-<arch>-<libc>.so` suffix a shim of this wheel's tuple carries."""
    if not libc:
        return ""
    return f"-{arch}-{libc}.so"


def unknown_arch_problem(wheel: Path, libc: str) -> str:
    """`--libc` pins the tuple, so a filename with no known arch cannot satisfy it."""
    return (
        f"{wheel.name}: `--libc {libc}` pins the shim tuple, but this filename names "
        f"none of the arches this release ships ({', '.join(SHIM_ARCHES)}) — refusing "
        "rather than accepting any arch's shim"
    )


def inspect_wheel(
    wheel: Path, *, require: str, libc: str
) -> tuple[list[str], list[str]]:
    """The shims this wheel carries and every shim it promised but does not."""
    if not wheel.is_file():
        return [], [f"{wheel}: wheel file does not exist"]
    if not zipfile.is_zipfile(wheel):
        return [], [f"{wheel.name}: not a valid zip wheel archive"]
    arch = wheel_arch(wheel.name)
    if libc and not arch:
        return [], [unknown_arch_problem(wheel, libc)]
    try:
        names = payload_names(wheel)
    except (OSError, zipfile.BadZipFile) as error:
        return [], [f"{wheel.name}: cannot read the archive: {error}"]

    suffix = tuple_suffix(arch, libc)
    wanted = ALWAYS + (PHP if require == "all" else ())
    found: list[str] = []
    problems: list[str] = []
    for tag in wanted:
        candidates = shims_for(names, tag)
        matched = [name for name in candidates if name.endswith(suffix)]
        if matched:
            found.extend(matched)
            continue
        if candidates:
            carried = ", ".join(PurePosixPath(name).name for name in candidates)
            problems.append(
                f"{wheel.name}: blastit/_bin/libblastit_{tag}-* carries no "
                f"`{suffix}` entry for this wheel's tuple (found {carried}); "
                f"{remedy(tag)}"
            )
        else:
            problems.append(
                f"{wheel.name}: no blastit/_bin/libblastit_{tag}-* shim in the wheel "
                f"(require-shims {require}); {remedy(tag)}"
            )
    return found, problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--require",
        choices=("py", "all"),
        default="py",
        help=(
            "the promise `scripts/build-wheel.sh` was given: `py` carries the two "
            "CPython shims, `all` the four PHP shims as well"
        ),
    )
    parser.add_argument(
        "--libc",
        choices=("gnu", "musl"),
        default="",
        help=(
            "require the shims of this wheel's own `(arch, libc)` tuple (the arch "
            "comes from the wheel filename); empty accepts any"
        ),
    )
    parser.add_argument("wheels", nargs="+", help="wheel file(s) to check")
    args = parser.parse_args(argv)

    all_problems: list[str] = []
    for raw_path in args.wheels:
        wheel = Path(raw_path)
        found, problems = inspect_wheel(wheel, require=args.require, libc=args.libc)
        if found:
            carried = ", ".join(PurePosixPath(name).name for name in found)
            print(f"wheel-shims: {wheel.name}: {carried}")
        all_problems.extend(problems)

    if all_problems:
        for problem in all_problems:
            print(f"wheel-shims: FAIL {problem}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
