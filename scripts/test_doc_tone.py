#!/usr/bin/env python3
"""Self-tests for the banned-phrase doc guard (`scripts/check_doc_tone.py`).

`make docs-unit` runs this with pytest. Three things are proved, because a guard
that cannot fail is worse than no guard:

1. **The list is real.** It is non-empty, and every phrase in it is detected on a
   fixture that contains only that phrase — so emptying the list, or breaking the
   matcher, fails here rather than passing silently.
2. **The guard names the hit.** A dirty fixture exits non-zero and prints
   `path:line: banned phrase "..."` for each phrase.
3. **The covered set is the user-facing one.** `README.md` and
   `docs/frameworks/` are read; `docs/design/`, `docs/decisions/`,
   `docs/research/`, `docs/knowledge/`, `docs/testing/` and `docs/VISION.md` are
   not, even when they contain a banned phrase.

Plus the repository itself must be clean, which is the actual acceptance.
"""

from __future__ import annotations

import importlib.util
import subprocess
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
GUARD = HERE / "check_doc_tone.py"


def load_guard():
    """The guard module, imported by path (it is a script, not a package)."""
    spec = importlib.util.spec_from_file_location("check_doc_tone", GUARD)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run_guard(root: Path) -> subprocess.CompletedProcess[str]:
    """The guard as `make check` runs it, against `root`."""
    return subprocess.run(
        [sys.executable, str(GUARD), "--root", str(root)],
        capture_output=True,
        text=True,
        check=False,
    )


def test_the_list_is_non_empty_and_every_phrase_is_detected(tmp_path: Path) -> None:
    guard = load_guard()
    assert guard.BANNED, "an empty banned-phrase list is a guard that cannot fail"
    for index, phrase in enumerate(guard.BANNED):
        root = tmp_path / f"case-{index}"
        (root / "docs").mkdir(parents=True)
        (root / "docs" / "page.md").write_text(
            f"# Page\n\nA clean line.\n{phrase} in a sentence.\n",
            encoding="utf-8",
        )
        assert guard.scan(root) == [("docs/page.md", 4, phrase)], phrase
        result = run_guard(root)
        assert result.returncode == 1, (
            f"{phrase!r} must fail the guard: {result.stdout}"
        )
        assert f'docs/page.md:4: banned phrase "{phrase}"' in result.stderr, (
            result.stderr
        )


def test_a_clean_fixture_passes(tmp_path: Path) -> None:
    root = tmp_path / "clean"
    (root / "docs").mkdir(parents=True)
    (root / "README.md").write_text("# Blastit\n\nWe support PHP 8.3, 8.4 and 8.5.\n")
    (root / "docs" / "page.md").write_text(
        "# Page\n\nA limitation of the runtime, worked around for now.\n",
        encoding="utf-8",
    )
    result = run_guard(root)
    assert result.returncode == 0, result.stderr
    assert "no banned phrase" in result.stdout, result.stdout


def test_the_covered_set_is_the_user_facing_pages(tmp_path: Path) -> None:
    guard = load_guard()
    root = tmp_path / "set"
    (root / "docs" / "frameworks").mkdir(parents=True)
    (root / "docs" / "design").mkdir(parents=True)
    (root / "docs" / "decisions").mkdir(parents=True)
    (root / "docs" / "research").mkdir(parents=True)
    (root / "docs" / "knowledge").mkdir(parents=True)
    (root / "docs" / "testing").mkdir(parents=True)
    (root / "README.md").write_text("# Blastit\n")
    (root / "docs" / "frameworks" / "php.md").write_text("# PHP\n")
    for internal in ("design", "decisions", "research", "knowledge", "testing"):
        (root / "docs" / internal / "note.md").write_text("sorry\n")
    (root / "docs" / "VISION.md").write_text("sorry\n")

    names = {path.relative_to(root).as_posix() for path in guard.covered(root)}
    assert names == {"README.md", "docs/frameworks/php.md"}, names
    assert guard.scan(root) == [], "an internal record is not a user-facing page"


def test_the_repository_is_clean() -> None:
    guard = load_guard()
    findings = guard.scan(REPO)
    assert findings == [], findings
    pages = guard.covered(REPO)
    assert REPO / "README.md" in pages
    assert REPO / "docs" / "frameworks" / "php" / "index.md" in pages


@pytest.mark.parametrize(
    "phrase",
    [
        "not a Blastit bug",
        "we apologize",
        "apologies",
        "host-package fact",
        "unfortunately",
        "sadly",
        "not supported yet",
        "self-inflicted",
        "sorry",
    ],
)
def test_every_named_phrase_is_in_the_list(phrase: str) -> None:
    """The owner's list, pinned literally: dropping one is a test failure."""
    guard = load_guard()
    assert phrase in guard.BANNED
