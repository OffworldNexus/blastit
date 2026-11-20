#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Retag a `maturin` bin wheel into the universal `py3-none-<platform>` layout.

Design §6.1, stage two of `scripts/build-wheel.sh`:

    maturin build --bindings bin  ->  blastit-<ver>-cp3NN-cp3NN-<platform>.whl
    repack_universal_wheel.py     ->  blastit-<ver>-py3-none-<platform>.whl

One wheel serves CPython 3.12, 3.13 and 3.14, because it carries no extension:
it carries the universal `blastit` binary, `blastit-lsp`, `blastit-dbproxy` and
every shim the build staged, all under `blastit/_bin/` (mode 0755), where
`blastit._blastit` looks first. What this script does, and why each step is
required:

1. **Relocates the binary.** A `bindings = "bin"` wheel puts `blastit` in
   `<name>-<ver>.data/scripts/`, and `pip` installs it into the interpreter's
   `bin/`. That is a *per-interpreter* location for an artifact that is not
   per-interpreter, and it collides with the console script of the same name.
   The binary moves to `blastit/_bin/blastit` and the console script takes the
   `bin/` slot.

2. **Adds the console scripts.** `[project.scripts]` cannot be declared in
   `pyproject.toml` beside `bindings = "bin"` — maturin refuses the pair — so
   the repack writes the `*.dist-info/entry_points.txt` that `pip` turns into
   `blastit`, `blastit-lsp` and `blastit-manifest` on `PATH`. `CONSOLE_SCRIPTS`
   is the single owner of those three names.

3. **Retags the file.** `Root-Is-Purelib: false` (the wheel carries ELFs — `pip`
   must not install it into the purelib tree unconditionally) and
   `Tag: py3-none-<platform>`, which is what makes one artifact valid for three
   CPython minors. The filename is renamed to match: a wheel whose filename and
   `WHEEL` disagree is refused by `scripts/check_wheel_tag.py` and by `pip`.

4. **Recomputes `RECORD`.** Every file a wheel carries is listed with its
   SHA-256 and size; `pip` verifies them on install, so a repacked wheel with a
   stale `RECORD` is a wheel that cannot be installed at all.

Usage:

    scripts/repack_universal_wheel.py --wheel dist/blastit-*.whl \\
        --platform manylinux_2_28_x86_64 --blobs target/wheel-blobs

`--blobs DIR` is where the build staged `blastit-lsp` and the shims
(`libblastit_py3*.so`, `libblastit_php8*.so`); every file in it is copied into
`blastit/_bin/` with mode 0755. `--arch`/`--libc` filter the shims to the wheel's
own `(arch, libc)`, because a wheel that carried another target's `libphp` shim
would `dlopen` a foreign ABI.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import re
import sys
import zipfile
from pathlib import Path

#: The console scripts the wheel installs (`design §6.1`): `name = "module:callable"`.
#:
#: `blastit` is the package's `main` (which `exec`s `blastit/_bin/blastit`);
#: `blastit-lsp` reaches `blastit.__main__:main_lsp`, the entry point that had no
#: console script until `MB.42.3`; `blastit-manifest` is `fold`'s command line
#: (`blastit-manifest fold LOG --manifest Blastfile`).
CONSOLE_SCRIPTS: dict[str, str] = {
    "blastit": "blastit.__main__:main",
    "blastit-lsp": "blastit.__main__:main_lsp",
    "blastit-manifest": "blastit.fold:main",
}

#: Where a `bindings = "bin"` wheel keeps the binary (`maturin`'s data scripts).
DATA_SCRIPTS = re.compile(r"^[^/]+\.data/scripts/(?P<name>[^/]+)$")
#: A **repaired** wheel (`auditwheel` inside maturin, what a musl build produces)
#: keeps the real program in `<name>.scripts/`, leaves a tiny `#!python` launcher
#: in `.data/scripts/`, and puts the shared objects it links in `<name>.libs/`,
#: reached by an `$ORIGIN/../<name>.libs` RPATH. All three have to be moved
#: together, or the relocated binary cannot load its own `libgcc_s`.
SCRIPTS_DIR = re.compile(r"^[^/]+\.scripts/(?P<name>[^/]+)$")
LIBS_DIR = re.compile(r"^(?P<prefix>[^/]+)\.libs/(?P<rest>.+)$")
#: `blastit-<version>-cp3NN-cp3NN-<platform>.whl` (PEP 427; a version has no `-`).
WHEEL_NAME = re.compile(
    r"^(?P<name>[^-]+)-(?P<version>[^-]+)-(?P<python>[^-]+)-(?P<abi>[^-]+)-"
    r"(?P<platform>.+)\.whl$"
)
#: A shim a wheel may carry, and the `(arch, libc)` it was built for.
SHIM = re.compile(
    r"^libblastit_(?P<kind>py3\d\d|php8\d)-(?P<arch>[^-]+)-(?P<libc>[^.]+)\.so$"
)
PLATFORM = re.compile(r"^(?P<family>manylinux|musllinux|linux)_(?P<rest>.+)$")
#: The arches a blastit artifact exists for (`scripts/platform_matrix.py`).
KNOWN_ARCHES = ("x86_64", "aarch64")
MANYLINUX_REST = re.compile(r"^(?P<major>\d+)_(?P<minor>\d+)_(?P<arch>x86_64|aarch64)$")


