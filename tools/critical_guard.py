#!/usr/bin/env -S uv run --no-project --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Guard the protected critical-test tiers in Blastit.

`python/tests/critical/**` (or any `**/tests/critical/**`) and `crates/*/tests/critical_*.rs`
hold tests that protect security invariants, wire-protocol contracts, DX promises, and CVE
prevention guarantees. Modifying or deleting them is allowed ONLY when the plan node doing it
explicitly lists a covering glob in `paths` of its `NODE.md`. Adding new critical tests is always
allowed.

Usage (reviewer verifying a node):
    uv run --no-project tools/critical_guard.py --base <commit-before-node> --node MA.3
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Iterable, Sequence

EXIT_OK: Final = 0
EXIT_VIOLATIONS: Final = 1
EXIT_GUARD_FAILED: Final = 2

PROTECTED_DIR: Final = ("tests", "critical")
PROTECTED_RUST_PREFIX: Final = "critical_"
BREAKING_STATUSES: Final = frozenset({"M", "D", "T"})


class GuardError(RuntimeError):
    """The guard could not inspect the tree."""


@dataclass(frozen=True, slots=True)
class Change:
    status: str
    path: str

    @property
    def is_breaking(self) -> bool:
        return self.status in BREAKING_STATUSES

    @property
    def is_addition(self) -> bool:
        return self.status == "A"


@dataclass(frozen=True, slots=True)
class Finding:
    change: Change
    pattern: str | None

    @property
    def is_violation(self) -> bool:
        return self.change.is_breaking and self.pattern is None

    @property
    def is_addition(self) -> bool:
        return self.change.is_addition


def glob_to_regex(pattern: str) -> re.Pattern[str]:
    out: list[str] = ["^"]
    index = 0
    while index < len(pattern):
        if pattern.startswith("**/", index):
            out.append("(?:.*/)?")
            index += 3
        elif pattern.startswith("**", index):
            out.append(".*")
            index += 2
        elif pattern[index] == "*":
            out.append("[^/]*")
            index += 1
        elif pattern[index] == "?":
            out.append("[^/]")
            index += 1
        else:
            out.append(re.escape(pattern[index]))
            index += 1
    out.append("$")
    return re.compile("".join(out))


def is_protected(path: str) -> bool:
    parts = PurePosixPath(path).parts
    for index in range(len(parts) - 1):
        if (parts[index], parts[index + 1]) == PROTECTED_DIR:
            return True
    if "tests" not in parts[:-1]:
        return False
    name = parts[-1] if parts else ""
    return name.startswith(PROTECTED_RUST_PREFIX) and name.endswith(".rs")


def authorizing_pattern(path: str, patterns: Iterable[str]) -> str | None:
    for pattern in patterns:
        if glob_to_regex(pattern).match(path):
            return pattern
    return None


def _unquote(value: str) -> str:
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        return value[1:-1]
    return value


def parse_node_paths(node_file: Path) -> list[str]:
    lines = node_file.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0].strip() != "---":
        msg = f"{node_file}: no YAML frontmatter"
        raise GuardError(msg)
    paths: list[str] = []
    in_paths = False
    for line in lines[1:]:
        stripped = line.strip()
        if stripped == "---":
            break
        if in_paths and stripped.startswith("- "):
            paths.append(_unquote(stripped[2:].strip()))
            continue
        if in_paths and stripped:
            in_paths = False
        if stripped.startswith("paths:"):
            in_paths = True
            inline = stripped[len("paths:") :].strip()
            if inline.startswith("[") and inline.endswith("]"):
                paths.extend(
                    _unquote(item.strip())
                    for item in inline[1:-1].split(",")
                    if item.strip()
                )
    return paths


