#!/usr/bin/env python3
"""The `std::env` guard for `crates/blastit-manifest/src/` (node MB.21.3.1).

A `Blastfile` names environment variables; it never reads them. `$DATABASE_URL`
is a name that the lowering step copies into `Proxy.input`, and the value is
resolved by `blastit-env` at boot. That is what makes a manifest safe to commit
and what keeps the parser from becoming an oracle for secrets (design §17.2),
so a `std::env` read under `crates/blastit-manifest/src/` is a build failure
rather than a review comment (R20, R24).

The rule is literal, comments included: the design's own words are "a `gate`
scan fails the build if `std::env` appears under
`crates/blastit-manifest/src/`". A file that needs to explain the rule says
"the environment" instead.

Usage (stdlib only; `make manifest-purity` runs it):

    scripts/check_manifest_purity.py
    scripts/check_manifest_purity.py --root /path/to/fixture   # self-test hook

Exit 0 when no source file under the covered tree mentions `std::env`; exit 1
with one `path:line: …` line per hit otherwise, and exit 1 rather than pass
when the covered tree is missing or empty.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

#: The tree the rule covers, relative to the scanned root.
COVERED = Path("crates/blastit-manifest/src")

#: The needle. `use std::env;`, `std::env::var(…)` and `std::env!` all hold it.
NEEDLE = "std::env"


def covered(root: Path) -> list[Path]:
    """Every Rust source file the guard reads, under `root`, in a stable order."""
    tree = root / COVERED
    if not tree.is_dir():
        return []
    return sorted(path for path in tree.rglob("*.rs") if path.is_file())


def scan(root: Path) -> list[tuple[str, int, str]]:
    """Every `(file, line, text)` hit under `root`, in file and line order."""
    findings: list[tuple[str, int, str]] = []
    for path in covered(root):
        relative = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8")
        for number, line in enumerate(text.splitlines(), 1):
            if NEEDLE in line:
                findings.append((relative, number, line.strip()))
    return findings


def main(argv: list[str] | None = None) -> int:
    """Runs the guard; returns the process exit status."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--root",
        default=str(REPO),
        help="the tree to scan (default: this repository)",
    )
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    files = covered(root)
    if not files:
        print(
            f"manifest-purity: no Rust source under {COVERED.as_posix()}/ in "
            f"{root}; refusing to pass",
            file=sys.stderr,
        )
        return 1
    findings = scan(root)
    for path, line, text in findings:
        print(f"{path}:{line}: {NEEDLE} is not allowed here: {text}", file=sys.stderr)
    if findings:
        print(
            f"manifest-purity: {len(findings)} {NEEDLE} use(s) under "
            f"{COVERED.as_posix()}/; a Blastfile names variables and never reads "
            "them (R20/R24)",
            file=sys.stderr,
        )
        return 1
    print(f"manifest-purity: ok — {len(files)} source file(s), no {NEEDLE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
