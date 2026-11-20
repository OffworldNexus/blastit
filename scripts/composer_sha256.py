#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Stamp the Composer wrapper's embedded digest table from built archives.

`packages/composer-blastit/bin/blastit` verifies the release archive it runs
against a **table it carries** (design §6.1). That table is generated, never
typed: a hand-written digest is a digest nobody checked, and a stale one turns
every `vendor/bin/blastit` into a refusal.

    make composer-stamp                      # --version $(BLASTIT_VERSION) --archives dist/*.tar.gz
    scripts/composer_sha256.py --version 0.2.0 --archives dist/*.tar.gz --require-complete

Each archive is named `blastit-<arch>-unknown-linux-<libc>.tar.gz` (the
`binaries` family in `scripts/platform_matrix.py`); its `(arch, libc)` row is
rewritten with the digest, and rows with no archive keep their previous value —
`pending` on a fresh release, which the wrapper refuses by name rather than
running something unverified. `--version` also rewrites the pinned `VERSION` the
wrapper downloads, so the two cannot disagree.

`--require-complete` is what a **release** runs before packaging: it exits 1
while any row is still `pending`, because a `pending` row is a `vendor/bin/blastit`
that refuses to run — the wrapper fails closed rather than trusting an origin, and
shipping that on purpose is a broken `composer require`. The default-origin path
is the only one a real `composer require` takes, so a table nobody stamped is a
package nobody can use (`scripts/composer_smoke.sh` runs it that way).

Exit status: 0 when the file is up to date (or was updated) and, with
`--require-complete`, every row carries a digest; 1 on an archive whose name is
not one of the four legs, or on a `pending` row under `--require-complete`.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
from pathlib import Path

WRAPPER = Path("packages/composer-blastit/bin/blastit")
ARCHIVE = re.compile(
    r"^blastit-(?P<arch>x86_64|aarch64)-unknown-linux-(?P<libc>gnu|musl)\.tar\.gz$"
)
ROW = re.compile(
    r"^(?P<arch>x86_64|aarch64) (?P<libc>gnu|musl) "
    r"(?P<archive>blastit-[^\s]+\.tar\.gz) (?P<digest>\S+)$",
    re.MULTILINE,
)
VERSION = re.compile(r'^VERSION="(?P<version>[^"]+)"$', re.MULTILINE)


def digest_of(path: Path) -> str:
    """The archive's SHA-256, lowercase hex — what `sha256sum` prints."""
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def stamp(text: str, archives: dict[tuple[str, str], tuple[str, str]]) -> str:
    """`text` with each `(arch, libc)` row's digest replaced."""
    by_archive = {archive: digest for (archive, digest) in archives.values()}

    def replace(match: re.Match[str]) -> str:
        digest = by_archive.get(match["archive"])
        if digest is None:
            return match.group(0)
        return f"{match['arch']} {match['libc']} {match['archive']} {digest}"

    return ROW.sub(replace, text)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--version", required=True, help="the release the table is for")
    parser.add_argument(
        "positional_archives",
        nargs="*",
        help="built blastit-<arch>-unknown-linux-<libc>.tar.gz",
    )
    parser.add_argument(
        "--archives",
        nargs="*",
        default=[],
        help="the same, as a flag: `--archives dist/*.tar.gz` reads at the call site",
    )
    parser.add_argument(
        "--wrapper", default=str(WRAPPER), help="the wrapper to rewrite"
    )
    parser.add_argument(
        "--require-complete",
        action="store_true",
        help="exit 1 while any asset row is still `pending` (a release runs this)",
    )
    args = parser.parse_args(argv)

    archives: dict[tuple[str, str], tuple[str, str]] = {}
    for raw in [*args.positional_archives, *args.archives]:
        path = Path(raw)
        match = ARCHIVE.fullmatch(path.name)
        if match is None:
            print(
                f"composer-sha256: error: {path.name} is not a "
                "blastit-<arch>-unknown-linux-<libc>.tar.gz release asset",
                file=sys.stderr,
            )
            return 1
        archives[(match["arch"], match["libc"])] = (path.name, digest_of(path))

    wrapper = Path(args.wrapper)
    text = wrapper.read_text(encoding="utf-8")
    if VERSION.search(text) is None:
        print(f"composer-sha256: error: {wrapper} has no VERSION line", file=sys.stderr)
        return 1
    updated = VERSION.sub(f'VERSION="{args.version}"', text, count=1)
    updated = stamp(updated, archives)
    if updated != text:
        wrapper.write_text(updated, encoding="utf-8")
    stamped = ", ".join(sorted(f"{arch}-{libc}" for arch, libc in archives)) or "none"
    print(
        f"composer-sha256: {wrapper} stamps v{args.version} ({stamped} from this run)"
    )

    # `--require-complete`: a `pending` row is a `vendor/bin/blastit` that refuses
    # to run, so a release must not package the table while one is left.
    pending = [
        match["archive"]
        for match in ROW.finditer(updated)
        if match["digest"] == "pending"
    ]
    if pending and args.require_complete:
        print(
            "composer-sha256: error: the table still has no digest for "
            f"{', '.join(pending)} — every asset must be built and stamped before "
            "packaging, or `vendor/bin/blastit` refuses that host by name",
            file=sys.stderr,
        )
        return 1
    if pending:
        print(
            f"composer-sha256: note: {len(pending)} row(s) still `pending` "
            "(`--require-complete` refuses this before a release)"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
