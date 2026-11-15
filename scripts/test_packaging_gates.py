#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Self-tests (positive + mutated negatives) for the three packaging gates (MB.3.18).

Runs in `make check` (`make workflow-check` / `make platform-matrix-check`) on
throwaway temporary copies — never mutating the repository — so none of the
workflow, matrix, or wheel-tag guards can silently go vacuous.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CHECK_WORKFLOWS = REPO / "scripts" / "check_workflows.py"
PLATFORM_MATRIX = REPO / "scripts" / "platform_matrix.py"
CHECK_WHEEL_TAG = REPO / "scripts" / "check_wheel_tag.py"


def _copy_workflow_fixture(dst: Path) -> Path:
    root = dst / "repo"
    shutil.copytree(REPO / ".github", root / ".github")
    (root / "scripts").mkdir(parents=True)
    shutil.copy2(PLATFORM_MATRIX, root / "scripts" / "platform_matrix.py")
    return root


def _copy_matrix_fixture(dst: Path) -> Path:
    root = dst / "repo"
    (root / "scripts").mkdir(parents=True)
    shutil.copy2(PLATFORM_MATRIX, root / "scripts" / "platform_matrix.py")
    shutil.copy2(REPO / "README.md", root / "README.md")
    shutil.copytree(REPO / "docs", root / "docs")
    return root


def _run_auditor(fixture_repo: Path) -> subprocess.CompletedProcess[str]:
    env = {**os.environ, "UV_OFFLINE": "1"}
    return subprocess.run(
        [
            "uv",
            "run",
            "--no-project",
            "--locked",
            str(CHECK_WORKFLOWS),
            "--repo",
            str(fixture_repo),
        ],
        capture_output=True,
        text=True,
        env=env,
        check=False,
    )


def _run_matrix(fixture_repo: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(fixture_repo / "scripts" / "platform_matrix.py"), *args],
        capture_output=True,
        text=True,
        check=False,
    )


def _write_test_wheel(directory: Path, filename: str, tag: str) -> Path:
    path = directory / filename
    wheel_meta = (
        "Wheel-Version: 1.0\n"
        "Generator: maturin (1.16.0)\n"
        "Root-Is-Purelib: false\n"
        f"Tag: {tag}\n"
    )
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("blastit-0.2.0.dist-info/WHEEL", wheel_meta)
    return path


def _expect(condition: bool, message: str, detail: str = "") -> None:
    if not condition:
        suffix = f"\n{detail}" if detail else ""
        raise AssertionError(f"{message}{suffix}")


