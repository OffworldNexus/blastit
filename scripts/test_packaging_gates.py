#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Self-tests (positive + mutated negatives) for the packaging gates (MB.3.18, MB.3.63).

Runs in `make check` (`make workflow-check` / `make platform-matrix-check`) on
throwaway temporary copies — never mutating the repository — so none of the
workflow, matrix, wheel-tag or exec-env guards can silently go vacuous.
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
CHECK_EXEC_ENV = REPO / "scripts" / "check-exec-env.sh"
CHECK_NO_TEST_SEAM = REPO / "scripts" / "check-no-test-seam.sh"

# Recorded verbatim from the shipped `aarch64-unknown-linux-musl` artifact of
# `0.2.0` (`readelf --version-info blastit/_blastit.cpython-314-aarch64-linux-musl.so`):
# the file links musl, and Alpine's aarch64 `libgcc_s` still declares a version
# node spelled `GLIBC_2.0`, which the check read as "needs glibc 2.0" and
# refused in `alpine:3.21` — the one image that can run it (`MB.3.63`).
MUSL_GLIBC_NAMES_TRANSCRIPT = """\
Version symbols section '.gnu.version' contains 311 entries:
 Addr: 0x000000000000302e  Offset: 0x0000302e  Link: 27 (.dynsym)
  000:   0 (*local*)       0 (*local*)       0 (*local*)       1 (*global*)
  004:   2 (GCC_3.3)       3 (GCC_4.2.0)     4 (GLIBC_2.0)     1 (*global*)

Version needs section '.gnu.version_r' contains 1 entry:
 Addr: 0x0000000000002140  Offset: 0x00002140  Link: 5 (.dynstr)
  000000: Version: 1  File: libgcc_s-69c45f16.so.1  Cnt: 4
  0x0010:   Name: GCC_3.0  Flags: none  Version: 5
  0x0020:   Name: GLIBC_2.0  Flags: none  Version: 4
  0x0030:   Name: GCC_4.2.0  Flags: none  Version: 3
  0x0040:   Name: GCC_3.3  Flags: none  Version: 2
"""


