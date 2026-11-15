#!/usr/bin/env python3
"""The `0.2.x` Linux packaging matrix: the one place a leg is declared (MB.3.18).

Every job that builds a distribution artifact — the wheels, the standalone
archives and the PHP SAPI shims — takes its list of legs from here, and every
document that states a count is checked against here. A leg is added (or
re-enabled) once, in `LEGS`, and appears in all of them; nothing is copied per
job. `ADR-0010` §4 is the table of record for what the artifacts contain; this
module is the table of record for *which* targets are attempted, and for what
has actually been built rather than merely declared.

Three things make this more than a matrix constant:

1. **Provenance is data, not prose.** `PROVEN` lists the artifacts that were
   built *and* exercised (a wheel installed and served, an archive executed, a
   shim `dlopen`ed and probed), with the node that did it. `--summary` prints
   declared next to proved, separately, because they are different claims: a
   declared leg is one CI builds on the release PR, a proved one is one whose
   artifact has been run.

2. **A count in a document is a claim, and claims are checked.** `--check`
   reads the published pages *and* the internal design records (both are
   edited by this repo, and the ADR is the table of record a reviewer reads),
   refusing any `N wheels/archives/shims` above the declared matrix and any
   `N proven …` above `PROVEN`. Internal hits are also listed as notes, so the
   report says which document states what.

3. **A dropped leg keeps its reason.** `enabled: False` carries
   `dropped_because` in one sentence, and `--summary` quotes it, so the honest
   count is computed from what is enabled and the reason does not live in a
   commit message.

Usage (stdlib only; `make platform-matrix-check` runs the first):

    scripts/platform_matrix.py --check
    scripts/platform_matrix.py --matrix wheels      # GitHub Actions matrix JSON
    scripts/platform_matrix.py --matrix binaries
    scripts/platform_matrix.py --matrix shims
    scripts/platform_matrix.py --matrix images
    scripts/platform_matrix.py --summary            # Markdown, for a RESULT
    scripts/platform_matrix.py --count shims --libc gnu --arch x86_64
    scripts/platform_matrix.py --leg shims --minor 8.3 --libc gnu --arch x86_64
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# --- the matrix -------------------------------------------------------------

# `runs_on` is the native runner for the arch, never an emulator: `ADR-0010` §4.2
# forbids cross-compiling a shim against a foreign `php_config.h`, and an
# emulated runner would make the probe evidence about qemu's syscalls.
ARCHES = {
    "x86_64": {"runs_on": "ubuntu-24.04", "manylinux_arch": "x86_64"},
    "aarch64": {"runs_on": "ubuntu-24.04-arm", "manylinux_arch": "aarch64"},
}

LIBCS = {
    "gnu": {
        "wheel_tag": "manylinux_2_28",
        "manylinux": "2_28",
        # The image a packaged archive is *executed* in for its runtime proof: a
        # distro with the matching C library and no CPython at all.
        "runtime_image": "debian:trixie-slim",
    },
    "musl": {
        "wheel_tag": "musllinux_1_2",
        "musllinux": "1_2",
        "runtime_image": "alpine:3.21",
    },
}

PYTHONS = ("3.13", "3.14")

# The distro that carries the headers *and* the embedded SAPI for each
# (minor, libc). Every entry is a signed distro package on a distro that ships
# that minor: no single distro ships 8.2, 8.3 and 8.4, and Alpine and Debian
# disagree about both the package names (`php82-dev` vs `php8.2-dev`) and where
# the library lives (`/usr/lib/php82/libphp.so` vs `/usr/lib/libphp8.2.so`).
PHP_BUILDS = {
    ("8.2", "gnu"): {
        "base": "debian:bookworm",
        "dev": "php8.2-dev",
        "embed": "libphp8.2-embed",
        "curl": "php8.2-curl",
        "libphp": "/usr/lib/libphp8.2.so",
    },
    ("8.3", "gnu"): {
        "base": "ubuntu:24.04",
        "dev": "php8.3-dev",
        "embed": "libphp8.3-embed",
        "curl": "php8.3-curl",
        "libphp": "/usr/lib/libphp8.3.so",
    },
    ("8.4", "gnu"): {
        "base": "debian:trixie-slim",
        "dev": "php8.4-dev",
        "embed": "libphp8.4-embed",
        "curl": "php8.4-curl",
        "libphp": "/usr/lib/libphp8.4.so",
    },
    ("8.2", "musl"): {
        "base": "alpine:3.20",
        "dev": "php82-dev",
        "embed": "php82-embed",
        "curl": "php82-curl",
        # Alpine's layout is per-minor and not uniform: 8.2 and 8.3 install
        # `libphpXY.so` at `/usr/lib`, 8.4 installs `/usr/lib/php84/libphp.so`.
        "libphp": "/usr/lib/libphp82.so",
    },
    ("8.3", "musl"): {
        "base": "alpine:3.21",
        "dev": "php83-dev",
        "embed": "php83-embed",
        "curl": "php83-curl",
        "libphp": "/usr/lib/libphp83.so",
    },
    ("8.4", "musl"): {
        "base": "alpine:3.21",
        "dev": "php84-dev",
        "embed": "php84-embed",
        "curl": "php84-curl",
        "libphp": "/usr/lib/php84/libphp.so",
    },
}

PHP_MINORS = {"8.2": "82", "8.3": "83", "8.4": "84"}

# A leg is `(arch, libc)`. `enabled` is the switch that turns a target into an
# artifact; a disabled leg must say why in one sentence.
LEGS = (
    {"id": "x86_64-gnu", "arch": "x86_64", "libc": "gnu", "enabled": True},
    {"id": "x86_64-musl", "arch": "x86_64", "libc": "musl", "enabled": True},
    {"id": "aarch64-gnu", "arch": "aarch64", "libc": "gnu", "enabled": True},
    {"id": "aarch64-musl", "arch": "aarch64", "libc": "musl", "enabled": True},
)

# Built **and** exercised. A key that is absent is declared and unbuilt: CI
# builds it on the release PR, and until it has run the artifact is a promise,
# not evidence. The value names the node that ran it, because "proven" without a
# who/when is the claim this file exists to keep honest.
PROVEN: dict[tuple, str] = {
    # `MB.3.12` built and probed 8.4 on x86_64-gnu; `MB.3.18` did the rest.
    ("shims", "8.4", "x86_64", "gnu"): "MB.3.12",
    ("shims", "8.3", "x86_64", "gnu"): "MB.3.18",
    ("shims", "8.2", "x86_64", "gnu"): "MB.3.18",
    ("shims", "8.4", "x86_64", "musl"): "MB.3.18",
    ("shims", "8.3", "x86_64", "musl"): "MB.3.18",
    ("shims", "8.2", "x86_64", "musl"): "MB.3.18",
    ("wheels", "3.13", "x86_64", "gnu"): "MB.3.18",
    ("wheels", "3.14", "x86_64", "gnu"): "MB.3.18",
    ("wheels", "3.13", "x86_64", "musl"): "MB.3.18",
    ("wheels", "3.14", "x86_64", "musl"): "MB.3.18",
    ("archives", "x86_64", "gnu"): "MB.3.18",
    ("archives", "x86_64", "musl"): "MB.3.18",
}

# Consumer images (`ADR-0010` §4.3), one tag per libc.
IMAGES = (
    {"tag": "ghcr.io/offworldnexus/blastit:php8.4", "minor": "8.4", "libc": "gnu"},
    {
        "tag": "ghcr.io/offworldnexus/blastit:php8.4-alpine",
        "minor": "8.4",
        "libc": "musl",
    },
)

# --- derived views ----------------------------------------------------------


def enabled_legs() -> list[dict]:
    return [leg for leg in LEGS if leg["enabled"]]


def wheel_legs() -> list[dict]:
    """One wheel per enabled (arch, libc) and CPython in `PYTHONS`."""
    return [
        {
            "name": f"{leg['id']}-cp{python.replace('.', '')}",
            "arch": leg["arch"],
            "libc": leg["libc"],
            "python": python,
            "python_tag": f"cp{python.replace('.', '')}",
            "wheel_tag": LIBCS[leg["libc"]]["wheel_tag"],
            "runtime_image": f"python:{python}-{'alpine' if leg['libc'] == 'musl' else 'slim'}",
            "runs_on": ARCHES[leg["arch"]]["runs_on"],
            "manylinux": LIBCS[leg["libc"]].get("manylinux", ""),
        }
        for leg in enabled_legs()
        for python in PYTHONS
    ]


def binary_legs() -> list[dict]:
    """One archive per enabled (arch, libc)."""
    return [
        {
            "name": leg["id"],
            "arch": leg["arch"],
            "libc": leg["libc"],
            "target": f"{leg['arch']}-unknown-linux-{leg['libc']}",
            "archive": f"blastit-{leg['arch']}-unknown-linux-{leg['libc']}.tar.gz",
            "runtime_image": LIBCS[leg["libc"]]["runtime_image"],
            "runs_on": ARCHES[leg["arch"]]["runs_on"],
        }
        for leg in enabled_legs()
    ]


def shim_legs() -> list[dict]:
    """One shim per (minor, arch, libc), named `ADR-0010` §4.1's way."""
    return [
        {
            "name": f"{leg['id']}-php{digits}",
            "arch": leg["arch"],
            "libc": leg["libc"],
            "minor": minor,
            "digits": digits,
            "shim": shim_name(minor, leg["arch"], leg["libc"]),
            "base": PHP_BUILDS[(minor, leg["libc"])]["base"],
            "dev": PHP_BUILDS[(minor, leg["libc"])]["dev"],
            "embed": PHP_BUILDS[(minor, leg["libc"])]["embed"],
            "curl": PHP_BUILDS[(minor, leg["libc"])]["curl"],
            "libphp": PHP_BUILDS[(minor, leg["libc"])]["libphp"],
            "runs_on": ARCHES[leg["arch"]]["runs_on"],
        }
        for leg in enabled_legs()
        for minor, digits in PHP_MINORS.items()
    ]


