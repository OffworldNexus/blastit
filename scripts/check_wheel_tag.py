#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Verify built wheel `*.dist-info/WHEEL` `Tag:` metadata against our platform floor.

Reads `*.dist-info/WHEEL` inside each `.whl` archive, prints the actual `Tag:`
values, refuses bare `linux_*` tags and filename/metadata mismatches, and fails
loudly when a `manylinux_<major>_<minor>` tag exceeds `--max-glibc` (for
example `2_28` on shipped/e2e wheels, catching build-host leaks such as
`manylinux_2_39`) or disagrees with `--platform`.
"""

from __future__ import annotations

import argparse
import re
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path

LEGACY_MANYLINUX: dict[str, tuple[int, int]] = {
    "manylinux1": (2, 5),
    "manylinux2010": (2, 12),
    "manylinux2014": (2, 17),
}
MANYLINUX_RE = re.compile(
    r"^manylinux_(?P<major>\d+)_(?P<minor>\d+)_(?P<arch>x86_64|aarch64)$"
)
LEGACY_RE = re.compile(
    r"^(?P<legacy>manylinux(?:1|2010|2014))_(?P<arch>x86_64|aarch64)$"
)
MUSLLINUX_RE = re.compile(
    r"^musllinux_(?P<major>\d+)_(?P<minor>\d+)_(?P<arch>x86_64|aarch64)$"
)
GLIBC_SPEC_RE = re.compile(r"^(?:manylinux_)?(?P<major>\d+)[._](?P<minor>\d+)$")


@dataclass(frozen=True, slots=True)
class WheelTag:
    raw: str
    python: str
    abi: str
    platform: str


def parse_glibc_bound(raw: str) -> tuple[int, int]:
    match = GLIBC_SPEC_RE.fullmatch(raw.strip())
    if match is None:
        msg = f"invalid --max-glibc {raw!r}: expected <major>_<minor>, e.g. 2_28"
        raise ValueError(msg)
    return int(match.group("major")), int(match.group("minor"))


def parse_tag(raw: str) -> WheelTag:
    parts = raw.strip().split("-")
    if len(parts) != 3 or not all(parts):
        msg = f"malformed Tag line {raw!r} (expected <python>-<abi>-<platform>)"
        raise ValueError(msg)
    return WheelTag(raw=raw.strip(), python=parts[0], abi=parts[1], platform=parts[2])


def read_wheel_tags(wheel: Path) -> list[WheelTag]:
    with zipfile.ZipFile(wheel) as archive:
        wheel_files = [
            name for name in archive.namelist() if name.endswith(".dist-info/WHEEL")
        ]
        if len(wheel_files) != 1:
            msg = (
                f"{wheel.name}: expected exactly one *.dist-info/WHEEL entry, "
                f"found {len(wheel_files)} ({wheel_files})"
            )
            raise ValueError(msg)
        content = archive.read(wheel_files[0]).decode("utf-8")
    tags: list[WheelTag] = []
    for line in content.splitlines():
        if line.startswith("Tag:"):
            _, _, value = line.partition(":")
            tags.append(parse_tag(value))
    if not tags:
        msg = f"{wheel.name}: *.dist-info/WHEEL contains no Tag: lines"
        raise ValueError(msg)
    return tags


def check_platform(
    wheel_name: str,
    tag: WheelTag,
    *,
    expected_platform: str | None,
    max_glibc: tuple[int, int] | None,
    allow_host_tag: bool = False,
) -> list[str]:
    problems: list[str] = []
    plat = tag.platform

    if plat.startswith("linux_"):
        if allow_host_tag:
            # `scripts/build-wheel.sh` without `--manylinux`: a developer wheel,
            # not a release artifact. The caller said so with `--allow-host-tag`,
            # and the sentence is still printed so a log shows which wheel this is.
            return problems
        problems.append(
            f"{wheel_name}: Tag: {tag.raw} uses unversioned host tag {plat!r}; "
            "build with manylinux_2_28 or musllinux_1_2 compatibility"
        )
        return problems

    glibc_version: tuple[int, int] | None = None
    many = MANYLINUX_RE.fullmatch(plat)
    legacy = LEGACY_RE.fullmatch(plat)
    musl = MUSLLINUX_RE.fullmatch(plat)
    if many is not None:
        glibc_version = (int(many.group("major")), int(many.group("minor")))
    elif legacy is not None:
        glibc_version = LEGACY_MANYLINUX[legacy.group("legacy")]
    elif musl is None:
        problems.append(
            f"{wheel_name}: Tag: {tag.raw} carries unrecognised platform {plat!r}"
        )
        return problems

    if expected_platform is not None and not plat.startswith(f"{expected_platform}_"):
        problems.append(
            f"{wheel_name}: Tag: {tag.raw} has platform {plat!r}, "
            f"expected prefix {expected_platform + '_'!r}"
        )

    if (
        max_glibc is not None
        and glibc_version is not None
        and glibc_version > max_glibc
    ):
        problems.append(
            f"{wheel_name}: Tag: {tag.raw} requires glibc {glibc_version[0]}.{glibc_version[1]} "
            f"(manylinux_{glibc_version[0]}_{glibc_version[1]}), which is above the "
            f"manylinux_{max_glibc[0]}_{max_glibc[1]} floor — build inside "
            f"manylinux_{max_glibc[0]}_{max_glibc[1]} (`scripts/build-wheel.sh --manylinux 2_28` "
            "or `.github/actions/build-wheel`)"
        )

    if f"-{plat}" not in wheel_name:
        problems.append(
            f"{wheel_name}: filename does not contain *.dist-info/WHEEL platform {plat!r} "
            f"(Tag: {tag.raw})"
        )

    return problems


def inspect_wheel(
    wheel: Path,
    *,
    expected_platform: str | None,
    max_glibc: tuple[int, int] | None,
    allow_stub: bool,
    allow_host_tag: bool = False,
) -> tuple[list[WheelTag], list[str]]:
    if not wheel.is_file():
        return [], [f"{wheel}: wheel file does not exist"]
    if not zipfile.is_zipfile(wheel):
        if allow_stub:
            return [], []
        return [], [f"{wheel.name}: not a valid zip wheel archive"]
    try:
        tags = read_wheel_tags(wheel)
    except ValueError as err:
        return [], [str(err)]

    problems: list[str] = []
    for tag in tags:
        problems.extend(
            check_platform(
                wheel.name,
                tag,
                expected_platform=expected_platform,
                max_glibc=max_glibc,
                allow_host_tag=allow_host_tag,
            )
        )
    return tags, problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--max-glibc",
        default="",
        help="maximum allowed manylinux glibc <major>_<minor> (e.g. 2_28)",
    )
    parser.add_argument(
        "--platform",
        default="",
        help="required platform tag prefix without arch (e.g. manylinux_2_28, musllinux_1_2)",
    )
    parser.add_argument(
        "--allow-stub",
        action="store_true",
        help="skip non-zip stub files used by README-restore unit tests",
    )
    parser.add_argument(
        "--allow-host-tag",
        action="store_true",
        help=(
            "accept the unversioned `linux_<arch>` tag a host build carries "
            "(scripts/build-wheel.sh without --manylinux: a developer wheel)"
        ),
    )
    parser.add_argument("wheels", nargs="+", help="wheel file(s) to check")
    args = parser.parse_args(argv)

    try:
        max_glibc = parse_glibc_bound(args.max_glibc) if args.max_glibc else None
    except ValueError as err:
        print(f"wheel-tag: error: {err}", file=sys.stderr)
        return 2
    expected_platform = args.platform.strip() or None

    all_problems: list[str] = []
    for raw_path in args.wheels:
        wheel = Path(raw_path)
        tags, problems = inspect_wheel(
            wheel,
            expected_platform=expected_platform,
            max_glibc=max_glibc,
            allow_stub=args.allow_stub,
            allow_host_tag=args.allow_host_tag,
        )
        for tag in tags:
            print(f"wheel-tag: {wheel.name}: Tag: {tag.raw}")
        all_problems.extend(problems)

    if all_problems:
        for problem in all_problems:
            print(f"wheel-tag: FAIL {problem}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