def record_hash(data: bytes) -> str:
    """The `RECORD` digest `pip` verifies: `sha256=` + urlsafe-b64, no padding."""
    digest = hashlib.sha256(data).digest()
    return "sha256=" + base64.urlsafe_b64encode(digest).decode("ascii").rstrip("=")


def entry_points_text(scripts: dict[str, str] | None = None) -> str:
    """The `*.dist-info/entry_points.txt` body for the console scripts."""
    lines = ["[console_scripts]"]
    for name, target in (scripts or CONSOLE_SCRIPTS).items():
        lines.append(f"{name} = {target}")
    return "\n".join(lines) + "\n"


def wheel_metadata(text: str, platform: str) -> str:
    """`WHEEL` with `Root-Is-Purelib: false` and one `Tag: py3-none-<platform>`.

    The other fields (`Wheel-Version`, `Generator`) are kept, every `Tag:` line
    is replaced, and a `Root-Is-Purelib` line is rewritten in place or added —
    `pip` reads the *last* value of a duplicated header as authoritative, but a
    wheel with two contradicting lines is a wheel two tools disagree about, so
    the old one is dropped rather than shadowed.
    """
    kept: list[str] = []
    for line in text.splitlines():
        if line.startswith("Tag:"):
            continue
        if line.startswith("Root-Is-Purelib:"):
            continue
        if not line.strip():
            continue
        kept.append(line)
    if not any(line.startswith("Wheel-Version:") for line in kept):
        kept.insert(0, "Wheel-Version: 1.0")
    kept.append("Root-Is-Purelib: false")
    kept.append(f"Tag: py3-none-{platform}")
    return "\n".join(kept) + "\n"


def platform_arch(platform: str) -> tuple[str, str]:
    """`(family, arch)` for a wheel platform tag, or `("", "")` when unreadable.

    Only the two arches the matrix builds are recognised (`MB.42.3.3` review): a
    guessed arch would filter shims for the wrong target and ship a wheel whose
    `_bin/` holds another machine's libraries, so an unknown platform is a
    refusal, never a default.
    """
    match = PLATFORM.fullmatch(platform)
    if match is None:
        return "", ""
    family, rest = match["family"], match["rest"]
    if family == "linux":
        return (family, rest) if rest in KNOWN_ARCHES else (family, "")
    inner = MANYLINUX_REST.fullmatch(rest)
    return (family, inner["arch"]) if inner is not None else (family, "")


def staged_blobs(directory: Path, *, arch: str, libc: str) -> dict[str, bytes]:
    """The files a build staged for `blastit/_bin/`, keyed by their basename.

    `blastit-lsp` and the shims belong to one `(arch, libc)`. A `.so` built for
    another tuple is **refused**, never skipped: silently dropping it produces a
    wheel that ships fewer shims than the leg promised while every count still
    says six, and the review of `MB.42.3.3` found exactly that hole. A name that
    does not parse as a shim is copied as is (`blastit-lsp`, a `blastit-dbproxy`
    a caller staged by hand).
    """
    blobs: dict[str, bytes] = {}
    if not directory.is_dir():
        return blobs
    for path in sorted(directory.iterdir()):
        if not path.is_file():
            continue
        shim = SHIM.fullmatch(path.name)
        if shim is not None and (shim["arch"], shim["libc"]) != (arch, libc):
            raise ValueError(
                f"{path.name} is built for {shim['arch']}-{shim['libc']}, and this "
                f"wheel is {arch}-{libc}: stage only this target's shims (a foreign "
                "shim would `dlopen` a different ABI at stage 1)"
            )
        blobs[path.name] = path.read_bytes()
    return blobs