def shim_name(minor: str, arch: str, libc: str) -> str:
    """`libblastit_php84-x86_64-gnu.so` (`ADR-0010` §4.1)."""
    return f"libblastit_php{PHP_MINORS[minor]}-{arch}-{libc}.so"


def proved(family: str) -> list[tuple]:
    return sorted(key for key in PROVEN if key[0] == family)


def proven_count(family: str) -> int:
    return len(proved(family))


# --- the check --------------------------------------------------------------


# Every page that states a count. `docs/` is scanned whole — hand-picking six
# pages is how the gate went vacuous the first time (the ADR and the design doc
# are the two documents a reviewer reads, and both were outside the list) — plus
# `README.md`, which is the wheel's long description.
#
# The two internal records get a `note` line either way, so `--check` says which
# document states what instead of leaving it to be discovered; the published
# pages are checked the same way.
def _pages() -> list[tuple[str, Path]]:
    """Every Markdown page the repository ships, plus the README.

    All of them, not a hand-picked few: the first version of this check looked at
    six pages and missed the two documents a reviewer reads (`ADR-0010` §4 and
    `backend-runtimes.md` §4.7, both of whose counts are backticked — see `MARK`),
    which made the gate vacuous exactly where it mattered. A page added under
    `docs/` is scanned the day it lands.
    """
    pages = [("README.md", REPO / "README.md")]
    for path in sorted((REPO / "docs").rglob("*.md")):
        pages.append((path.relative_to(REPO).as_posix(), path))
    return pages


