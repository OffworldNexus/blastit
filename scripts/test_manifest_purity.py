#!/usr/bin/env python3
"""Self-tests for the `std::env` guard (`scripts/check_manifest_purity.py`).

A guard that cannot fail is worse than no guard, so four things are proved on
every `make check`:

1. **A dirty fixture is refused**, naming `file:line`, for both spellings —
   `std::env::var(…)` and `use std::env;`.
2. **A clean fixture passes.**
3. **A tree with no Rust source is refused**, not passed vacuously (a moved
   directory must not turn the rule off).
4. **The repository itself is clean**, which is the actual acceptance.

Run by `make manifest-purity` (stdlib only, no pytest needed).
"""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
GUARD = HERE / "check_manifest_purity.py"

#: The tree the guard covers, relative to a scanned root.
COVERED = Path("crates/blastit-manifest/src")


def load_guard():
    """The guard module, imported by path (it is a script, not a package)."""
    spec = importlib.util.spec_from_file_location("check_manifest_purity", GUARD)
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


def write_source(root: Path, name: str, body: str) -> Path:
    """A fixture source file under the covered tree."""
    path = root / COVERED / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body, encoding="utf-8")
    return path


def check(condition: bool, message: str) -> None:
    """Fails the self-test with one sentence, never a bare traceback."""
    if not condition:
        raise SystemExit(f"manifest-purity self-test: {message}")


def main() -> int:
    """Runs every case; returns the process exit status."""
    guard = load_guard()
    check(guard.NEEDLE == "std::env", "the needle changed; update this self-test")
    check(
        guard.COVERED == COVERED,
        f"the covered tree changed to {guard.COVERED}; update this self-test",
    )

    # 1. A dirty fixture is refused, and the hit names file:line.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        root = Path(scratch)
        body = 'fn main() { let _ = std::env::var("X"); }\n'
        write_source(root, "dirty.rs", body)
        relative = f"{COVERED.as_posix()}/dirty.rs"
        check(
            guard.scan(root) == [(relative, 1, body.strip())],
            f"the scan missed the hit: {guard.scan(root)}",
        )
        result = run_guard(root)
        check(
            result.returncode == 1,
            f"a dirty tree must fail: {result.stdout}{result.stderr}",
        )
        check(f"{relative}:1:" in result.stderr, result.stderr)

        # `use std::env;` is the other spelling, and it is caught too.
        write_source(root, "use.rs", "use std::env;\n")
        check(
            (f"{COVERED.as_posix()}/use.rs", 1, "use std::env;") in guard.scan(root),
            f"the scan missed `use std::env;`: {guard.scan(root)}",
        )

    # 2. A clean fixture passes.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        root = Path(scratch)
        write_source(root, "clean.rs", "fn main() {}\n")
        result = run_guard(root)
        check(result.returncode == 0, f"a clean tree must pass: {result.stderr}")
        check("manifest-purity: ok" in result.stdout, result.stdout)

    # 3. No source at all is refused, never a vacuous pass.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        result = run_guard(Path(scratch))
        check(
            result.returncode == 1,
            f"an empty tree must be refused, not pass: {result.stdout}",
        )

    # 4. The real tree is clean: the actual acceptance.
    result = run_guard(REPO)
    check(result.returncode == 0, f"the repository is not clean: {result.stderr}")
    print(result.stdout.strip())
    print(
        "manifest-purity self-test: ok — 3 fixtures proved, "
        f"{len(guard.covered(REPO))} real source file(s)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