def find_node_file(repo: Path, node_id: str) -> Path:
    matches = sorted(
        path
        for path in (repo / "plan" / "nodes").rglob("NODE.md")
        if path.parent.name == node_id
    )
    if not matches:
        msg = f"no plan node named {node_id!r} under {repo / 'plan' / 'nodes'}"
        raise GuardError(msg)
    if len(matches) > 1:
        listing = ", ".join(str(path) for path in matches)
        msg = f"plan node id {node_id!r} is ambiguous: {listing}"
        raise GuardError(msg)
    return matches[0]


def changed_files(repo: Path, base: str) -> list[Change]:
    completed = subprocess.run(
        [
            "git",
            "-C",
            str(repo),
            "diff",
            "--no-renames",
            "--name-status",
            "--no-ext-diff",
            base,
            "--",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        msg = f"git diff {base} failed ({completed.returncode}): {completed.stderr.strip()}"
        raise GuardError(msg)
    changes: list[Change] = []
    for line in completed.stdout.splitlines():
        if not line.strip():
            continue
        status, _, path = line.partition("\t")
        changes.append(Change(status=status[:1], path=path))
    return changes


def evaluate(changes: Iterable[Change], patterns: Iterable[str]) -> list[Finding]:
    covered = list(patterns)
    return [
        Finding(change=change, pattern=authorizing_pattern(change.path, covered))
        for change in changes
        if is_protected(change.path)
    ]


def _describe(finding: Finding) -> str:
    status = finding.change.status
    if finding.is_addition:
        verdict = "added (always allowed)"
    elif finding.pattern is not None:
        verdict = f"authorized by {finding.pattern}"
    else:
        verdict = "VIOLATION: not covered by any node path"
    return f"  {status}  {finding.change.path}  [{verdict}]"


def _run(args: argparse.Namespace) -> int:
    repo = args.repo.resolve()
    changes = changed_files(repo, args.base)
    nodes: list[str] = list(args.node or [])
    patterns: list[str] = []
    node_patterns: list[tuple[str, list[str]]] = []
    for node_id in nodes:
        node_file = find_node_file(repo, node_id)
        parsed = parse_node_paths(node_file)
        node_patterns.append((node_id, parsed))
        patterns.extend(parsed)
    findings = evaluate(changes, patterns)

    print(f"critical-guard: {args.base}..working tree, {len(changes)} changed file(s)")
    for node_id, parsed in node_patterns:
        print(f"node {node_id}: paths {parsed or '(none)'}")
    if not findings:
        print("  no change to a protected critical test")
        return EXIT_OK
    for finding in findings:
        print(_describe(finding))

    violations = [finding for finding in findings if finding.is_violation]
    if not nodes or args.report:
        if violations:
            print(
                f"  report only: {len(violations)} breaking change(s) would fail an enforced run"
            )
        return EXIT_OK
    label = ", ".join(nodes)
    if violations:
        print(
            f"critical-guard: FAILED: {len(violations)} protected critical test(s) changed or deleted "
            f"without a covering path in node(s) {label}. Add the file to the node paths, or report "
            "STATUS: blocked and let the lead send it to the test-curator."
        )
        return EXIT_VIOLATIONS
    print(
        f"critical-guard: OK: all {len(findings)} protected change(s) are authorized by node(s) {label}"
    )
    return EXIT_OK


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Refuse unauthorized changes to critical tests."
    )
    parser.add_argument(
        "--base", required=True, help="git revision to compare the working tree against"
    )
    parser.add_argument(
        "--node",
        action="append",
        default=None,
        help="plan node whose NODE.md paths authorize the changes (repeatable for multi-node release PRs)",
    )
    parser.add_argument(
        "--repo",
        default=Path(),
        type=Path,
        help="repository root (default: current directory)",
    )
    parser.add_argument(
        "--report", action="store_true", help="only report, even when --node is given"
    )
    args = parser.parse_args(argv)
    try:
        return _run(args)
    except GuardError as error:
        print(f"critical-guard: {error}", file=sys.stderr)
        return EXIT_GUARD_FAILED


if __name__ == "__main__":
    sys.exit(main())