INTERNAL_DOCS = frozenset(
    {
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "docs/design/backend-runtimes.md",
    }
)

# `N shims`, "`8` wheels", "**4** proven PHP shims": every live count in the ADR
# and the design doc is backticked, so the digit is never bare and a pattern that
# insists on `\d+` matches nothing. `MARK` is the wrapper the Markdown actually
# uses (backticks, bold, italics), and the qualifier allows the words that sit
# between the number and the noun ("native PHP", "standalone", "proven").
MARK = r"[`*_]*"
# The words that sit between a number and its noun, plus one inline code span:
# `ADR-0009` says "up to `3` proven `libblastit_php{82,83,84}-…so` shims", where
# the artifact name separates the count from the noun.
QUALIFIER = r"(?:proven\s+|PHP\s+|native\s+|standalone\s+|`[^`\n]*`\s+)*"
CLAIM = re.compile(
    r"(?<![A-Za-z0-9._-])"
    + MARK
    + r"(?P<count>\d+)"
    + MARK
    + r"\s+"
    + QUALIFIER
    + r"(?P<family>shims?|wheels?|archives?|tarballs?)",
    re.IGNORECASE,
)
PROVEN_CLAIM = re.compile(
    r"(?<![A-Za-z0-9._-])"
    + MARK
    + r"(?P<count>\d+)"
    + MARK
    + r"\s+proven\s+"
    + QUALIFIER
    + r"(?P<family>shims?|wheels?|archives?|tarballs?)",
    re.IGNORECASE,
)

