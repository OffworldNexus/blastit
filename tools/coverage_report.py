#!/usr/bin/env -S uv run --no-project --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Line/region coverage for instrumented Rust tests (`make coverage`).

Builds `cargo test <TESTS> --no-run --locked` with `-C instrument-coverage` into
`target/cov` (never `target/debug`), runs the resulting test executables, merges
their profiles with rustup's `llvm-profdata`, and prints, per file in `FILES`:

- line and region percentages (`llvm-cov report --format=text`);
- the exact uncovered line ranges (`llvm-cov export --format=lcov`).

Non-test `bin` artefacts built for those tests (for example a server that an
integration test spawns) are passed to `llvm-cov` as well, so code reached only
through a spawned binary is measured instead of reading 0%. A test that spawns
that binary with `env_clear()` hides `LLVM_PROFILE_FILE`, so a `0.0%` row prints
a caveat rather than meaning dead code.

Python sources are not measured offline: `uvx --offline coverage` is not cached
and `python/tests` is pytest-only, so `.py` entries are skipped with a stderr
note; audit each one against its `TESTING_PLAN.md` story and E2E test instead.

The first instrumented build populates `target/cov` and is slow once: the whole
dependency graph is rebuilt with instrumentation. Warm runs reuse it and only
rebuild what changed.

Uncovered lines are never an error here. The reviewer decides whether they are
dead code (delete it) or an untested user story (extend the E2E test); the exit
status only fails on a tooling, build or test failure.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

TARGET_DIR = Path("target/cov")
PROFRAW_DIR = TARGET_DIR / "profraw"
PASSED = re.compile(r"(\d+) passed")


class CoverageError(RuntimeError):
    """The instrumented build, test run or report could not complete."""


@dataclass(frozen=True, slots=True)
class Summary:
    """Per-file percentages as printed by `llvm-cov report`."""

    lines: float
    regions: float


def _capture(argv: list[str], env: dict[str, str]) -> str:
    """Run ``argv`` to completion and return stdout, refusing a non-zero exit."""
    done = subprocess.run(argv, env=env, text=True, capture_output=True, check=False)
    if done.returncode != 0:
        tail = (done.stderr or done.stdout).strip().splitlines()[-12:]
        raise CoverageError(f"{argv[0]} exited {done.returncode}: " + " | ".join(tail))
    return done.stdout


def _phase(message: str) -> None:
    """Announce a wait on stderr before it starts (charter CLI rule)."""
    print(f"coverage: {message}", file=sys.stderr, flush=True)


def _plural(count: int, noun: str) -> str:
    return f"{count} {noun}" if count == 1 else f"{count} {noun}s"


def _host(env: dict[str, str]) -> str:
    for line in _capture(["rustc", "-vV"], env).splitlines():
        if line.startswith("host: "):
            return line[len("host: ") :].strip()
    raise CoverageError("rustc -vV reports no host triple")


def _toolchain(env: dict[str, str]) -> tuple[Path, Path]:
    """Return rustup's `llvm-profdata` and `llvm-cov` for the active toolchain."""
    sysroot = Path(_capture(["rustc", "--print", "sysroot"], env).strip())
    bin_dir = sysroot / "lib" / "rustlib" / _host(env) / "bin"
    profdata = bin_dir / "llvm-profdata"
    cov = bin_dir / "llvm-cov"
    for tool in (profdata, cov):
        if not tool.is_file():
            raise CoverageError(
                f"{tool} is missing; run `rustup component add llvm-tools`"
            )
    return profdata, cov


def _build_targets(
    tests: list[str], env: dict[str, str]
) -> tuple[list[Path], list[Path]]:
    """Build the requested targets; return (test executables, bin objects)."""
    output = _capture(
        ["cargo", "test", *tests, "--no-run", "--locked", "--message-format=json"],
        env,
    )
    executables: list[Path] = []
    binaries: list[Path] = []
    for line in output.splitlines():
        if not line.startswith("{"):
            continue
        message = json.loads(line)
        if message.get("reason") != "compiler-artifact":
            continue
        executable = message.get("executable")
        if not executable:
            continue
        profile = message.get("profile") or {}
        if profile.get("test"):
            executables.append(Path(executable))
        elif "bin" in ((message.get("target") or {}).get("kind") or []):
            binaries.append(Path(executable))
    if not executables:
        raise CoverageError("cargo test --no-run built no test executable")
    target = TARGET_DIR.resolve()
    objects = list(dict.fromkeys([*executables, *binaries]))
    for artifact in objects:
        if target not in artifact.resolve().parents:
            raise CoverageError(
                f"{artifact} is outside {TARGET_DIR}: set CARGO_TARGET_DIR"
            )
    return executables, list(dict.fromkeys(binaries))


def _run_tests(executables: list[Path], env: dict[str, str]) -> int:
    """Run every test binary under the profraw sink and count passing tests."""
    passed = 0
    for executable in executables:
        output = _capture([str(executable)], env)
        passed += sum(int(count) for count in PASSED.findall(output))
    return passed


def _merge(profdata_tool: Path, output: Path, env: dict[str, str]) -> None:
    profraws = sorted(PROFRAW_DIR.glob("*.profraw"))
    if not profraws:
        raise CoverageError(f"no .profraw under {PROFRAW_DIR}: did the tests run?")
    argv = [str(profdata_tool), "merge", "-sparse", "-o", str(output)]
    _capture([*argv, *(str(profraw) for profraw in profraws)], env)


def _report(
    cov_tool: Path, profdata: Path, objects: list[Path], env: dict[str, str]
) -> str:
    argv = [
        str(cov_tool),
        "report",
        f"-instr-profile={profdata}",
        *(str(obj) for obj in objects),
    ]
    return _capture(argv, env)


