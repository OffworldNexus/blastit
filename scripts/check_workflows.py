#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = ["pyyaml==6.0.3"]
# ///
"""Checks the GitHub workflows and composite actions as an execution graph (MB.3.18).

    UV_OFFLINE=1 uv run --no-project --locked scripts/check_workflows.py [--repo DIR] [--quiet]

`pyyaml==6.0.3` is pinned in this script's PEP 723 header and hash-locked in
`scripts/check_workflows.py.lock` so `make workflow-check` runs completely
offline (`UV_OFFLINE=1 --locked`). `scripts/test_packaging_gates.py` (also in
`make check`) mutates throwaway copies to prove every failure class stays caught.

What it checks across `.github/workflows/*.yml` and `.github/actions/*/action.yml`:

1.  every YAML file parses into a mapping;
2.  every `needs:` entry names an existing job in that workflow, with no cycle,
    and in `publish-pypi.yml` no build/verify job is orphaned outside the
    publish sinks (`testpypi`, `release`);
3.  every `needs.<id>` expression avoids hyphens and names a direct `needs` entry;
4.  every `needs.<id>.outputs.<key>` names an output that job declares;
5.  every job using `${{ matrix.<k> }}` has a non-empty `strategy.matrix`, every
    referenced key exists in *every* matrix entry (resolving `fromJSON(...)` via
    `scripts/platform_matrix.py`), and no `cp${{ matrix.python_tag }}` double
    prefix appears;
6.  if a job's matrix entries carry multi-arch `runs_on`, the job uses
    `runs-on: ${{ matrix.runs_on }}` (never a hardcoded x86_64 runner), and no
    job combines `container: alpine*` with an `*-arm` runner;
7.  every `platform_matrix.py --leg <family>` call in a `run:` block passes all
    disambiguating flags (`--minor` for `shims`, `--python` for `wheels`, plus
    `--libc` and `--arch`);
8.  every non-diagnostic (`!failure()`, `!always()`) `actions/upload-artifact`
    step in workflows and actions sets `if-no-files-found: error`, and no two
    jobs in a workflow upload the same literal artifact name;
9.  every `actions/download-artifact` step (by `name`, `pattern`, or unfiltered)
    matches at least one upload in *that same workflow*, and **every** matching
    producer job in that workflow is in the consumer's transitive `needs`;
10. for every `download-artifact` with `merge-multiple: true`, expanding matched
    producers across their concrete matrix entries produces no colliding literal
    file basenames (such as two legs both uploading `dist/SHA256SUMS`);
11. no `#` comment line sits inside a `\\` line continuation in any workflow
    step, composite action `runs.steps` step, or `before-script-linux` script;
12. every `inputs.<k>` referenced inside a composite action is declared, and
    every `./.github/actions/<name>` call site passes valid, complete inputs.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

import yaml

EXPR_MATRIX = re.compile(r"\$\{\{[^}]*?\bmatrix\.([A-Za-z0-9_-]+)")
EXPR_MATRIX_SUB = re.compile(r"\$\{\{\s*matrix\.([A-Za-z0-9_-]+)\s*\}\}")
DOUBLE_CP_TAG = re.compile(r"cp\$\{\{\s*matrix\.python_tag\s*\}\}")
PY_TAG_VAR = re.compile(
    r"\b([A-Za-z_][A-Za-z0-9_]*)=['\"]?\$\{\{\s*matrix\.python_tag\s*\}\}"
)
EXPR_NEEDS = re.compile(r"\bneeds\.([A-Za-z0-9_-]+)")
EXPR_OUTPUT = re.compile(r"\bneeds\.([A-Za-z0-9_-]+)\.outputs\.([A-Za-z0-9_-]+)")
FROM_JSON = re.compile(r"fromJSON\(needs\.([\w-]+)\.outputs\.(\w+)\)")
TEMPLATE = re.compile(r"\$\{\{[^}]*\}\}")
LEG_CALL = re.compile(r"platform_matrix\.py\b([^\n;|`]*)")

GENERATOR = "scripts/platform_matrix.py"
OUTPUT_SPECS: dict[str, tuple[str, str]] = {
    "wheels": ("wheels", ""),
    "shims": ("shims", ""),
    "binaries": ("binaries", ""),
    "legs": ("shims", ""),
    "wheels_gnu": ("wheels", "gnu"),
    "wheels_musl": ("wheels", "musl"),
    "binaries_gnu": ("binaries", "gnu"),
    "binaries_musl": ("binaries", "musl"),
}
PUBLISH_SINKS = frozenset({"testpypi", "release"})


@dataclass(frozen=True, slots=True)
class UploadSpec:
    job_name: str
    artifact_pattern: str
    raw_artifact: str
    raw_paths: tuple[str, ...]
    matrix_entries: tuple[dict[str, str], ...]


class Report:
    def __init__(self, quiet: bool) -> None:
        self.problems: list[str] = []
        self.checks = 0
        self.quiet = quiet

    def check(self, ok: bool, message: str) -> None:
        self.checks += 1
        if not ok:
            self.problems.append(message)

    def fail(self, message: str) -> None:
        self.checks += 1
        self.problems.append(message)


def matrix_entries(
    job: dict, repo: Path, report: Report
) -> list[dict[str, str]] | None:
    """Concrete matrix entries for `job`, or `None` when the job has no matrix."""
    strategy = job.get("strategy") or {}
    matrix = strategy.get("matrix")
    if matrix is None:
        return None
    if isinstance(matrix, dict):
        includes = matrix.get("include")
        if isinstance(includes, list) and includes:
            return [
                {str(k): str(v) for k, v in item.items()}
                for item in includes
                if isinstance(item, dict)
            ]
        axes = {
            k: v
            for k, v in matrix.items()
            if k not in {"include", "exclude"} and isinstance(v, list)
        }
        if not axes:
            return [{}]
        combos: list[dict[str, str]] = [{}]
        for key, values in axes.items():
            combos = [{**base, str(key): str(val)} for base in combos for val in values]
        return combos

    match = FROM_JSON.search(str(matrix))
    if not match:
        report.fail(f"unrecognised strategy.matrix: {str(matrix)[:80]!r}")
        return []
    spec = OUTPUT_SPECS.get(match.group(2))
    if spec is None:
        report.fail(
            f"strategy.matrix reads needs.{match.group(1)}.outputs.{match.group(2)}, unknown family"
        )
        return []
    family, libc = spec
    cmd = [sys.executable, str(repo / GENERATOR), "--matrix", family]
    if libc:
        cmd.extend(["--libc", libc])
    result = subprocess.run(
        cmd,
        cwd=repo,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        report.fail(
            f"the matrix generator failed for {family}: {(result.stderr or '').strip()[:200]}"
        )
        return []
    entries = json.loads(result.stdout).get("include") or []
    report.check(
        bool(entries),
        f"strategy.matrix for {family} ({libc or 'all'}) is empty: a job that runs no legs proves nothing",
    )
    return [{str(k): str(v) for k, v in entry.items()} for entry in entries]


def continuations(run_text: str) -> list[str]:
    """Lines of a shell block that are a `#` comment inside a `\\` continuation."""
    bad: list[str] = []
    continued = False
    for line in run_text.splitlines():
        stripped = line.strip()
        if continued and stripped.startswith("#"):
            bad.append(stripped)
        continued = stripped.endswith("\\")
    return bad


def join_shell_continuations(run_text: str) -> str:
    """Fold `\\`-continued lines so multi-line CLI invocations can be inspected."""
    out: list[str] = []
    buf = ""
    for line in run_text.splitlines():
        stripped = line.rstrip()
        if stripped.lstrip().startswith("#"):
            if buf:
                out.append(buf)
                buf = ""
            continue
        if stripped.endswith("\\"):
            buf += stripped[:-1] + " "
        else:
            buf += stripped
            out.append(buf)
            buf = ""
    if buf:
        out.append(buf)
    return "\n".join(out)


def check_platform_matrix_calls(run_text: str, scope: str, report: Report) -> None:
    folded = join_shell_continuations(run_text)
    for match in LEG_CALL.finditer(folded):
        tail = match.group(1)
        leg_match = re.search(r"--leg\s+(\S+)", tail)
        if leg_match is None:
            continue
        family = leg_match.group(1).strip("'\"")
        if family not in {"shims", "binaries", "wheels"}:
            report.fail(
                f"{scope}: platform_matrix.py --leg {family!r} names an unknown family"
            )
            continue
        required = ["--libc", "--arch"]
        if family == "shims":
            required.insert(0, "--minor")
        elif family == "wheels":
            required.insert(0, "--python")
        for flag in required:
            report.check(
                re.search(rf"{re.escape(flag)}\b", tail) is not None,
                f"{scope}: `platform_matrix.py --leg {family}` omits `{flag}`, "
                f"which matches multiple legs and exits 2 under `set -e` (use `--count {family}` for counts)",
            )


def is_diagnostic_condition(if_expr: object) -> bool:
    text = str(if_expr or "")
    return "failure()" in text or "always()" in text


def split_upload_paths(raw_path: object) -> tuple[str, ...]:
    if not isinstance(raw_path, str):
        return ()
    return tuple(
        line.strip()
        for line in raw_path.splitlines()
        if line.strip() and not line.strip().startswith("!")
    )


def expand_matrix_text(text: str, entry: dict[str, str]) -> str:
    return EXPR_MATRIX_SUB.sub(lambda m: entry.get(m.group(1), "*"), text)


def expand_action_inputs(text: str, call_with: dict[str, object]) -> str:
    def _sub(match: re.Match[str]) -> str:
        key = match.group(1)
        return str(call_with.get(key, "*"))

    return re.sub(r"\$\{\{\s*inputs\.([A-Za-z0-9_-]+)\s*\}\}", _sub, text)


def artifact_matches(upload_pattern: str, download_glob: str) -> bool:
    return fnmatch.fnmatchcase(upload_pattern, download_glob) or fnmatch.fnmatchcase(
        download_glob, upload_pattern
    )


def transitive_needs(job_name: str, jobs_needing: dict[str, list[str]]) -> set[str]:
    """Every job `job_name` depends on, directly or through another job."""
    seen: set[str] = set()
    stack = list(jobs_needing.get(job_name, []))
    while stack:
        need = stack.pop()
        if need in seen:
            continue
        seen.add(need)
        stack.extend(jobs_needing.get(need, []))
    return seen


def has_cycle(jobs_needing: dict[str, list[str]]) -> list[str]:
    visiting: set[str] = set()
    visited: set[str] = set()
    cycles: list[str] = []

    def visit(node: str) -> None:
        if node in visited:
            return
        if node in visiting:
            cycles.append(node)
            return
        visiting.add(node)
        for dep in jobs_needing.get(node, []):
            if dep in jobs_needing:
                visit(dep)
        visiting.remove(node)
        visited.add(node)

    for job_name in jobs_needing:
        visit(job_name)
    return cycles


def has_double_cp_prefix(text: str) -> bool:
    if DOUBLE_CP_TAG.search(text) is not None:
        return True
    for var_name in set(PY_TAG_VAR.findall(text)):
        if re.search(
            rf"cp(?:['\"]?\$\{{?{re.escape(var_name)}\}}?)",
            text,
        ):
            return True
    return False


def check_step_scripts(step: dict, scope: str, report: Report) -> None:
    run = step.get("run")
    if isinstance(run, str):
        for line in continuations(run):
            report.fail(
                f"{scope}: a `#` comment inside a `\\` continuation "
                f"({line[:60]!r}) ends the command there"
            )
        check_platform_matrix_calls(run, scope, report)
        env_map = step.get("env") or {}
        combined = run
        if isinstance(env_map, dict) and env_map:
            combined = "\n".join(f"{k}={v}" for k, v in env_map.items()) + "\n" + run
        report.check(
            not has_double_cp_prefix(combined),
            f"{scope}: `cp${{{{ matrix.python_tag }}}}` (or `cp$PY_TAG`) doubles the `cp` prefix (`python_tag` is already `cp3NN`)",
        )
    with_ = step.get("with") or {}
    before_linux = with_.get("before-script-linux")
    if isinstance(before_linux, str):
        for line in continuations(before_linux):
            report.fail(
                f"{scope} (before-script-linux): a `#` comment inside a `\\` continuation "
                f"({line[:60]!r}) ends the command there"
            )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", default=".", help="repository root")
    parser.add_argument(
        "--quiet", action="store_true", help="only problems and the summary"
    )
    args = parser.parse_args(argv)

    repo = Path(args.repo).resolve()
    report = Report(args.quiet)
    workflows = sorted(repo.glob(".github/workflows/*.yml"))
    actions = sorted(repo.glob(".github/actions/*/action.yml"))

    documents: dict[Path, dict] = {}
    for path in workflows + actions:
        try:
            document = yaml.safe_load(path.read_text(encoding="utf-8"))
        except yaml.YAMLError as error:
            report.fail(f"{path.name}: does not parse: {str(error)[:200]}")
            continue
        report.check(
            isinstance(document, dict), f"{path.name}: top level is not a mapping"
        )
        if isinstance(document, dict):
            documents[path] = document

    # Index composite actions first so workflow call sites can be validated and
    # their inner `upload-artifact` steps inherit caller matrix entries.
    action_by_ref: dict[str, dict] = {}
    for path in actions:
        document = documents.get(path)
        if document is None:
            continue
        action_name = path.parent.name
        ref = f"./.github/actions/{action_name}"
        action_by_ref[ref] = document
        declared_inputs = document.get("inputs") or {}
        declared_names = set(declared_inputs.keys())
        steps = (document.get("runs") or {}).get("steps") or []
        report.check(bool(steps), f"{action_name}/action.yml: runs.steps is empty")
        for idx, step in enumerate(steps, 1):
            scope = f"{action_name}/action.yml: step {idx} ({step.get('name') or step.get('uses') or 'run'})"
            check_step_scripts(step, scope, report)
            if "upload-artifact" in str(
                step.get("uses", "")
            ) and not is_diagnostic_condition(step.get("if")):
                with_ = step.get("with") or {}
                report.check(
                    with_.get("if-no-files-found") == "error",
                    f"{action_name}/action.yml: upload-artifact must set `if-no-files-found: error`",
                )
            for key in set(
                re.findall(r"inputs\.([A-Za-z0-9_-]+)", yaml.safe_dump(step))
            ):
                report.check(
                    key in declared_names,
                    f"{action_name}/action.yml: uses inputs.{key}, undeclared ({sorted(declared_names)})",
                )

    for path in workflows:
        document = documents.get(path)
        if document is None:
            continue
        jobs = document.get("jobs") or {}
        if not jobs:
            report.fail(f"{path.name}: no jobs")
            continue

        job_matrices: dict[str, list[dict[str, str]] | None] = {}
        uploads: list[UploadSpec] = []
        jobs_needing: dict[str, list[str]] = {}

        # Pass 1: resolve matrices, direct `needs`, and per-workflow artifact uploads.
        for job_name, job in jobs.items():
            needs = job.get("needs") or []
            needs_list = [needs] if isinstance(needs, str) else list(needs)
            jobs_needing[job_name] = [str(item) for item in needs_list]
            entries = matrix_entries(job, repo, report)
            job_matrices[job_name] = entries
            concrete_entries = tuple(entries) if entries is not None else ({},)

            for step in job.get("steps") or []:
                uses = str(step.get("uses", ""))
                with_ = step.get("with") or {}
                if "upload-artifact" in uses:
                    raw_name = str(with_.get("name", ""))
                    if not is_diagnostic_condition(step.get("if")):
                        report.check(
                            with_.get("if-no-files-found") == "error",
                            f"{path.name}: job {job_name}: upload-artifact {raw_name!r} "
                            "must set `if-no-files-found: error`",
                        )
                    uploads.append(
                        UploadSpec(
                            job_name=job_name,
                            artifact_pattern=TEMPLATE.sub("*", raw_name),
                            raw_artifact=raw_name,
                            raw_paths=split_upload_paths(with_.get("path")),
                            matrix_entries=concrete_entries,
                        )
                    )
                elif uses in action_by_ref:
                    action_doc = action_by_ref[uses]
                    declared_inputs = action_doc.get("inputs") or {}
                    for passed_key in with_:
                        report.check(
                            passed_key in declared_inputs,
                            f"{path.name}: job {job_name} passes unknown input {passed_key!r} to {uses}",
                        )
                    for req_key, spec in declared_inputs.items():
                        if (
                            isinstance(spec, dict)
                            and spec.get("required")
                            and "default" not in spec
                        ):
                            report.check(
                                req_key in with_,
                                f"{path.name}: job {job_name} omits required input {req_key!r} on {uses}",
                            )
                    for a_step in (action_doc.get("runs") or {}).get("steps") or []:
                        if "upload-artifact" not in str(a_step.get("uses", "")):
                            continue
                        a_with = a_step.get("with") or {}
                        raw_name = expand_action_inputs(
                            str(a_with.get("name", "")), with_
                        )
                        raw_paths = tuple(
                            expand_action_inputs(p, with_)
                            for p in split_upload_paths(a_with.get("path"))
                        )
                        uploads.append(
                            UploadSpec(
                                job_name=job_name,
                                artifact_pattern=TEMPLATE.sub("*", raw_name),
                                raw_artifact=raw_name,
                                raw_paths=raw_paths,
                                matrix_entries=concrete_entries,
                            )
                        )

        cycles = has_cycle(jobs_needing)
        report.check(
            not cycles,
            f"{path.name}: cycle in job `needs:` graph involving {sorted(set(cycles))}",
        )

        if path.name == "publish-pypi.yml":
            depended_on = {dep for deps in jobs_needing.values() for dep in deps}
            for job_name in jobs:
                if job_name in PUBLISH_SINKS:
                    continue
                report.check(
                    job_name in depended_on,
                    f"{path.name}: job {job_name!r} is orphaned (no downstream job depends on it, "
                    "so a failure cannot block publishing)",
                )

        # Duplicate literal upload names in the same workflow.
        literal_uploads = [u for u in uploads if "*" not in u.artifact_pattern]
        for spec in literal_uploads:
            count = sum(
                len(other.matrix_entries)
                for other in literal_uploads
                if other.artifact_pattern == spec.artifact_pattern
            )
            report.check(
                count == 1,
                f"{path.name}: literal artifact {spec.artifact_pattern!r} is uploaded {count} times "
                "(`upload-artifact@v4` refuses duplicate names)",
            )

        # Pass 2: validate each job's expressions, steps, runner/container rules, and downloads.
        for job_name, job in jobs.items():
            needs = jobs_needing[job_name]
            for need in needs:
                report.check(
                    need in jobs,
                    f"{path.name}: job {job_name} needs {need!r}, which is not a job in this file",
                )

            body = yaml.safe_dump(job)
            for referenced in set(EXPR_NEEDS.findall(body)):
                if "-" in referenced:
                    report.fail(
                        f"{path.name}: job {job_name} references needs.{referenced} — use an underscore job id"
                    )
                report.check(
                    referenced in needs,
                    f"{path.name}: job {job_name} references needs.{referenced} but does not need it",
                )
            for referenced, output in set(EXPR_OUTPUT.findall(body)):
                declared = jobs.get(referenced, {}).get("outputs") or {}
                report.check(
                    output in declared,
                    f"{path.name}: job {job_name} reads needs.{referenced}.outputs.{output}, "
                    f"which {referenced} does not declare ({sorted(declared)})",
                )

            entries = job_matrices[job_name]
            used = {key for key in EXPR_MATRIX.findall(body) if key != "outputs"}
            if used:
                report.check(
                    entries is not None,
                    f"{path.name}: job {job_name} uses ${{{{ matrix.{min(used)} }}}} but has no strategy.matrix",
                )
                if entries:
                    common_keys = set(entries[0]).intersection(
                        *(set(e) for e in entries)
                    )
                    for key in sorted(used):
                        report.check(
                            key in common_keys,
                            f"{path.name}: job {job_name} uses matrix.{key}, which its matrix "
                            f"does not provide on every leg ({sorted(common_keys)})",
                        )

            if entries:
                matrix_runners = {e["runs_on"] for e in entries if "runs_on" in e}
                runs_on_raw = str(job.get("runs-on", ""))
                if len(matrix_runners) > 1:
                    report.check(
                        "matrix.runs_on" in runs_on_raw,
                        f"{path.name}: job {job_name} has multi-arch matrix runners {sorted(matrix_runners)} "
                        f"but hardcodes runs-on={runs_on_raw!r} instead of `${{{{ matrix.runs_on }}}}`",
                    )
                container_spec = job.get("container")
                container_image = (
                    container_spec.get("image", "")
                    if isinstance(container_spec, dict)
                    else str(container_spec or "")
                )
                if "alpine" in container_image and any(
                    "arm" in r for r in matrix_runners
                ):
                    report.fail(
                        f"{path.name}: job {job_name} uses job-level `container: {container_image}` "
                        "on an arm64 runner (`externals/node20_alpine` is absent on linux-arm64); "
                        "use `docker run alpine:...` on the runner instead"
                    )

            upstream = transitive_needs(job_name, jobs_needing)
            for idx, step in enumerate(job.get("steps") or [], 1):
                scope = f"{path.name}: job {job_name} step {idx}"
                check_step_scripts(step, scope, report)

                if "download-artifact" not in str(step.get("uses", "")):
                    continue
                with_ = step.get("with") or {}
                dl_name = with_.get("name")
                dl_pattern = with_.get("pattern")
                merge_multiple = bool(with_.get("merge-multiple"))

                if dl_name is not None:
                    glob_pat = TEMPLATE.sub("*", str(dl_name))
                    matched = [
                        u
                        for u in uploads
                        if artifact_matches(u.artifact_pattern, glob_pat)
                    ]
                    label = f"artifact {glob_pat!r}"
                elif dl_pattern is not None:
                    glob_pat = TEMPLATE.sub("*", str(dl_pattern))
                    matched = [
                        u
                        for u in uploads
                        if artifact_matches(u.artifact_pattern, glob_pat)
                    ]
                    label = f"pattern {dl_pattern!r}"
                else:
                    matched = list(uploads)
                    label = "all workflow artifacts (unfiltered download-artifact)"

                report.check(
                    bool(matched),
                    f"{path.name}: job {job_name} downloads {label}, which matches no upload in {path.name}",
                )
                if matched:
                    produced_by = {u.job_name for u in matched}
                    missing_needs = sorted(produced_by - upstream)
                    report.check(
                        not missing_needs,
                        f"{path.name}: job {job_name} downloads {label}, produced by "
                        f"{sorted(produced_by)}, missing from its transitive needs: {missing_needs}",
                    )

                if merge_multiple and matched:
                    # Expand every matched producer's uploaded paths across its matrix
                    # entries: if two concrete uploads write the same non-glob file
                    # basename, `merge-multiple: true` silently overwrites one.
                    seen_basenames: dict[str, list[str]] = {}
                    for spec in matched:
                        for entry in spec.matrix_entries:
                            expanded_art = expand_matrix_text(spec.raw_artifact, entry)
                            if dl_name is not None:
                                # A per-leg consumer (`standalone-${{ matrix.name }}`)
                                # only downloads its own leg unless it uses a wildcard.
                                consumer_pat = TEMPLATE.sub("*", str(dl_name))
                                if not fnmatch.fnmatchcase(expanded_art, consumer_pat):
                                    continue
                            elif dl_pattern is not None:
                                consumer_pat = TEMPLATE.sub("*", str(dl_pattern))
                                if not fnmatch.fnmatchcase(expanded_art, consumer_pat):
                                    continue
                            for raw_p in spec.raw_paths:
                                expanded_p = expand_matrix_text(raw_p, entry)
                                base = PurePosixPath(expanded_p).name
                                if (
                                    not base
                                    or "*" in base
                                    or "?" in base
                                    or "[" in base
                                ):
                                    continue
                                origin = f"{spec.job_name}({expanded_art}:{expanded_p})"
                                seen_basenames.setdefault(base, []).append(origin)
                    for base, origins in sorted(seen_basenames.items()):
                        report.check(
                            len(origins) == 1,
                            f"{path.name}: job {job_name} merges {label} with `merge-multiple: true`, "
                            f"and file basename {base!r} collides across {origins}",
                        )

    if not report.quiet:
        for problem in report.problems:
            print(f"FAIL {problem}")
    print(
        f"workflow-check: {'FAIL' if report.problems else 'ok'} — {report.checks} checks over "
        f"{len(workflows)} workflows and {len(actions)} actions, {len(report.problems)} problem(s)"
    )
    return 1 if report.problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