# The lines the gate must keep matching, quoted from the two records it exists to
# guard. If a Markdown edit (or a regex change) makes the scanner miss them,
# `--check` says so instead of quietly checking nothing — a gate that matches no
# document is worse than no gate, because it reports `ok`.
PINNED: tuple[tuple[str, str, str], ...] = (
    (
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "`8` wheels",
        "declared wheel count",
    ),
    (
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "`4` tarballs",
        "declared archive count",
    ),
    (
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "`12` shims",
        "declared shim count",
    ),
    (
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "`3` shims",
        "per-tuple shim count (8.2/8.3/8.4)",
    ),
    (
        "docs/design/backend-runtimes.md",
        "`8` wheels",
        "declared wheel count",
    ),
    (
        "docs/design/backend-runtimes.md",
        "`12` native PHP shims",
        "declared shim count",
    ),
    (
        "docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md",
        "`3` proven",
        "ADR-0009's per-target proven shim count",
    ),
)


def declared_of(family: str) -> int:
    """How many artifacts of this family the matrix declares."""
    family = family.lower().rstrip("s")
    return {
        "shim": len(shim_legs()),
        "wheel": len(wheel_legs()),
        "archive": len(binary_legs()),
        "tarball": len(binary_legs()),
        # `images` is not a count any page states yet; 0 refuses the claim.
        "image": 0,
    }[family]


def proven_of(family: str) -> int:
    family = family.lower().rstrip("s")
    return {
        "shim": proven_count("shims"),
        "wheel": proven_count("wheels"),
        "archive": proven_count("archives"),
        "tarball": proven_count("archives"),
        "image": 0,
    }[family]


def check(problems: list[str], notes: list[str]) -> None:
    ids = [leg["id"] for leg in LEGS]
    if len(set(ids)) != len(ids):
        problems.append(f"duplicate leg id in LEGS: {ids}")

    for leg in LEGS:
        if leg["arch"] not in ARCHES:
            problems.append(f"{leg['id']}: unknown arch {leg['arch']!r}")
        if leg["libc"] not in LIBCS:
            problems.append(f"{leg['id']}: unknown libc {leg['libc']!r}")
        if not leg["enabled"] and not leg.get("dropped_because"):
            problems.append(
                f"{leg['id']}: disabled with no reason: the matrix records why a "
                "leg is not attempted, or `--summary` cannot be honest"
            )
        if leg["enabled"] and leg.get("dropped_because"):
            problems.append(f"{leg['id']}: enabled and dropped at the same time")

    # Every (arch, libc) pair is declared, enabled or dropped: a target that
    # silently vanishes from LEGS is the failure this file exists to prevent.
    declared = {(leg["arch"], leg["libc"]) for leg in LEGS}
    for arch in ARCHES:
        for libc in LIBCS:
            if (arch, libc) not in declared:
                problems.append(
                    f"{arch}-{libc} is not declared: declare it enabled or dropped "
                    "with a reason, never leave it out"
                )

    for (minor, libc), spec in PHP_BUILDS.items():
        if minor not in PHP_MINORS:
            problems.append(f"PHP_BUILDS names an unknown minor {minor!r}")
        for key in ("base", "dev", "embed", "curl", "libphp"):
            if not spec.get(key):
                problems.append(
                    f"PHP {minor} on {libc}: no {key}: the leg cannot be built"
                )
    for minor in PHP_MINORS:
        for leg in LEGS:
            if (minor, leg["libc"]) not in PHP_BUILDS:
                problems.append(
                    f"PHP {minor} on {leg['libc']}: no build record, so {leg['id']} "
                    "cannot produce that shim"
                )

    for leg in shim_legs():
        expected = shim_name(leg["minor"], leg["arch"], leg["libc"])
        if leg["shim"] != expected:
            problems.append(f"{leg['name']}: shim {leg['shim']} != {expected}")
        if not re.fullmatch(
            r"libblastit_php8\d-(x86_64|aarch64)-(gnu|musl)\.so", leg["shim"]
        ):
            problems.append(f"{leg['name']}: {leg['shim']} is not the §4.1 name")

    # Every declared artifact has a place in PROVEN or it is a promise; every
    # PROVEN key names an artifact the matrix actually builds.
    declared_keys = (
        {("shims", leg["minor"], leg["arch"], leg["libc"]) for leg in shim_legs()}
        | {("wheels", leg["python"], leg["arch"], leg["libc"]) for leg in wheel_legs()}
        | {("archives", leg["arch"], leg["libc"]) for leg in binary_legs()}
    )
    for key in PROVEN:
        if key not in declared_keys:
            problems.append(
                f"PROVEN names {key}, which this matrix does not build: a proof of "
                "something no artifact exists for is not evidence"
            )

    for image in IMAGES:
        if not image.get("tag"):
            problems.append("an image entry has no tag")

    # The pinned lines first: if the scanner no longer sees them, every count
    # below is being read from a document it does not understand, and an `ok`
    # would be a lie.
    texts: dict[str, str] = {}
    for name, target in _pages():
        if not target.exists():
            problems.append(f"{name}: a page named here is missing")
            continue
        texts[name] = target.read_text(encoding="utf-8")
    for name, needle, what in PINNED:
        text = texts.get(name)
        if text is None:
            # A pin whose document moved is a pin that checks nothing.
            problems.append(
                f"PINNED names {name}, which this repository does not have: a count "
                "gate that pins a missing document is not a gate"
            )
            continue

        # Markdown wrappers are compared away on both sides: the regex may consume
        # the opening backtick or the `@` before it, and a pin should fail on the
        # *count* disappearing, not on which character swallowed the wrapper.
        def bare(chunk: str) -> str:
            return chunk.replace("`", "").replace("*", "")

        if not any(
            bare(needle) in bare(match.group(0)) for match in CLAIM.finditer(text)
        ):
            problems.append(
                f"{name}: the scanner no longer matches {needle!r} ({what}): a "
                "count gate that sees no count is not a gate"
            )

    # The proven-count rule, applied to every page that states a count.
    for name, text in texts.items():
        internal = name in INTERNAL_DOCS
        label = "note" if internal else "claim"
        for match in CLAIM.finditer(text):
            count = int(match.group("count"))
            if count == 0:
                # "zero shims in the wheel" is a true statement about a family
                # that does not travel there; it is not a claim about the matrix.
                continue
            family = match.group("family")
            ceiling = declared_of(family)
            line = text[: match.start()].count("\n") + 1
            if count > ceiling:
                problems.append(
                    f"{name}:{line}: claims {count} {family}, the matrix declares "
                    f"{ceiling}: a page may not promise a leg that is not attempted"
                )
            notes.append(
                f"{label} {name}:{line}: {count} {family} "
                f"(matrix {ceiling}, proved {proven_of(family)})"
            )
        for match in PROVEN_CLAIM.finditer(text):
            count = int(match.group("count"))
            if count == 0:
                continue
            family = match.group("family")
            ceiling = proven_of(family)
            line = text[: match.start()].count("\n") + 1
            if count > ceiling:
                problems.append(
                    f"{name}:{line}: claims {count} proven {family}, "
                    f"{ceiling} are built and exercised (PROVEN)"
                )
            notes.append(
                f"{label} {name}:{line}: {count} proven {family} (proved {ceiling})"
            )