def _lcov(
    cov_tool: Path, profdata: Path, objects: list[Path], env: dict[str, str]
) -> str:
    argv = [
        str(cov_tool),
        "export",
        "-format=lcov",
        f"-instr-profile={profdata}",
        *(str(obj) for obj in objects),
    ]
    return _capture(argv, env)


def _same_file(reported: str, wanted: str) -> bool:
    normalized = reported.replace("\\", "/").removeprefix("./")
    return normalized == wanted or normalized.endswith("/" + wanted)


def _summaries(report: str, files: list[str]) -> dict[str, Summary]:
    """Pick the requested rows out of the aligned `llvm-cov report` text."""
    summaries: dict[str, Summary] = {}
    for line in report.splitlines():
        columns = line.split()
        if len(columns) < 10:
            continue
        for wanted in files:
            if wanted in summaries or not _same_file(columns[0], wanted):
                continue
            try:
                regions = float(columns[3].rstrip("%"))
                coverage = float(columns[9].rstrip("%"))
            except ValueError:
                continue
            summaries[wanted] = Summary(lines=coverage, regions=regions)
    return summaries


def _uncovered(lcov: str, files: list[str]) -> dict[str, list[int]]:
    """Collect `DA:<line>,0` records per requested file from lcov output."""
    hits: dict[str, list[int]] = {wanted: [] for wanted in files}
    current: str | None = None
    for row in lcov.splitlines():
        if row.startswith("SF:"):
            reported = row[len("SF:") :]
            current = next(
                (wanted for wanted in files if _same_file(reported, wanted)), None
            )
        elif row.startswith("DA:") and current is not None:
            number, count = row[len("DA:") :].split(",")[:2]
            if int(count) == 0:
                hits[current].append(int(number))
    return hits


def _compress(lines: list[int]) -> str:
    """Compress line numbers into `12-14, 40` spans."""
    spans: list[tuple[int, int]] = []
    for line in sorted(set(lines)):
        if spans and line == spans[-1][1] + 1:
            spans[-1] = (spans[-1][0], line)
        else:
            spans.append((line, line))
    if not spans:
        return "none"
    return ", ".join(
        f"{start}-{end}" if start != end else str(start) for start, end in spans
    )


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--tests",
        required=True,
        help="cargo test arguments, e.g. '-p blastit-lsp --tests'",
    )
    parser.add_argument(
        "--files",
        required=True,
        help="sources to report, e.g. 'crates/blastit-lsp/src/lib.rs'",
    )
    return parser.parse_args(argv)


def _rust_files(requested: list[str]) -> list[str]:
    """Drop `.py` entries with a loud skip note; keep everything llvm-cov reads."""
    files: list[str] = []
    for wanted in requested:
        if wanted.endswith(".py"):
            print(
                "python: line coverage unavailable offline (uvx --offline coverage "
                "is not cached; python/tests is pytest-only) — audit "
                f"{wanted} against its TESTING_PLAN.md story + E2E test",
                file=sys.stderr,
            )
        else:
            files.append(wanted)
    return files


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)
    tests = shlex.split(args.tests)
    requested = shlex.split(args.files)
    if not tests or not requested:
        raise CoverageError("--tests and --files must both name something")
    files = _rust_files(requested)
    if not files:
        return 0

    env = dict(os.environ)
    env.update(
        {
            "CARGO_TARGET_DIR": str(TARGET_DIR),
            "RUSTFLAGS": "-C instrument-coverage",
            "CARGO_INCREMENTAL": "0",
            "LLVM_PROFILE_FILE": str(PROFRAW_DIR / "%p-%m.profraw"),
            "BLASTIT_COVERAGE": "1",
        }
    )

    for stale in PROFRAW_DIR.glob("*.profraw"):
        stale.unlink()
    PROFRAW_DIR.mkdir(parents=True, exist_ok=True)

    profdata_tool, cov_tool = _toolchain(env)
    _phase(
        "building instrumented test targets into target/cov "
        "(first run rebuilds the dependency graph)…"
    )
    executables, binaries = _build_targets(tests, env)
    _phase(f"running {_plural(len(executables), 'test executable')}…")
    passed = _run_tests(executables, env)
    if passed == 0:
        raise CoverageError("no tests ran: coverage would be meaningless")
    profdata = TARGET_DIR / "coverage.profdata"
    _merge(profdata_tool, profdata, env)

    objects = [*executables, *binaries]
    summaries = _summaries(_report(cov_tool, profdata, objects, env), files)
    uncovered = _uncovered(_lcov(cov_tool, profdata, objects, env), files)
    missing = [wanted for wanted in files if wanted not in summaries]
    if missing:
        raise CoverageError(f"no instrumented coverage for: {', '.join(missing)}")

    extra = f" + {_plural(len(binaries), 'bin object')}" if binaries else ""
    noun = "test" if passed == 1 else "tests"
    print(
        f"coverage: {_plural(len(executables), 'test executable')}{extra}, "
        f"{passed} {noun} passed"
    )
    for wanted in files:
        summary = summaries[wanted]
        print(f"  {wanted}  lines {summary.lines:.1f}%  regions {summary.regions:.1f}%")
        print(f"  uncovered lines: {_compress(uncovered[wanted])}")
        if summary.lines == 0.0:
            print(
                f"coverage: {wanted} reports 0.0% lines — the story test may spawn "
                "the binary with env_clear() so no .profraw is inherited; confirm "
                "its bin object is included before calling anything dead",
                file=sys.stderr,
            )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CoverageError as error:
        print(f"coverage: {error}", file=sys.stderr)
        sys.exit(1)
