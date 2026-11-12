#!/usr/bin/env -S uv run --no-project --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Repository hygiene guard for Blastit.

Inspects staged files plus untracked non-ignored files and refuses commits that break repo rules:
- no file larger than 5 MiB;
- no `.env` or `.env.*` secret file (`.env.example` and `*.env` test/e2e fixtures without secrets are checked);
- no compiled wheels, archives or large binary artefacts outside `tests/golden/` (<= 1 MiB) or WASM codecs under `crates/` (<= 4 MiB).

Run via `make guard` or `uv run --no-project tools/guard.py`.
"""

from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

MAX_FILE_BYTES = 5 * 1024 * 1024
GOLDEN_MAX_FILE_BYTES = 1024 * 1024
CRATE_WASM_MAX_BYTES = 4 * 1024 * 1024

BANNED_EXTENSIONS = (
    ".whl",
    ".tar",
    ".tar.gz",
    ".tgz",
    ".zip",
    ".so",
    ".dylib",
    ".dll",
    ".pyc",
)

GOLDEN_FOLDER = ("tests", "golden")
ALLOWED_ENV_FILES = frozenset({".env.example"})

EXIT_OK = 0
EXIT_VIOLATIONS = 1
EXIT_GUARD_FAILED = 2


class GuardError(RuntimeError):
    """The guard could not inspect the work tree."""


@dataclass(frozen=True, slots=True)
class Violation:
    """One candidate file that breaks a repository hygiene rule."""

    path: str
    reason: str


def _mib(size: int) -> str:
    return f"{size / (1024 * 1024):.1f} MiB"


def _git(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        msg = f"git {' '.join(args)} failed in {root}: {result.stderr.strip()}"
        raise GuardError(msg)
    return result.stdout


def repository_root(start: Path) -> Path:
    return Path(_git(start, "rev-parse", "--show-toplevel").strip())


def staged_and_untracked(root: Path) -> tuple[PurePosixPath, ...]:
    output = _git(
        root, "status", "--porcelain=v1", "-z", "--untracked-files=all", "--no-renames"
    )
    paths: list[PurePosixPath] = []
    for record in output.split("\0"):
        if len(record) < 4:
            continue
        index_status, worktree_status, path = record[0], record[1], record[3:]
        staged = index_status in "AMRC"
        untracked = index_status == "?" and worktree_status == "?"
        if staged or untracked:
            paths.append(PurePosixPath(path))
    return tuple(paths)


def is_secret_env(path: PurePosixPath) -> bool:
    name = path.name
    if name in ALLOWED_ENV_FILES:
        return False
    return name == ".env" or name.startswith(".env.")


def in_golden_folder(path: PurePosixPath) -> bool:
    parts = path.parts
    return any(parts[idx : idx + 2] == GOLDEN_FOLDER for idx in range(len(parts) - 1))


def in_crates_folder(path: PurePosixPath) -> bool:
    return bool(path.parts) and path.parts[0] == "crates"


def banned_extension(path: PurePosixPath) -> str | None:
    lower = path.name.lower()
    for ext in BANNED_EXTENSIONS:
        if lower.endswith(ext):
            return ext
    return None


def check_file(path: PurePosixPath, size: int | None) -> tuple[Violation, ...]:
    violations: list[Violation] = []
    if is_secret_env(path):
        violations.append(
            Violation(str(path), "secret file: commit .env.example instead")
        )
    ext = banned_extension(path)
    if ext is not None:
        if not in_golden_folder(path):
            violations.append(
                Violation(
                    str(path),
                    f"'{ext}' artefact does not belong in git (allowed only in tests/golden/)",
                )
            )
        elif size is not None and size > GOLDEN_MAX_FILE_BYTES:
            violations.append(
                Violation(
                    str(path),
                    f"{_mib(size)} golden file exceeds {_mib(GOLDEN_MAX_FILE_BYTES)} limit",
                )
            )
    if path.name.lower().endswith(".wasm") and size is not None:
        if in_crates_folder(path) and size > CRATE_WASM_MAX_BYTES:
            violations.append(
                Violation(
                    str(path),
                    f"{_mib(size)} WASM module exceeds {_mib(CRATE_WASM_MAX_BYTES)} limit",
                )
            )
        elif not in_crates_folder(path) and not in_golden_folder(path):
            violations.append(
                Violation(
                    str(path),
                    "WASM binaries belong only under crates/ or tests/golden/",
                )
            )
    if size is not None and size > MAX_FILE_BYTES:
        violations.append(
            Violation(
                str(path), f"{_mib(size)} exceeds {_mib(MAX_FILE_BYTES)} hard limit"
            )
        )
    return tuple(violations)


def _file_size(root: Path, path: PurePosixPath) -> int | None:
    absolute = root / path
    try:
        return absolute.stat().st_size if absolute.is_file() else None
    except OSError:
        return None


def inspect(root: Path) -> tuple[tuple[Violation, ...], int]:
    paths = staged_and_untracked(root)
    violations: list[Violation] = []
    for path in paths:
        violations.extend(check_file(path, _file_size(root, path)))
    return tuple(violations), len(paths)


def main() -> int:
    try:
        root = repository_root(Path.cwd())
        violations, checked = inspect(root)
    except GuardError as error:
        print(f"guard: cannot inspect work tree: {error}", file=sys.stderr)
        return EXIT_GUARD_FAILED
    if violations:
        lines = [f"guard: {len(violations)} violation(s); do not commit these files:"]
        lines.extend(f"  X {v.path}: {v.reason}" for v in violations)
        lines.append("See skill git-workflow for repository hygiene rules.")
        print("\n".join(lines), file=sys.stderr)
        return EXIT_VIOLATIONS
    print(f"guard: OK ({checked} staged/untracked file(s) checked)")
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())
