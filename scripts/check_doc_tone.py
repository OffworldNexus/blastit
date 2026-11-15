#!/usr/bin/env python3
"""The banned-phrase guard for user-facing documentation (node MB.3.65, part B).

A doc states the support set and stops. It does not explain, excuse or soften a
decision we made on purpose — and the way that tone survives review is by
nobody looking for it. This guard looks, on every `make check`.

The list is deliberately **narrow and literal**. No bare "limitation",
"workaround", "for now" or "degraded": those are legitimate technical words, and
a fuzzy rule either fires on real sentences or gets disabled by the first person
it annoys. Each entry below is a phrase no honest page needs.

The covered set is every user-facing page:

* `README.md`, and
* `docs/**/*.md` **except** the internal records — `docs/design/`,
  `docs/decisions/`, `docs/research/`, `docs/knowledge/`, `docs/testing/` and
  `docs/VISION.md`.

Those are working documents where a reviewer's history, a rejected option and a
measured "this is why" belong; the pages a user reads are `docs/` minus them.

Usage (stdlib only; `make doc-tone` runs it):

    scripts/check_doc_tone.py
    scripts/check_doc_tone.py --root /path/to/fixture   # the self-tests' hook

Exit 0 when no covered page contains a banned phrase; exit 1 with one
`path:line: banned phrase "..."` line per hit otherwise.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

#: Directories under `docs/` that are internal records, not user-facing pages.
EXCLUDED_DIRS = (
    "docs/design",
    "docs/decisions",
    "docs/research",
    "docs/knowledge",
    "docs/testing",
)

#: Single pages that are internal for the same reason.
EXCLUDED_FILES = ("docs/VISION.md",)

#: The phrases no user-facing page may contain. Literal, and matched
#: case-insensitively; see the module docstring for why the list stays short.
BANNED: tuple[str, ...] = (
    "not a Blastit bug",
    "we apologize",
    "apologies",
    "host-package fact",
    "unfortunately",
    "sadly",
    "not supported yet",
    "self-inflicted",
    "sorry",
)


def covered(root: Path) -> list[Path]:
    """Every Markdown page the guard reads, under `root`, in a stable order."""
    pages: list[Path] = []
    readme = root / "README.md"
    if readme.is_file():
        pages.append(readme)
    docs = root / "docs"
    if docs.is_dir():
        for path in sorted(docs.rglob("*.md")):
            relative = path.relative_to(root).as_posix()
            if relative in EXCLUDED_FILES:
                continue
            if any(
                relative == excluded or relative.startswith(excluded + "/")
                for excluded in EXCLUDED_DIRS
            ):
                continue
            pages.append(path)
    return pages


def scan(root: Path) -> list[tuple[str, int, str]]:
    """Every `(page, line, phrase)` hit under `root`, in file and line order."""
    findings: list[tuple[str, int, str]] = []
    for path in covered(root):
        relative = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8")
        for number, line in enumerate(text.splitlines(), 1):
            lowered = line.lower()
            for phrase in BANNED:
                if phrase.lower() in lowered:
                    findings.append((relative, number, phrase))
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
    findings = scan(root)
    for page, line, phrase in findings:
        print(f'{page}:{line}: banned phrase "{phrase}"', file=sys.stderr)
    if findings:
        print(
            f"doc-tone: {len(findings)} banned phrase(s) in user-facing docs; state the "
            "support set and stop (node MB.3.65, part B)",
            file=sys.stderr,
        )
        return 1
    print(f"doc-tone: ok — {len(covered(root))} user-facing page(s), no banned phrase")
    return 0


if __name__ == "__main__":
    sys.exit(main())