# --- the output -------------------------------------------------------------


def summary() -> str:
    lines = [
        "| family | declared | built & exercised | dropped (reason) |",
        "|---|---|---|---|",
    ]
    dropped = [leg for leg in LEGS if not leg["enabled"]]
    reason = (
        "; ".join(f"{leg['id']}: {leg['dropped_because']}" for leg in dropped)
        if dropped
        else "—"
    )
    for name, family, count in (
        ("wheels", "wheels", len(wheel_legs())),
        ("standalone archives", "archives", len(binary_legs())),
        ("PHP shims", "shims", len(shim_legs())),
    ):
        lines.append(
            f"| {name} | {count} | {proven_count(family)} of {count} | {reason} |"
        )
    lines.append("")
    lines.append(
        "Declared is what CI builds on the release PR; built & exercised is what "
        "has been run (PROVEN)."
    )
    lines.append("")
    lines.append("| artifact | leg | status |")
    lines.append("|---|---|---|")
    for leg in wheel_legs():
        node = PROVEN.get(("wheels", leg["python"], leg["arch"], leg["libc"]))
        lines.append(
            f"| wheel cp{leg['python'].replace('.', '')} `{leg['name']}` | "
            f"{leg['runs_on']} | {'proved by ' + node if node else 'declared, unbuilt'} |"
        )
    for leg in binary_legs():
        node = PROVEN.get(("archives", leg["arch"], leg["libc"]))
        lines.append(
            f"| `{leg['archive']}` | {leg['runs_on']} | "
            f"{'proved by ' + node if node else 'declared, unbuilt'} |"
        )
    for leg in shim_legs():
        node = PROVEN.get(("shims", leg["minor"], leg["arch"], leg["libc"]))
        lines.append(
            f"| `{leg['shim']}` | {leg['base']} ({leg['runs_on']}) | "
            f"{'proved by ' + node if node else 'declared, unbuilt'} |"
        )
    return "\n".join(lines)


