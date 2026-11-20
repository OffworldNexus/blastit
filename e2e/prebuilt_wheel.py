"""The wheel a prebuilt e2e image installs, and the CPython that wheel needs.

`make e2e` (and CI) build the wheel on the *host* and hand `dist/` to the
`*.prebuilt` Dockerfiles, which install it. Two shapes reach here:

* a **release wheel** (`MB.42.3.3`, design §6.1): `blastit-<ver>-py3-none-<platform>.whl`.
  It carries no extension — the universal binary and the shims live in
  `blastit/_bin/` — so it installs into any CPython `>=3.12`. The interpreter the
  image runs is therefore a *choice*, and `E2E_PYTHON_VERSION` makes it (default
  `3.14`, the newest minor a WSGI view is served on);
* a **developer wheel** straight out of `maturin` (`blastit-<ver>-cp3NN-cp3NN-…`).
  `blastit._blastit` is not abi3, so that wheel installs into CPython 3.NN and
  nowhere else, and the tag in the file name is the interpreter the image must
  use.

Exactly one wheel: `dist/` holding two (a release and a development build, or a
stale wheel from another interpreter) is a mistake to report, not to guess at.
"""

import glob
import os
import re

# `blastit-<version>-<python tag>-<abi tag>-<platform...>.whl` (PEP 427); a
# version has no `-`, PEP 440 normalizes it away.
WHEEL = re.compile(
    r"blastit-(?P<version>[^-]+)-(?P<python>py3|cp3(?P<minor>\d+))-(?P<abi>[^-]+)-"
    r"(?P<platform>.+)\.whl"
)
MANYLINUX_GLIBC = re.compile(
    r"^manylinux_(?P<major>\d+)_(?P<minor>\d+)_(?P<arch>x86_64|aarch64)$"
)
# Every prebuilt e2e image is Debian trixie (`glibc 2.41`).
TRIXIE_GLIBC = (2, 41)
#: The CPython a `py3-none` wheel is driven with unless `E2E_PYTHON_VERSION` says
#: otherwise: the newest minor a WSGI view (`[route.wsgi]`) is served on.
DEFAULT_PYTHON_VERSION = "3.14"


def version(dist):
    """The CPython version (`"3.14"`) a prebuilt e2e image installs from `dist`.

    Raises `ValueError` naming what `dist/` holds rather than guessing: none (a
    build that has not run yet) and two both refuse, a tag this cannot read
    (abi3, free-threaded) refuses, and a musllinux or too-new manylinux platform
    tag refuses before Docker runs `pip install`.
    """
    wheels = sorted(glob.glob(os.path.join(dist, "blastit-*.whl")))
    if len(wheels) != 1:
        found = (
            "".join(f"\n  {os.path.basename(wheel)}" for wheel in wheels)
            or "\n  (none)"
        )
        raise ValueError(
            f"a prebuilt e2e image installs exactly one wheel from {dist}, "
            f"found {len(wheels)}:{found}\n"
            "build it with scripts/build-wheel.sh, and clean up the other wheels"
        )
    name = os.path.basename(wheels[0])
    match = WHEEL.fullmatch(name)
    if match is None:
        raise ValueError(
            f"{name} is not a blastit wheel: cannot read the CPython it targets"
        )
    if match["python"] != "py3" and match["abi"] != match["python"]:
        raise ValueError(
            f"{name} targets {match['abi']}, and the e2e images are built for the ordinary "
            f"{match['python']} build: the wheel has to match the interpreter it installs into"
        )
    plat = match["platform"]
    if plat.startswith("musllinux_"):
        raise ValueError(
            f"{name} is a musl wheel ({plat}), and the prebuilt e2e images are Debian trixie (glibc)"
        )
    many = MANYLINUX_GLIBC.fullmatch(plat)
    if many is not None:
        glibc = (int(many.group("major")), int(many.group("minor")))
        if glibc > TRIXIE_GLIBC:
            raise ValueError(
                f"{name} requires glibc {glibc[0]}.{glibc[1]} ({plat}), above the Debian trixie "
                "e2e base image floor (2.41); build with `scripts/build-wheel.sh --manylinux 2_28`"
            )
    if match["python"] == "py3":
        # One artifact, every CPython `>=3.12`: the image's interpreter is a
        # choice, and the environment makes it (`E2E_PYTHON_VERSION`).
        return os.environ.get("E2E_PYTHON_VERSION") or DEFAULT_PYTHON_VERSION
    return f"3.{match['minor']}"