def _copy_workflow_fixture(dst: Path) -> Path:
    root = dst / "repo"
    shutil.copytree(REPO / ".github", root / ".github")
    (root / "scripts").mkdir(parents=True)
    shutil.copy2(PLATFORM_MATRIX, root / "scripts" / "platform_matrix.py")
    # Rule 16 reads shell text as well as YAML (`MB.3.63`), so the build script
    # the container cache lives in belongs in the fixture too.
    shutil.copy2(
        REPO / "scripts" / "build-standalone-in-image.sh",
        root / "scripts" / "build-standalone-in-image.sh",
    )
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

        # 13. MB.3.60 defect 1, restored: the gnu archive built on the runner
        #     again. That is what gave every "portable" archive the runner's
        #     glibc 2.39; the wrapper builds it inside the image whose glibc is
        #     the declared floor.
        case_dir = Path(tmp) / "standalone-built-on-runner"
        shutil.copytree(base, case_dir)
        pub = case_dir / ".github" / "workflows" / "publish-pypi.yml"
        text = pub.read_text(encoding="utf-8")
        _expect(
            "          scripts/build-standalone-in-image.sh \\\n" in text,
            "the publish fixture no longer builds the gnu archive in an image",
        )
        pub.write_text(
            text.replace(
                "          scripts/build-standalone-in-image.sh \\\n",
                "          scripts/build-standalone.sh \\\n",
                1,
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1
            and "runs scripts/build-standalone.sh on the runner" in res.stdout,
            "auditor must reject a standalone archive built on the runner",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 14. ... and the other half: a container build that never names the
        #     floor. Without `--max-glibc` nothing measures the binary, so the
        #     number the matrix declares is a comment.
        case_dir = Path(tmp) / "standalone-without-floor"
        shutil.copytree(base, case_dir)
        pub = case_dir / ".github" / "workflows" / "publish-pypi.yml"
        text = pub.read_text(encoding="utf-8")
        floor_line = '              --max-glibc "${{ matrix.glibc_floor }}" \\\n'
        _expect(floor_line in text, "the publish fixture no longer passes --max-glibc")
        pub.write_text(text.replace(floor_line, "", 1), encoding="utf-8")
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "never passes it as" in res.stdout,
            "auditor must require the matrix's glibc floor to be asserted by the build",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 15. `MB.3.63`: the read-only `CARGO_HOME` symlink, restored. Cargo
        #     extracts `registry/src/<crate>` into CARGO_HOME, so a symlink into
        #     the read-only host cache is a write to a read-only mount that only
        #     fires on a cold cache.
        case_dir = Path(tmp) / "readonly-cargo-home"
        shutil.copytree(base, case_dir)
        build = case_dir / "scripts" / "build-standalone-in-image.sh"
        text = build.read_text(encoding="utf-8")
        _expect(
            'cp -a /host-cargo/registry/. "$CARGO_HOME/registry/"' in text,
            "the build fixture no longer seeds CARGO_HOME by copying",
        )
        build.write_text(
            text.replace(
                "if [ -d /host-cargo/registry ]; then",
                "ln -s /host-cargo/registry /tmp/build-cargo-home/registry-link\n"
                "if [ -d /host-cargo/registry ]; then",
                1,
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "symlinks /host-cargo/registry" in res.stdout,
            "auditor must reject a symlink under the container CARGO_HOME",
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


#: A `readelf`/`docker` pair a fixture can answer with, so the judge is tested
#: against evidence the way a foreign-arch or CI host would present it — never
#: against this host's own ELF, which would make the test vacuous wherever the
#: bug cannot appear.
FAKE_READELF = """\
#!/bin/sh
case "$1" in
  --version-info) cat "$FAKE_VERSION_INFO" ;;
  -h) printf '  Machine:                           %s\\n' "$FAKE_MACHINE" ;;
  -l) [ -n "$FAKE_INTERP" ] && printf '  INTERP        0x0002f4\\n        [Requesting program interpreter: %s]\\n' "$FAKE_INTERP"; true ;;
  -d) printf '%s\\n' "$FAKE_NEEDED" | while IFS= read -r lib; do [ -n "$lib" ] && printf ' 0x0000000000000001 (NEEDED)             Shared library: [%s]\\n' "$lib"; done; true ;;
  *) exit 0 ;;
esac
"""

FAKE_DOCKER = """\
#!/bin/sh
case "$1" in
  pull) exit 0 ;;
  run) printf '%s\\n' "$FAKE_IMAGE_LIBC" ;;
  *) exit 0 ;;
esac
"""


def _fake_env(
    directory: Path,
    *,
    machine: str = "AArch64",
    interp: str = "",
    needed: str = "",
    version_info: str,
    image_libc: str = "musl",
) -> dict[str, str]:
    """A PATH whose first `readelf` and `docker` answer with this fixture."""
    bin_dir = directory / "bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    transcript = directory / "version-info.txt"
    transcript.write_text(version_info, encoding="utf-8")
    for name, body in (("readelf", FAKE_READELF), ("docker", FAKE_DOCKER)):
        path = bin_dir / name
        path.write_text(body, encoding="utf-8")
        path.chmod(0o755)
    env = {
        **os.environ,
        "PATH": f"{bin_dir}{os.pathsep}{os.environ.get('PATH', '')}",
        "FAKE_MACHINE": machine,
        "FAKE_INTERP": interp,
        "FAKE_NEEDED": needed,
        "FAKE_VERSION_INFO": str(transcript),
        "FAKE_IMAGE_LIBC": image_libc,
    }
    return env


def _run_exec_env(env: dict[str, str], binary: Path, image: str = "alpine:3.21"):
    return subprocess.run(
        [str(CHECK_EXEC_ENV), "--image", image, "--binary", str(binary)],
        capture_output=True,
        text=True,
        env=env,
        check=False,
    )


def test_exec_env_cases() -> int:
    """`MB.3.63`: the judge reads the binary's own libc, and cannot decide is 2.

    Four cases, each one a verdict the gate used to get wrong in some shape:

    * a file `readelf` cannot parse is exit **2** (cannot decide), never 1 (too
      old) — the direction `MB.3.61` documented and this test pins;
    * a musl artifact whose `.gnu.version_r` carries the name `GLIBC_2.0`
      (recorded from the shipped `aarch64-unknown-linux-musl` wheel) is **not** a
      glibc requirement: the file links `libc.musl-*`, so it passes in
      `alpine:3.21` and says which name it ignored and why;
    * the same `GLIBC_2.0` name on a **glibc** ELF is still compared and still
      refuses a musl image, because the rule keys on the ELF's libc and not on
      the spelling of a version node;
    * a musl interpreter beside `NEEDED libc.so.6` is contradictory evidence and
      is exit 2, not a silently chosen half.
    """
    cases_run = 0
    if shutil.which("readelf") is None:
        raise AssertionError(
            "packaging-gates-test needs a real `readelf` for the unparseable-file case"
        )
    with tempfile.TemporaryDirectory(prefix="blastit-exec-env-") as tmp:
        root = Path(tmp)
        unreadable = root / "README.md"
        unreadable.write_text("# not an ELF\n", encoding="utf-8")
        # A fake `docker` alone: the file must be refused before any image is
        # inspected, so the fixture never needs a real daemon.
        bin_dir = root / "docker-bin"
        bin_dir.mkdir()
        (bin_dir / "docker").write_text(FAKE_DOCKER, encoding="utf-8")
        (bin_dir / "docker").chmod(0o755)
        env = {
            **os.environ,
            "PATH": f"{bin_dir}{os.pathsep}{os.environ.get('PATH', '')}",
            "FAKE_IMAGE_LIBC": "musl",
        }
        res = _run_exec_env(env, unreadable)
        _expect(
            res.returncode == 2,
            "a file readelf cannot parse must be exit 2 (cannot decide), never "
            f"exit 1 (too old); got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "cannot read symbol versions" in res.stderr,
            "the exit-2 refusal must name the parse failure it could not decide on",
            res.stderr,
        )
        cases_run += 1

        binary = root / "blastit"
        binary.write_text("", encoding="utf-8")
        musl_env = _fake_env(
            root / "musl",
            interp="/lib/ld-musl-aarch64.so.1",
            needed="libgcc_s.so.1\nlibc.musl-aarch64.so.1",
            version_info=MUSL_GLIBC_NAMES_TRANSCRIPT,
        )
        res = _run_exec_env(musl_env, binary)
        _expect(
            res.returncode == 0,
            "a musl artifact whose version names include GLIBC_2.0 must pass in a "
            f"musl image, not be refused as glibc (MB.3.63); got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "links musl" in res.stdout + res.stderr
            and "GLIBC_2.0" in res.stdout + res.stderr,
            "the pass must name the GLIBC_* node it ignored and the evidence that "
            "the file links musl",
            res.stdout + res.stderr,
        )
        cases_run += 1

        glibc_env = _fake_env(
            root / "glibc",
            machine="Advanced Micro Devices X86-64",
            interp="/lib64/ld-linux-x86-64.so.2",
            needed="libgcc_s.so.1\nlibc.so.6",
            version_info=MUSL_GLIBC_NAMES_TRANSCRIPT,
        )
        res = _run_exec_env(glibc_env, binary)
        _expect(
            res.returncode == 1,
            "the same GLIBC_2.0 name on a glibc ELF must still be compared and must "
            f"still refuse a musl image; got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "needs glibc 2.0" in res.stderr,
            "the glibc refusal must still read the version name out of the ELF",
            res.stderr,
        )
        cases_run += 1

        contradiction_env = _fake_env(
            root / "contradiction",
            interp="/lib/ld-musl-aarch64.so.1",
            needed="libc.so.6\nlibc.musl-aarch64.so.1",
            version_info=MUSL_GLIBC_NAMES_TRANSCRIPT,
        )
        res = _run_exec_env(contradiction_env, binary)
        _expect(
            res.returncode == 2,
            "a musl interpreter beside `NEEDED libc.so.6` is contradictory evidence: "
            f"exit 2 (cannot decide), never a chosen half; got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "contradicts itself" in res.stderr,
            "the contradiction refusal must say what cannot both be true",
            res.stderr,
        )
        cases_run += 1

        # The three shapes the first version of this judge got wrong (review):
        # `INTERP` is an **absolute** path, so an `ld-musl-*` match never fired;
        # and the contradiction was gated on a `GLIBC_*` name, so two impossible
        # files passed.

        # (a) interp-only musl: a real musl binary, no `libc.*` in NEEDED, whose
        #     version names still carry the `GLIBC_2.0` node of `libgcc_s`.
        interp_only_env = _fake_env(
            root / "interp-only-musl",
            machine="Advanced Micro Devices X86-64",
            interp="/lib/ld-musl-x86_64.so.1",
            needed="libgcc_s.so.1",
            version_info=MUSL_GLIBC_NAMES_TRANSCRIPT,
        )
        res = _run_exec_env(interp_only_env, binary)
        _expect(
            res.returncode == 0,
            "a musl interpreter alone is a musl binary: it must pass in a musl "
            f"image, not read as glibc; got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "interpreter /lib/ld-musl-x86_64.so.1" in res.stdout + res.stderr,
            "the pass must name the interpreter it read (an absolute path, as "
            "readelf prints it) as the evidence that the file links musl",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # (b) the reverse contradiction: a **glibc** loader beside
        #     `NEEDED libc.musl-*` is as impossible as the forward one, and must
        #     not become a false pass by way of "links musl".
        reverse_env = _fake_env(
            root / "reverse-contradiction",
            machine="Advanced Micro Devices X86-64",
            interp="/lib64/ld-linux-x86-64.so.2",
            needed="libc.musl-aarch64.so.1",
            version_info=MUSL_GLIBC_NAMES_TRANSCRIPT,
        )
        res = _run_exec_env(reverse_env, binary)
        _expect(
            res.returncode == 2,
            "a glibc interpreter beside `NEEDED libc.musl-*` is contradictory "
            f"evidence: exit 2 (cannot decide), never 0 'links musl'; got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "contradicts itself" in res.stderr,
            "the reverse contradiction must say what cannot both be true",
            res.stderr,
        )
        cases_run += 1

        # (c) the same forward contradiction with **no** `GLIBC_*` name at all:
        #     the impossible pair is in `INTERP`/`NEEDED`, so it must be exit 2
        #     even though no version node is there to trigger the check.
        nameless_env = _fake_env(
            root / "contradiction-without-names",
            interp="/lib/ld-musl-aarch64.so.1",
            needed="libc.so.6",
            version_info="No version information found in this file.\n",
        )
        res = _run_exec_env(nameless_env, binary)
        _expect(
            res.returncode == 2,
            "a musl interpreter beside `NEEDED libc.so.6` contradicts itself with or "
            f"without GLIBC_* names; got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "contradicts itself" in res.stderr,
            "the name-free contradiction must be refused as such, not passed as "
            "static/no-glibc",
            res.stderr,
        )
        cases_run += 1

    return cases_run


def test_test_seam_cases() -> int:
    """`MB.3.64`: the shipped artifact's absence of the test-only boot seam.

    `#[cfg(debug_assertions)]` is an assertion until a check looks at a file, so
    `scripts/check-no-test-seam.sh` is run here against four synthetic "binaries"
    — real files, never a real build — to prove it is not vacuous:

    * the name present → `--absent` (the shipped direction) must exit **1**;
    * the name absent → `--absent` must exit **0**;
    * the name absent → `--present` (the debug direction) must exit **1**, so the
      absence check cannot pass by the seam having been deleted everywhere;
    * a path that does not exist, and no argument at all, are exit **2**.
    """
    cases_run = 0
    with tempfile.TemporaryDirectory(prefix="blastit-seam-gate-") as tmp:
        root = Path(tmp)
        seam = "BLASTIT_TEST_SWAP_POINT"
        # A file shaped like a real ELF's `.rodata`: the name is a contiguous
        # ASCII literal, which is exactly what the guard's byte search reads.
        with_name = root / "debug-blastit"
        with_name.write_bytes(
            b"\x7fELF\x02\x01\x01\x00"
            + b"\x00" * 61
            + f"stage 1 is at {seam}\n".encode()
            + b"\x00" * 61
            + f"{seam}=\n".encode()
        )
        without_name = root / "release-blastit"
        without_name.write_bytes(b"\x7fELF\x02\x01\x01\x00" + b"\x00" * 120)

        def run(*args: str):
            return subprocess.run(
                ["bash", str(CHECK_NO_TEST_SEAM), *args],
                capture_output=True,
                text=True,
                check=False,
            )

        res = run(str(with_name))
        _expect(
            res.returncode == 1 and "denial-of-service" in res.stderr,
            "the shipped direction must refuse a binary that carries the seam, "
            f"and say why; got exit {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

        res = run("--absent", str(without_name))
        _expect(
            res.returncode == 0 and "no BLASTIT_TEST_SWAP_POINT" in res.stdout,
            f"a release binary must pass; got exit {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

        res = run("--present", str(with_name))
        _expect(
            res.returncode == 0,
            f"a debug binary must pass --present; got exit {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

        res = run("--present", str(without_name))
        _expect(
            res.returncode == 1 and "not in a debug build either" in res.stderr,
            "the debug direction must refuse a binary without the seam, so a seam "
            f"deleted from the source cannot look green; got exit {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

        res = run(str(root / "does-not-exist"))
        _expect(
            res.returncode == 2,
            f"a missing artifact must be exit 2 (cannot decide); got {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

        res = run()
        _expect(
            res.returncode == 2 and "usage:" in res.stderr,
            f"no argument must be exit 2 with usage; got {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

    return cases_run


def main() -> int:
    wf = test_workflow_auditor_cases()
    pm = test_platform_matrix_cases()
    wt = test_wheel_tag_guard_cases()
    ee = test_exec_env_cases()
    ts = test_test_seam_cases()
    total = wf + pm + wt + ee + ts
    print(
        f"packaging-gates-test: ok — {total} cases passed "
        f"(workflow={wf}, matrix={pm}, wheel-tag={wt}, exec-env={ee}, test-seam={ts})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
