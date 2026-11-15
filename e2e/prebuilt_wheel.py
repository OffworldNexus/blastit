"""The wheel a prebuilt e2e image installs, and the CPython that wheel needs.

`make e2e` (and CI) build the wheel on the *host* and hand `dist/` to the
`*.prebuilt` Dockerfiles, which install it. `blastit._blastit` is not abi3, so
the image's interpreter has to be the one the wheel was built for: a `cp314`
wheel installs into CPython 3.14 and nowhere else. The tag in the wheel's file
name is therefore what the drivers pass as `PYTHON_VERSION`, and it becomes the
base image — 3.13 on a runner that pinned 3.13, 3.14 on a developer's machine —
instead of a version hardcoded in the Dockerfiles that only works where the
host's interpreter happens to match it.

Exactly one wheel: `dist/` holding two (a release and a development build, or a
stale wheel from another interpreter) is a mistake to report, not to guess at.
"""

import glob
import os
import re

# `blastit-<version>-<python tag>-<abi tag>-<platform...>.whl` (PEP 427); a
# version has no `-`, PEP 440 normalizes it away.
WHEEL = re.compile(
    r"blastit-(?P<version>[^-]+)-(?P<python>cp3(?P<minor>\d+))-(?P<abi>[^-]+)-(?P<platform>.+)\.whl"
)
MANYLINUX_GLIBC = re.compile(
    r"^manylinux_(?P<major>\d+)_(?P<minor>\d+)_(?P<arch>x86_64|aarch64)$"
)
# Every prebuilt e2e image is Debian trixie (`glibc 2.41`).
TRIXIE_GLIBC = (2, 41)


def version(dist):
    """The CPython version (`"3.14"`) of the single blastit wheel in `dist`.

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
    if match["abi"] != match["python"]:
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
    return f"3.{match['minor']}"
