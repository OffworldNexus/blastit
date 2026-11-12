"""Run every public console fence and print the verdict table.

The inventory is checked before anything runs: a public page or a fence that has
no entry is a failure, not a silent pass. Then the harness executes the fences.
"""

from __future__ import annotations

import fcntl
import sys
import tempfile
import time
from pathlib import Path
from typing import TextIO

from fences import console_fences, public_pages
from harness import (
    MODES,
    NEEDS_REASON,
    REPO,
    Failure,
    Harness,
    PageSpec,
    Report,
    load_inventory,
)

INVENTORY = REPO / "python" / "tests" / "runnable_docs" / "inventory.toml"
PORT_FALLBACK_MODES = {"dev-flags"}
# Two gates on one machine would fight over the documented ports, so they queue
# here instead of one failing the other's fence for a busy 8080.
LOCK_TIMEOUT = 900.0


def exclusive_lock() -> TextIO:
    """Hold the harness's port lock, waiting for a concurrent gate to finish."""
    path = Path(tempfile.gettempdir()) / "blastit-runnable-docs.lock"
    handle = path.open("w", encoding="utf-8")
    deadline = time.monotonic() + LOCK_TIMEOUT
    waiting = False
    while True:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return handle
        except BlockingIOError:
            if not waiting:
                print(
                    f"another runnable-docs gate holds the documented ports; "
                    f"waiting for {path}",
                    file=sys.stderr,
                )
                waiting = True
            if time.monotonic() > deadline:
                handle.close()
                raise Failure(
                    f"{path} is held by another run after {LOCK_TIMEOUT:.0f}s; "
                    "stop that run (`make docs-test` in another terminal) or "
                    "delete the stale lock file if no run is alive"
                ) from None
            time.sleep(0.5)


def main() -> int:
    started = time.monotonic()
    pages = load_inventory(INVENTORY)
    problems = check_inventory(pages)
    if problems:
        for problem in problems:
            print(f"inventory: {problem}", file=sys.stderr)
        return 1
    report = Report()
    selected = set(sys.argv[1:])
    if selected:
        pages = [page for page in pages if page.path in selected]
        if not pages:
            print(f"no page matches {sorted(selected)}", file=sys.stderr)
            return 1
    lock = exclusive_lock()
    try:
        harness = Harness(pages, report)
    except Failure as failure:
        lock.close()
        print(f"harness: {failure}", file=sys.stderr)
        return 1
    # Provenance on stderr, so the verdict table on stdout stays reproducible.
    # The per-run cache starts empty and the installed package is compared with
    # this wheel file for file, so a same-version rebuild cannot hide behind the
    # global uv cache.
    print(
        f"harness: wheel {harness.wheel.name} sha256 {harness.wheel_sha256[:12]} · "
        f"per-run uv cache {harness.cache} · installed package verified against "
        "the built wheel",
        file=sys.stderr,
    )
    try:
        harness.execute()
    except Failure as failure:
        print(f"harness: {failure}", file=sys.stderr)
        harness.close()
        return 1
    finally:
        harness.close()
        lock.close()
    return report_result(report, time.monotonic() - started)


def check_inventory(pages: list[PageSpec]) -> list[str]:
    problems: list[str] = []
    expected = public_pages(
        (REPO / "zensical.toml").read_text(encoding="utf-8"), "README.md"
    )
    listed = [page.path for page in pages]
    for missing in sorted(set(expected) - set(listed)):
        problems.append(f"the public page {missing} has no entry")
    for extra in sorted(set(listed) - set(expected)):
        problems.append(f"{extra} is not a public page in zensical.toml")
    for page in pages:
        path = REPO / page.path
        if not path.is_file():
            problems.append(f"{page.path} does not exist")
            continue
        fences = console_fences(page.path, path.read_text(encoding="utf-8"))
        indexes = [spec.index for spec in page.fences]
        if indexes != list(range(1, len(fences) + 1)):
            problems.append(
                f"{page.path}: {len(fences)} console fences but the inventory names "
                f"{indexes}; add or renumber the entries"
            )
            continue
        for spec in page.fences:
            if spec.mode not in MODES:
                problems.append(
                    f"{page.path} fence {spec.index}: unknown mode {spec.mode}"
                )
            if spec.mode in NEEDS_REASON and not spec.reason:
                problems.append(
                    f"{page.path} fence {spec.index}: {spec.mode} needs a written reason"
                )
            if spec.port_fallback is not None and spec.mode not in PORT_FALLBACK_MODES:
                problems.append(
                    f"{page.path} fence {spec.index}: `port_fallback` is only read "
                    f"by {', '.join(sorted(PORT_FALLBACK_MODES))}"
                )

            if (
                spec.fixture is not None
                and not (
                    REPO
                    / "python"
                    / "tests"
                    / "runnable_docs"
                    / "fixtures"
                    / spec.fixture
                ).is_dir()
            ):
                problems.append(
                    f"{page.path} fence {spec.index}: fixture {spec.fixture} does not exist"
                )
    return problems


def report_result(report: Report, seconds: float) -> int:
    executed = 0
    accounted = 0
    for outcome in report.outcomes:
        status = "PASS" if outcome.ok else "FAIL"
        if outcome.executed:
            executed += 1
        else:
            accounted += 1
        print(
            f"{status}  {outcome.page}  fence {outcome.index}  "
            f"{outcome.mode:<18} {outcome.seconds:6.1f}s  {outcome.detail}"
        )
    failed = [outcome for outcome in report.outcomes if not outcome.ok]
    print(
        f"\n{len(report.outcomes)} fences: {executed} executed, {accounted} accounted, "
        f"{len(failed)} failed · {seconds:.1f}s"
    )
    if seconds > 300:
        print(f"\nwarning: the harness took {seconds:.0f}s, over the ~5 minute budget")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