def _filtered_legs(
    family: str,
    *,
    minor: str = "",
    python: str = "",
    libc: str = "",
    arch: str = "",
) -> list[dict]:
    legs = {
        "wheels": wheel_legs,
        "binaries": binary_legs,
        "shims": shim_legs,
        "images": lambda: [dict(item) for item in IMAGES],
    }[family]()
    if minor:
        legs = [leg for leg in legs if leg.get("minor") == minor]
    if python:
        legs = [leg for leg in legs if leg.get("python") == python]
    if libc:
        legs = [leg for leg in legs if leg.get("libc") == libc]
    if arch:
        legs = [leg for leg in legs if leg.get("arch") == arch]
    return legs


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--check",
        action="store_true",
        help="validate the matrix and every stated count",
    )
    parser.add_argument("--matrix", choices=("wheels", "binaries", "shims", "images"))
    parser.add_argument(
        "--count",
        choices=("wheels", "binaries", "shims", "images"),
        help="print how many enabled legs match the filters (exits 0 even when 0)",
    )
    parser.add_argument("--summary", action="store_true")
    parser.add_argument("--leg", choices=("wheels", "binaries", "shims"))
    parser.add_argument(
        "--minor",
        default="",
        help="with --leg/--count shims: filter by PHP minor (8.2, 8.3, 8.4)",
    )
    parser.add_argument(
        "--python",
        default="",
        help="with --leg/--count wheels: filter by CPython minor (3.13, 3.14)",
    )
    parser.add_argument(
        "--libc",
        default="",
        choices=("", "gnu", "musl"),
        help="filter to this libc (gnu or musl)",
    )
    parser.add_argument(
        "--arch",
        default="",
        choices=("", "x86_64", "aarch64"),
        help="filter to this arch (x86_64 or aarch64)",
    )
    parser.add_argument(
        "--quiet", action="store_true", help="--check without the per-document notes"
    )
    args = parser.parse_args(argv)

    problems: list[str] = []
    notes: list[str] = []
    if args.check:
        check(problems, notes)
    if notes and not args.quiet:
        for note in notes:
            print(f"platform-matrix: {note}")
    if problems:
        for problem in problems:
            print(f"platform-matrix: {problem}", file=sys.stderr)
        return 1

    if args.matrix:
        legs = _filtered_legs(
            args.matrix,
            minor=args.minor,
            python=args.python,
            libc=args.libc,
            arch=args.arch,
        )
        print(json.dumps({"include": legs}, indent=None, separators=(",", ":")))
    if args.count:
        legs = _filtered_legs(
            args.count,
            minor=args.minor,
            python=args.python,
            libc=args.libc,
            arch=args.arch,
        )
        print(len(legs))
    if args.leg:
        legs = _filtered_legs(
            args.leg,
            minor=args.minor,
            python=args.python,
            libc=args.libc,
            arch=args.arch,
        )
        if len(legs) == 0:
            print(
                f"platform-matrix: --leg {args.leg} matched 0 enabled legs "
                f"(minor={args.minor!r}, python={args.python!r}, libc={args.libc!r}, arch={args.arch!r})",
                file=sys.stderr,
            )
            return 2
        if len(legs) > 1:
            missing: list[str] = []
            if args.leg == "shims" and not args.minor:
                missing.append("--minor")
            if args.leg == "wheels" and not args.python:
                missing.append("--python")
            if not args.libc:
                missing.append("--libc")
            if not args.arch:
                missing.append("--arch")
            hint = (
                f"pass {', '.join(missing)} (or use --count {args.leg})"
                if missing
                else f"use --count {args.leg} or --matrix {args.leg}"
            )
            print(
                "platform-matrix: --leg matched "
                f"{len(legs)} legs ({[leg.get('name') for leg in legs]}): {hint}",
                file=sys.stderr,
            )
            return 2
        print("\n".join(f"{key}={value}" for key, value in legs[0].items()))
    if args.summary:
        print(summary())
    if args.check and not (args.matrix or args.count or args.leg or args.summary):
        print(
            f"platform-matrix: ok — {len(wheel_legs())} wheels, "
            f"{len(binary_legs())} archives, {len(shim_legs())} shims declared; "
            f"built and exercised: {proven_count('wheels')}/{len(wheel_legs())} wheels, "
            f"{proven_count('archives')}/{len(binary_legs())} archives, "
            f"{proven_count('shims')}/{len(shim_legs())} shims; "
            f"{len([leg for leg in LEGS if not leg['enabled']])} leg(s) dropped"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