def test_workflow_auditor_cases() -> int:
    cases_run = 0
    with tempfile.TemporaryDirectory(prefix="blastit-wf-gate-") as tmp:
        base = _copy_workflow_fixture(Path(tmp))

        # 1. Clean baseline passes.
        clean = _run_auditor(base)
        _expect(
            clean.returncode == 0,
            "clean workflow tree must pass",
            clean.stdout + clean.stderr,
        )
        cases_run += 1

        # 2. Attempt-3 BLOCKER: `--leg shims` without `--minor`.
        case_dir = Path(tmp) / "leg-no-minor"
        shutil.copytree(base, case_dir)
        pub = case_dir / ".github" / "workflows" / "publish-pypi.yml"
        pub.write_text(
            pub.read_text(encoding="utf-8").replace(
                "--count shims \\\n              --libc gnu",
                "--leg shims \\\n              --libc gnu",
                1,
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "omits `--minor`" in res.stdout,
            "auditor must reject `--leg shims` without `--minor`",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 3. Attempt-3 MAJOR 1 / Attempt-2 MAJOR 2: `merge-multiple: true` file basename collision.
        case_dir = Path(tmp) / "merge-collision"
        shutil.copytree(base, case_dir)
        pub = case_dir / ".github" / "workflows" / "publish-pypi.yml"
        pub.write_text(
            pub.read_text(encoding="utf-8").replace(
                "dist/SHA256SUMS.${{ matrix.target }}",
                "dist/SHA256SUMS",
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1
            and "SHA256SUMS" in res.stdout
            and "collides" in res.stdout,
            "auditor must reject colliding file basenames in `merge-multiple: true`",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 4. Attempt-3 MAJOR 2: `#` comment inside a `\` continuation in composite action.
        case_dir = Path(tmp) / "action-continuation"
        shutil.copytree(base, case_dir)
        act = case_dir / ".github" / "actions" / "build-wheel" / "action.yml"
        act.write_text(
            act.read_text(encoding="utf-8").replace(
                "docker run --rm \\\n",
                "docker run --rm \\\n                # broken comment inside continuation\n",
                1,
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1
            and "build-wheel/action.yml" in res.stdout
            and "continuation" in res.stdout,
            "auditor must reject `#` inside `\\` continuation in composite action steps",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 5. Attempt-2 BLOCKER: `needs:` naming a nonexistent job (`wheels`).
        case_dir = Path(tmp) / "missing-need"
        shutil.copytree(base, case_dir)
        pub = case_dir / ".github" / "workflows" / "publish-pypi.yml"
        pub.write_text(
            pub.read_text(encoding="utf-8").replace(
                "needs: [guard, wheels_gnu, wheels_musl, sdist]",
                "needs: [guard, wheels, sdist]",
                1,
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "needs 'wheels', which is not a job" in res.stdout,
            "auditor must reject nonexistent `needs:` target",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 6. Partial fan-in: `verify` omits `wheels_musl` while downloading `wheel-*`.
        case_dir = Path(tmp) / "partial-fanin"
        shutil.copytree(base, case_dir)
        pub = case_dir / ".github" / "workflows" / "publish-pypi.yml"
        pub.write_text(
            pub.read_text(encoding="utf-8").replace(
                "needs: [guard, wheels_gnu, wheels_musl, sdist]",
                "needs: [guard, wheels_gnu, sdist]",
                1,
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1
            and "missing from its transitive needs: ['wheels_musl']" in res.stdout,
            "auditor must require ALL matching artifact producers in transitive needs",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 7. Missing `if-no-files-found: error` on non-diagnostic upload.
        case_dir = Path(tmp) / "missing-if-no-files"
        shutil.copytree(base, case_dir)
        pub = case_dir / ".github" / "workflows" / "publish-pypi.yml"
        pub.write_text(
            pub.read_text(encoding="utf-8").replace(
                "          name: sdist\n          path: dist/*.tar.gz\n          if-no-files-found: error",
                "          name: sdist\n          path: dist/*.tar.gz",
                1,
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "if-no-files-found: error" in res.stdout,
            "auditor must require `if-no-files-found: error` on artifact uploads",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 8. Double `cp` prefix (`cp${{ matrix.python_tag }}` and `cp"${PY_TAG}"` indirection) + orphaned multi-arch job on x86_64.
        case_dir = Path(tmp) / "orphan-and-double-cp"
        shutil.copytree(base, case_dir)
        whl = case_dir / ".github" / "workflows" / "wheels.yml"
        whl.write_text(
            whl.read_text(encoding="utf-8").replace(
                'dist/blastit-*-"${PY_TAG}"*-musllinux_*.whl',
                'dist/blastit-*-cp"${PY_TAG}"*-musllinux_*.whl',
                1,
            ),
            encoding="utf-8",
        )
        pub = case_dir / ".github" / "workflows" / "publish-pypi.yml"
        pub.write_text(
            pub.read_text(encoding="utf-8")
            + "\n  verify_musl:\n"
            + "    needs: [guard, matrix, binaries_musl]\n"
            + "    runs-on: ubuntu-latest\n"
            + "    strategy:\n"
            + "      matrix: ${{ fromJSON(needs.matrix.outputs.binaries_musl) }}\n"
            + "    steps:\n"
            + "      - run: echo ${{ matrix.target }}\n",
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1
            and "doubles the `cp` prefix" in res.stdout
            and "orphaned" in res.stdout
            and "hardcodes runs-on='ubuntu-latest'" in res.stdout,
            "auditor must catch double `cp` prefix, orphaned publish jobs, and x86_64 runner mismatch",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 9. MB.3.58 failure 1: maturin gets the platform tag twice. `manylinux:`
        #    names the container *and* the wheel's tag, so `--compatibility`
        #    beside it is the `Expected only one platform tag but found 2` build.
        case_dir = Path(tmp) / "double-platform-tag"
        shutil.copytree(base, case_dir)
        act = case_dir / ".github" / "actions" / "build-wheel" / "action.yml"
        act.write_text(
            act.read_text(encoding="utf-8").replace(
                "--interpreter python${{ inputs.python }} --features",
                "--interpreter python${{ inputs.python }} "
                "--compatibility ${{ inputs.platform }} --features",
                1,
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1
            and "passes both `manylinux:` and `--compatibility`" in res.stdout,
            "auditor must reject a maturin step that supplies two platform tags",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 10. ... and the other half of the same rule: a maturin step with no
        #     `manylinux:` builds on the host, so the wheel carries the runner's
        #     glibc tag.
        case_dir = Path(tmp) / "no-manylinux-image"
        shutil.copytree(base, case_dir)
        act = case_dir / ".github" / "actions" / "build-wheel" / "action.yml"
        act.write_text(
            act.read_text(encoding="utf-8").replace(
                "        manylinux: ${{ inputs.manylinux }}\n", "", 1
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "sets no `manylinux:`" in res.stdout,
            "auditor must reject a maturin step that names no manylinux image",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 11. MB.3.58 failure 2: the job runs a runner-built binary in a container
        #     without checking that the container's glibc is at least the
        #     builder's. Deleting the check must fail, not quietly bless it.
        case_dir = Path(tmp) / "no-exec-env-guard"
        shutil.copytree(base, case_dir)
        ci = case_dir / ".github" / "workflows" / "ci.yml"
        guard = (
            "      - name: The runtime image is not older than the builder\n"
            "        run: >-\n"
            "          scripts/check-exec-env.sh --image debian:trixie-slim\n"
            "          --binary target/debug/blastit\n"
        )
        text = ci.read_text(encoding="utf-8")
        _expect(guard in text, "the ci.yml fixture no longer contains the guard step")
        ci.write_text(text.replace(guard, "", 1), encoding="utf-8")
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "without `scripts/check-exec-env.sh`" in res.stdout,
            "auditor must require a glibc check where a runner-built binary enters a container",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 12. The exact pre-node shape that rule 14 exists for: the job builds the
        #     test binaries on the runner and `docker run`s one, with no literal
        #     `target/debug` and no `check-exec-env.sh` anywhere. An artefact-path
        #     rule alone reported "ok" on this.
        case_dir = Path(tmp) / "host-build-then-docker"
        shutil.copytree(base, case_dir)
        php = case_dir / ".github" / "workflows" / "php-sapi.yml"
        anchor = "      - name: 'guard: the pair legs must serve, not refuse'\n"
        restored = (
            "      - name: the pre-node shape (host build, then container run)\n"
            "        run: |\n"
            "          set -eu\n"
            "          cargo test -p blastit --locked --test critical_php_backend --no-run\n"
            '          docker run --rm "$IMAGE" target/debug/blastit --version\n'
        )
        text = php.read_text(encoding="utf-8")
        _expect(anchor in text, "the php-sapi fixture no longer has the guard anchor")
        php.write_text(text.replace(anchor, restored + anchor, 1), encoding="utf-8")
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "builds Rust on the runner" in res.stdout,
            "auditor must reject a host Rust build followed by `docker run` without the check",
            res.stdout + res.stderr,
        )
        cases_run += 1

    return cases_run


def test_platform_matrix_cases() -> int:
    cases_run = 0
    # CLI `--count` and `--leg` behaviour on the live script (read-only).
    for flags, expected in (
        (("--count", "shims", "--libc", "gnu", "--arch", "x86_64"), "3"),
        (("--count", "shims"), "12"),
        (("--count", "wheels"), "8"),
        (("--count", "binaries"), "4"),
    ):
        res = subprocess.run(
            [sys.executable, str(PLATFORM_MATRIX), *flags],
            capture_output=True,
            text=True,
            check=False,
        )
        _expect(
            res.returncode == 0 and res.stdout.strip() == expected,
            f"platform_matrix.py {flags} expected {expected!r}",
            res.stdout + res.stderr,
        )
        cases_run += 1

    ambiguous = subprocess.run(
        [
            sys.executable,
            str(PLATFORM_MATRIX),
            "--leg",
            "shims",
            "--libc",
            "gnu",
            "--arch",
            "x86_64",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    _expect(
        ambiguous.returncode == 2 and "--minor" in ambiguous.stderr,
        "--leg shims without --minor must exit 2 naming --minor",
        ambiguous.stdout + ambiguous.stderr,
    )
    cases_run += 1

    with tempfile.TemporaryDirectory(prefix="blastit-pm-gate-") as tmp:
        base = _copy_matrix_fixture(Path(tmp))
        clean = _run_matrix(base, "--check", "--quiet")
        _expect(
            clean.returncode == 0,
            "clean matrix copy must pass",
            clean.stdout + clean.stderr,
        )
        cases_run += 1

        # Public page over-claims declared wheels.
        case_dir = Path(tmp) / "overclaim-wheels"
        shutil.copytree(base, case_dir)
        gs = case_dir / "docs" / "getting-started.md"
        gs.write_text(
            gs.read_text(encoding="utf-8") + "\n12 wheels ship today.\n",
            encoding="utf-8",
        )
        res = _run_matrix(case_dir, "--check", "--quiet")
        _expect(
            res.returncode == 1
            and "claims 12 wheels, the matrix declares 8" in res.stderr,
            "matrix check must reject public over-claim of wheels",
            res.stderr,
        )
        cases_run += 1

        # Public page over-claims proven shims.
        case_dir = Path(tmp) / "overclaim-proven"
        shutil.copytree(base, case_dir)
        gs = case_dir / "docs" / "getting-started.md"
        gs.write_text(
            gs.read_text(encoding="utf-8") + "\nAll 12 proven shims are built.\n",
            encoding="utf-8",
        )
        res = _run_matrix(case_dir, "--check", "--quiet")
        _expect(
            res.returncode == 1 and "claims 12 proven shims" in res.stderr,
            "matrix check must reject over-claim of proven shims",
            res.stderr,
        )
        cases_run += 1

        # Backticked ADR over-claim (`20` shims, `9` wheels).
        case_dir = Path(tmp) / "adr-backtick"
        shutil.copytree(base, case_dir)
        adr = (
            case_dir
            / "docs"
            / "decisions"
            / "ADR-0010-php-embedded-via-libphp-embed.md"
        )
        adr.write_text(
            adr.read_text(encoding="utf-8") + "\nCarries `20` shims and `9` wheels.\n",
            encoding="utf-8",
        )
        res = _run_matrix(case_dir, "--check", "--quiet")
        _expect(
            res.returncode == 1
            and "claims 20 shims" in res.stderr
            and "claims 9 wheels" in res.stderr,
            "matrix check must reject backticked over-claims in ADR-0010",
            res.stderr,
        )
        cases_run += 1

        # PROVEN naming unbuilt tuple + leg both enabled and dropped.
        case_dir = Path(tmp) / "bad-proven"
        shutil.copytree(base, case_dir)
        pm = case_dir / "scripts" / "platform_matrix.py"
        pm.write_text(
            pm.read_text(encoding="utf-8")
            .replace(
                '("archives", "x86_64", "musl"): "MB.3.18",',
                '("archives", "x86_64", "musl"): "MB.3.18",\n    ("archives", "riscv64", "gnu"): "test",',
                1,
            )
            .replace(
                '{"id": "x86_64-gnu", "arch": "x86_64", "libc": "gnu", "enabled": True},',
                '{"id": "x86_64-gnu", "arch": "x86_64", "libc": "gnu", "enabled": True, "dropped_because": "x"},',
                1,
            ),
            encoding="utf-8",
        )
        res = _run_matrix(case_dir, "--check", "--quiet")
        _expect(
            res.returncode == 1
            and "('archives', 'riscv64', 'gnu')" in res.stderr
            and "enabled and dropped at the same time" in res.stderr,
            "matrix check must reject unknown PROVEN entry and enabled+dropped leg",
            res.stderr,
        )
        cases_run += 1

        # MB.3.58: a leg that does not run the protected suites must say why.
        # Dropping the reason (not the leg) has to fail, or the drop reads as a
        # pass the next time someone reads `--summary`.
        case_dir = Path(tmp) / "suites-without-reason"
        shutil.copytree(base, case_dir)
        pm = case_dir / "scripts" / "platform_matrix.py"
        text = pm.read_text(encoding="utf-8")
        reason = (
            '        "suites_because": (\n'
            '            "PHP 8.2\'s `libphp82` exports no `php_version`, so stage 0 refuses the "\n'
            '            "pair (`MB.3.59` measured the sentence in this image); the shim and its "\n'
            '            "real-load probe still build and run"\n'
            "        ),\n"
        )
        _expect(reason in text, "the matrix fixture no longer carries the musl reason")
        pm.write_text(text.replace(reason, "", 1), encoding="utf-8")
        res = _run_matrix(case_dir, "--check", "--quiet")
        _expect(
            res.returncode == 1 and "no `suites_because` says why" in res.stderr,
            "matrix check must reject a leg whose suites are dropped without a reason",
            res.stderr,
        )
        cases_run += 1

    return cases_run


def test_wheel_tag_guard_cases() -> int:
    cases_run = 0
    with tempfile.TemporaryDirectory(prefix="blastit-whl-gate-") as tmp:
        td = Path(tmp)
        good_228 = _write_test_wheel(
            td,
            "blastit-0.2.0-cp313-cp313-manylinux_2_28_x86_64.whl",
            "cp313-cp313-manylinux_2_28_x86_64",
        )
        bad_239 = _write_test_wheel(
            td,
            "blastit-0.2.0-cp313-cp313-manylinux_2_39_x86_64.whl",
            "cp313-cp313-manylinux_2_39_x86_64",
        )
        bad_linux = _write_test_wheel(
            td,
            "blastit-0.2.0-cp313-cp313-linux_x86_64.whl",
            "cp313-cp313-linux_x86_64",
        )
        good_musl = _write_test_wheel(
            td,
            "blastit-0.2.0-cp314-cp314-musllinux_1_2_aarch64.whl",
            "cp314-cp314-musllinux_1_2_aarch64",
        )

        # 1. manylinux_2_28 passes `--max-glibc 2_28 --platform manylinux_2_28`.
        res = subprocess.run(
            [
                sys.executable,
                str(CHECK_WHEEL_TAG),
                "--max-glibc",
                "2_28",
                "--platform",
                "manylinux_2_28",
                str(good_228),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        _expect(
            res.returncode == 0
            and "Tag: cp313-cp313-manylinux_2_28_x86_64" in res.stdout,
            "manylinux_2_28 wheel must pass 2_28 floor",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 2. manylinux_2_39 fails `--max-glibc 2_28` loudly naming actual tag.
        res = subprocess.run(
            [sys.executable, str(CHECK_WHEEL_TAG), "--max-glibc", "2_28", str(bad_239)],
            capture_output=True,
            text=True,
            check=False,
        )
        _expect(
            res.returncode == 1
            and "Tag: cp313-cp313-manylinux_2_39_x86_64" in res.stderr
            and "manylinux_2_28 floor" in res.stderr,
            "manylinux_2_39 wheel must fail loudly against 2_28 floor",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 3. bare `linux_x86_64` fails.
        res = subprocess.run(
            [
                sys.executable,
                str(CHECK_WHEEL_TAG),
                "--max-glibc",
                "2_41",
                str(bad_linux),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        _expect(
            res.returncode == 1 and "unversioned host tag" in res.stderr,
            "bare linux_x86_64 wheel must be refused",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 4. musllinux_1_2 passes `--platform musllinux_1_2` and fails `--platform manylinux_2_28`.
        ok_musl = subprocess.run(
            [
                sys.executable,
                str(CHECK_WHEEL_TAG),
                "--platform",
                "musllinux_1_2",
                str(good_musl),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        wrong_plat = subprocess.run(
            [
                sys.executable,
                str(CHECK_WHEEL_TAG),
                "--platform",
                "manylinux_2_28",
                str(good_musl),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        _expect(
            ok_musl.returncode == 0 and wrong_plat.returncode == 1,
            "musllinux_1_2 wheel must pass musllinux_1_2 and fail manylinux_2_28",
            ok_musl.stderr + wrong_plat.stderr,
        )
        cases_run += 1

    return cases_run


def main() -> int:
    wf = test_workflow_auditor_cases()
    pm = test_platform_matrix_cases()
    wt = test_wheel_tag_guard_cases()
    total = wf + pm + wt
    print(
        f"packaging-gates-test: ok — {total} cases passed "
        f"(workflow={wf}, matrix={pm}, wheel-tag={wt})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