def repack(
    wheel: Path,
    out_dir: Path,
    *,
    platform: str,
    blobs: Path | None = None,
    scripts: dict[str, str] | None = None,
) -> Path:
    """Write the universal wheel and return its path. Raises `ValueError`."""
    if not wheel.is_file():
        raise ValueError(f"{wheel}: no such wheel")
    if not platform or "/" in platform:
        raise ValueError(f"--platform {platform!r} is not a wheel platform tag")
    match = WHEEL_NAME.fullmatch(wheel.name)
    if match is None:
        raise ValueError(f"{wheel.name}: not a PEP 427 wheel file name")
    family, arch = platform_arch(platform)
    # A `linux_<arch>` wheel is a host build on a glibc machine (design §6.1's
    # developer artifact); `musllinux` is musl, and `manylinux` is glibc.
    libc = "musl" if family == "musllinux" else "gnu"
    if not arch:
        raise ValueError(
            f"--platform {platform!r} names no architecture this wheel can be "
            "built for (expected ..._x86_64 or ..._aarch64)"
        )

    with zipfile.ZipFile(wheel) as archive:
        members = {name: archive.read(name) for name in archive.namelist()}
    dist_infos = [name.split("/")[0] for name in members if ".dist-info/WHEEL" in name]
    if len(dist_infos) != 1:
        raise ValueError(
            f"{wheel.name}: expected exactly one *.dist-info/WHEEL entry, "
            f"found {len(dist_infos)} ({dist_infos})"
        )
    dist_info = dist_infos[0]

    # A repaired wheel's real binary, before anything is moved (see `SCRIPTS_DIR`).
    # A loop, not a walrus in a comprehension: that binds in the enclosing scope
    # and would overwrite the file-name `match` this function relies on below.
    real_binaries: dict[str, bytes] = {}
    for member_name, member_data in members.items():
        staged = SCRIPTS_DIR.match(member_name)
        if staged is not None:
            real_binaries[staged["name"]] = member_data

    # 1. The binary out of `.data/scripts/` (or `<name>.scripts/`) and into the
    # package, with the shared objects its RPATH names.
    relocated: dict[str, bytes] = {}
    for name, data in members.items():
        script = DATA_SCRIPTS.match(name)
        if script is not None:
            program = script["name"]
            # `maturin`'s repaired wheels put a `#!python` launcher here and the
            # program itself in `<name>.scripts/`; take the program.
            payload = real_binaries.pop(program, data)
            relocated[f"blastit/_bin/{program}"] = payload
            continue
        libs = LIBS_DIR.match(name)
        if libs is not None:
            # `blastit/_bin/blastit`'s RPATH is `$ORIGIN/../<name>.libs`, so the
            # directory moves *inside* the package, keeping that relative path.
            relocated[f"blastit/{libs['prefix']}.libs/{libs['rest']}"] = data
            continue
        relocated[name] = data
    # A `<name>.scripts/` member no data script named is kept under `_bin/`, so a
    # wheel that starts shipping a second program does not lose it silently.
    for program, data in real_binaries.items():
        relocated[f"blastit/_bin/{program}"] = data
    if "blastit/_bin/blastit" not in relocated:
        raise ValueError(
            f"{wheel.name}: carries no <name>.data/scripts/blastit: it was not built "
            "with `maturin build --bindings bin`"
        )

    # 2. `blastit-lsp`, the shims and anything else the build staged.
    if blobs is not None:
        for name, data in staged_blobs(blobs, arch=arch, libc=libc).items():
            relocated[f"blastit/_bin/{name}"] = data

    # 3/4. Metadata, entry points, RECORD.
    relocated[f"{dist_info}/WHEEL"] = wheel_metadata(
        relocated[f"{dist_info}/WHEEL"].decode("utf-8"), platform
    ).encode()
    relocated[f"{dist_info}/entry_points.txt"] = entry_points_text(scripts).encode()
    record = f"{dist_info}/RECORD"
    relocated[record] = b""
    rows = []
    for name in sorted(relocated):
        data = relocated[name]
        if name == record:
            rows.append(f"{record},,")
        else:
            rows.append(f"{name},{record_hash(data)},{len(data)}")
    relocated[record] = ("\n".join(rows) + "\n").encode()

    out_dir.mkdir(parents=True, exist_ok=True)
    target = out_dir / f"{match['name']}-{match['version']}-py3-none-{platform}.whl"
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
        for name in sorted(relocated):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            # `0100755` for what the layout executes, `0100644` otherwise: the
            # high 16 bits of `external_attr` are a `stat` mode, so a bare `0o755`
            # renders as `?rwxr-xr-x` and `unzip -l`/`zipinfo` read no file type.
            executable = name.startswith("blastit/_bin/") and not name.endswith(".py")
            info.external_attr = (0o100755 if executable else 0o100644) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, relocated[name])
    return target


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--wheel", required=True, help="the `bindings = bin` wheel")
    parser.add_argument("--out", default="dist", help="output directory (default dist)")
    parser.add_argument(
        "--platform",
        required=True,
        help="the wheel's platform tag: manylinux_2_28_x86_64, musllinux_1_2_x86_64",
    )
    parser.add_argument(
        "--blobs",
        default="",
        help="directory holding `blastit-lsp` and the shims to place in blastit/_bin/",
    )
    args = parser.parse_args(argv)

    try:
        target = repack(
            Path(args.wheel),
            Path(args.out),
            platform=args.platform,
            blobs=Path(args.blobs) if args.blobs else None,
        )
    except ValueError as err:
        print(f"repack: error: {err}", file=sys.stderr)
        return 1
    with zipfile.ZipFile(target) as archive:
        carried = sorted(
            name.split("/", 2)[-1]
            for name in archive.namelist()
            if name.startswith("blastit/_bin/")
        )
    print(f"repack: {target}")
    print(f"repack: blastit/_bin/ carries {', '.join(carried)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
