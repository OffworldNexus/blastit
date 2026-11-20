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

import json
import os
import re
import shutil
import struct
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
CHECK_PORTABLE_ELF = REPO / "scripts" / "check-portable-elf.sh"
BUILD_WHEEL = REPO / "scripts" / "build-wheel.sh"

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

# The same shape one step further: a **non-glibc** version script that declares an
# ABI node instead of a `GLIBC_<x.y>` one. `GLIBC_ABI_DT_RELR` is the name glibc
# gives its DT_RELR *feature*, so a library that is not glibc can list it without
# the file depending on glibc at all — and a musl ELF carrying it was still read
# as "needs glibc 2.36" and refused in `alpine:3.21` (`MB.3.66`).
MUSL_ABI_NODE_TRANSCRIPT = """\
Version symbols section '.gnu.version' contains 311 entries:
 Addr: 0x000000000000302e  Offset: 0x0000302e  Link: 27 (.dynsym)
  000:   0 (*local*)       1 (*global*)       2 (GLIBC_ABI_DT_RELR)  1 (*global*)

Version needs section '.gnu.version_r' contains 1 entry:
 Addr: 0x0000000000002140  Offset: 0x00002140  Link: 5 (.dynstr)
  000000: Version: 1  File: libgcc_s-69c45f16.so.1  Cnt: 1
  0x0010:   Name: GLIBC_ABI_DT_RELR  Flags: none  Version: 2
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
    # `--check` reads the site's own `exclude_docs` list to decide what a page is
    # (`MB.35.5`), so the fixture carries the site configuration it reads.
    shutil.copy2(REPO / "zensical.toml", root / "zensical.toml")
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
            # Anchored on the action's own build step (rewritten in
            # `MB.42.3.3`: the action now calls `scripts/build-wheel.sh`), so the
            # mutation lands inside a real `\` continuation.
            act.read_text(encoding="utf-8").replace(
                "        bash scripts/build-wheel.sh \\\n",
                "        bash scripts/build-wheel.sh \\\n"
                "            # broken comment inside continuation\n",
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
            whl.read_text(encoding="utf-8")
            # The variable has to be *defined* in the same step for the double
            # prefix to be recognisable (`MB.42.3.3` removed the per-minor
            # `cp${{ matrix.python_tag }}` from the wheel jobs, so this case
            # reintroduces both halves).
            .replace(
                "              -e PY_VER=3.13 \\\n",
                '              -e PY_VER=3.13 \\\n              -e PY_TAG="${{ matrix.python_tag }}" \\\n',
                1,
            )
            .replace(
                "dist/blastit-*-py3-none-musllinux_*.whl",
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

        # 9/10. MB.3.58's two maturin shapes. `MB.42.3.3` replaced the
        # `maturin-action` step in `.github/actions/build-wheel` with
        # `scripts/build-wheel.sh` (one place builds a wheel), so these cases
        # mutate a synthetic step instead of the action: the *rule* is what has to
        # keep working, and a workflow that still calls `maturin-action` directly
        # must still be refused.
        two_tags = (
            "name: maturin two tags\n"
            "on: {workflow_dispatch: {}}\n"
            "jobs:\n"
            "  wheel:\n"
            "    runs-on: ubuntu-24.04\n"
            "    steps:\n"
            "      - uses: PyO3/maturin-action@v1\n"
            "        with:\n"
            "          manylinux: 2_28\n"
            "          args: --release --compatibility manylinux_2_28\n"
        )
        no_image = (
            "name: maturin no image\n"
            "on: {workflow_dispatch: {}}\n"
            "jobs:\n"
            "  wheel:\n"
            "    runs-on: ubuntu-24.04\n"
            "    steps:\n"
            "      - uses: PyO3/maturin-action@v1\n"
            "        with:\n"
            "          args: --release --compatibility manylinux_2_28\n"
        )
        for text, needle, what in (
            (
                two_tags,
                "passes both `manylinux:` and `--compatibility`",
                "two platform tags",
            ),
            (no_image, "sets no `manylinux:`", "no manylinux image"),
        ):
            case_dir = Path(tmp) / f"maturin-{what.replace(' ', '-')}"
            shutil.copytree(base, case_dir)
            (case_dir / ".github" / "workflows" / "maturin-fixture.yml").write_text(
                text, encoding="utf-8"
            )
            res = _run_auditor(case_dir)
            _expect(
                res.returncode == 1 and needle in res.stdout,
                f"auditor must reject a maturin step with {what}",
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

        # 16. `MB.35.5`: the editor family is declared once. A matrix job that
        #     stops exporting `lsp` leaves every consumer reading an output no
        #     job declares, and `fromJSON("")` fails at run time — after the
        #     checkout, on the release PR.
        case_dir = Path(tmp) / "lsp-output-dropped"
        shutil.copytree(base, case_dir)
        ci = case_dir / ".github" / "workflows" / "ci.yml"
        text = ci.read_text(encoding="utf-8")
        output_line = "      lsp: ${{ steps.legs.outputs.lsp }}\n"
        _expect(output_line in text, "the ci fixture no longer exports the lsp matrix")
        ci.write_text(text.replace(output_line, "", 1), encoding="utf-8")
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "needs.matrix.outputs.lsp" in res.stdout,
            "auditor must reject a consumer of an undeclared matrix output",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 17. ... and an asset renamed on one side only: the release downloads
        #     `pattern: lsp-*`, so an upload renamed `editor-*` pairs with
        #     nothing and five assets silently stop shipping.
        case_dir = Path(tmp) / "lsp-asset-renamed"
        shutil.copytree(base, case_dir)
        pub = case_dir / ".github" / "workflows" / "publish-pypi.yml"
        text = pub.read_text(encoding="utf-8")
        upload_line = "          name: lsp-${{ matrix.target }}\n"
        _expect(upload_line in text, "the publish fixture no longer uploads lsp assets")
        pub.write_text(
            text.replace(
                upload_line, "          name: editor-${{ matrix.target }}\n", 1
            ),
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "matches no upload" in res.stdout,
            "auditor must reject a download pattern with no matching upload",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 18. `MB.35.5` / rule 17: a shipped LSP archive built on the runner
        #     instead of inside its glibc floor's image — `MB.3.60`'s defect,
        #     one artifact family later.
        case_dir = Path(tmp) / "lsp-built-on-the-runner"
        shutil.copytree(base, case_dir)
        pub = case_dir / ".github" / "workflows" / "publish-pypi.yml"
        text = pub.read_text(encoding="utf-8")
        image_line = (
            '              args+=(--image "${{ matrix.builder_image }}" '
            '--max-glibc "${{ matrix.glibc_floor }}")\n'
        )
        _expect(
            image_line in text, "the publish fixture no longer names the builder image"
        )
        pub.write_text(text.replace(image_line, "", 1), encoding="utf-8")
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "without `--image`" in res.stdout,
            "auditor must reject a blastit-lsp archive built on the runner",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # 19. `MB.35.5` / rule 18: the five editor targets are the matrix's. A
        #     workflow or the LSP build script that spells one is the second copy
        #     that drifts when a leg is added, dropped or renamed.
        case_dir = Path(tmp) / "editor-target-respelled"
        shutil.copytree(base, case_dir)
        shutil.copy2(
            REPO / "scripts" / "build-lsp.sh", case_dir / "scripts" / "build-lsp.sh"
        )
        builder = case_dir / "scripts" / "build-lsp.sh"
        builder.write_text(
            builder.read_text(encoding="utf-8")
            + "\n# legacy: bash build-lsp.sh --target x86_64-apple-darwin\n",
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "second copy of the table" in res.stdout,
            "auditor must reject an editor target spelled outside platform_matrix.py",
            res.stdout + res.stderr,
        )
        cases_run += 1

    return cases_run


def test_platform_matrix_cases() -> int:
    cases_run = 0
    # CLI `--count` and `--leg` behaviour on the live script (read-only).
    for flags, expected in (
        (("--count", "shims", "--libc", "gnu", "--arch", "x86_64"), "4"),
        (("--count", "shims"), "16"),
        (("--count", "wheels"), "4"),
        (("--count", "py-shims"), "8"),
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
            and "claims 12 wheels, the matrix declares 4" in res.stderr,
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
        # pass the next time someone reads `--summary`. The fixture turns the
        # 8.5/gnu leg's `suites` off, so the case does not depend on any one
        # leg's prose.
        case_dir = Path(tmp) / "suites-without-reason"
        shutil.copytree(base, case_dir)
        pm = case_dir / "scripts" / "platform_matrix.py"
        text = pm.read_text(encoding="utf-8")
        suites_on = '        "suites": True,\n    },\n    ("8.3", "gnu"): {'
        _expect(
            suites_on in text, "the matrix fixture no longer carries the 8.5/gnu leg"
        )
        pm.write_text(
            text.replace(
                suites_on,
                '        "suites": False,\n    },\n    ("8.3", "gnu"): {',
                1,
            ),
            encoding="utf-8",
        )
        res = _run_matrix(case_dir, "--check", "--quiet")
        _expect(
            res.returncode == 1 and "no `suites_because` says why" in res.stderr,
            "matrix check must reject a leg whose suites are dropped without a reason",
            res.stderr,
        )
        cases_run += 1

        # `MB.35.5`: the lsp family's one numeric claim is `5` lsp assets, and the
        # chain noun → key → count has to hold. Counting the editor family as
        # "archives" is the exact drift the separate noun exists to prevent: the
        # page would then be read against the standalone family (`4`).
        case_dir = Path(tmp) / "lsp-counted-as-archives"
        shutil.copytree(base, case_dir)
        pm = case_dir / "scripts" / "platform_matrix.py"
        text = pm.read_text(encoding="utf-8")
        lsp_key = '        "lsp asset": len(lsp_legs()),\n'
        _expect(lsp_key in text, "the matrix fixture no longer declares the lsp key")
        pm.write_text(
            text.replace(lsp_key, '        "lsp asset": len(binary_legs()),\n', 1),
            encoding="utf-8",
        )
        res = _run_matrix(case_dir, "--check", "--quiet")
        _expect(
            res.returncode == 1 and "claims 5 lsp assets" in res.stderr,
            "matrix check must reject an lsp count taken from another family",
            res.stderr,
        )
        cases_run += 1

        # `MB.35.5`: a **design review** is not a page — `zensical.toml`'s
        # `exclude_docs` keeps `docs/design/` out of the site — so a count it
        # quotes must not decide a build. This already happened once: an
        # untracked `portable-lsp-arch.review-implementability.md` said "9
        # archives" and `make check` died at `platform-matrix-check`. The same
        # sentence on a published page must still be refused, so the case proves
        # the boundary rather than "the gate stopped reading".
        case_dir = Path(tmp) / "review-file-is-not-a-page"
        shutil.copytree(base, case_dir)
        review = (
            case_dir
            / "docs"
            / "design"
            / "portable-lsp-arch.review-implementability.md"
        )
        review.parent.mkdir(parents=True, exist_ok=True)
        review.write_text(
            "The matrix declares 9 archives today, and 3 proven wheels.\n",
            encoding="utf-8",
        )
        res = _run_matrix(case_dir, "--check", "--quiet")
        _expect(
            res.returncode == 0,
            "a design review file is not a page: a count it quotes must not red --check",
            res.stdout + res.stderr,
        )
        page = case_dir / "docs" / "getting-started.md"
        page.write_text(
            page.read_text(encoding="utf-8")
            + "\nThe matrix declares 9 archives today.\n",
            encoding="utf-8",
        )
        res = _run_matrix(case_dir, "--check", "--quiet")
        _expect(
            res.returncode == 1 and "claims 9 archives" in res.stderr,
            "the same count on a published page must still be refused",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # `MB.35.5`: one table. A leg added to `LSP_LEGS` alone must reach
        # `--matrix lsp` with every field a producer and a consumer need, and
        # `--summary` must carry its `ci_command` — no second edit anywhere.
        case_dir = Path(tmp) / "lsp-leg-added-once"
        shutil.copytree(base, case_dir)
        pm = case_dir / "scripts" / "platform_matrix.py"
        text = pm.read_text(encoding="utf-8")
        anchor = '        "target": "x86_64-pc-windows-msvc",\n'
        _expect(anchor in text, "the matrix fixture no longer carries the windows leg")
        pm.write_text(
            text.replace(
                anchor,
                '        "target": "aarch64-pc-windows-msvc",\n'
                '        "runs_on": "windows-11-arm",\n'
                '        "platform": "windows",\n'
                '        "enabled": True,\n'
                "    },\n"
                "    {\n" + anchor,
                1,
            ),
            encoding="utf-8",
        )
        res = _run_matrix(case_dir, "--matrix", "lsp")
        _expect(
            res.returncode == 0,
            "the matrix with the added leg must generate",
            res.stdout + res.stderr,
        )
        legs = {leg["target"]: leg for leg in json.loads(res.stdout)["include"]}
        _expect(
            len(legs) == 6,
            "a leg added to LSP_LEGS once must appear in --matrix lsp",
            res.stdout,
        )
        new = legs.get("aarch64-pc-windows-msvc", {})
        _expect(
            new.get("archive") == "blastit-lsp-aarch64-pc-windows-msvc.zip"
            and new.get("dir") == "blastit-lsp-aarch64-pc-windows-msvc"
            and new.get("executable") == "blastit-lsp.exe"
            and "scripts/lsp_smoke.py --binary dist/smoke-aarch64-pc-windows-msvc/"
            in new.get("ci_command", ""),
            "the added leg's names must be derived by the table, not re-spelled",
            res.stdout,
        )
        summary = _run_matrix(case_dir, "--summary", "lsp")
        _expect(
            summary.returncode == 0
            and "`blastit-lsp-aarch64-pc-windows-msvc.zip`" in summary.stdout
            and "NOT RUN" in summary.stdout,
            "the summary must list the new leg and its command",
            summary.stdout + summary.stderr,
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
    """`MB.3.63`/`MB.3.66`/`MB.3.67`: the judge reads the binary's own libc.

    Twelve cases, each one a verdict the gate used to get wrong in some shape:

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
      is exit 2, not a silently chosen half;
    * a musl ELF whose only version needs are a non-glibc `GLIBC_ABI_DT_RELR`
      node passes in `alpine:3.21` rather than being read as a 2.36 floor, while
      the same node on a **glibc** ELF keeps its floor and is refused by 2.31
      (`MB.3.66`) — and **accepted** by a 2.36 image, so the refusal is a
      comparison and not a constant (`MB.3.67` item 4);
    * an ELF whose own headers name no C library at all — no `INTERP`, no
      `NEEDED libc.*`, the static-pie shape — is exit **2** (cannot decide), not a
      refusal built on a version name nothing attributed to glibc (`MB.3.67`).
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

        # (d) the last false refusal of its class (`MB.3.66`): a **non-glibc**
        #     version script declaring an ABI node — `GLIBC_ABI_DT_RELR`, no
        #     `GLIBC_<x.y>` name at all — on a musl ELF. The ABI-node loop used to
        #     run whatever the loader said, so this file was read as "needs glibc
        #     2.36" and refused in the one image that can run it. The libc is a
        #     fact in `INTERP`/`NEEDED`, so a glibc feature name from a library
        #     that is not glibc is no more a floor than `GLIBC_2.0` is, and the
        #     ignored name is printed rather than dropped.
        musl_abi_env = _fake_env(
            root / "musl-with-abi-node",
            interp="/lib/ld-musl-aarch64.so.1",
            needed="libgcc_s.so.1\nlibc.musl-aarch64.so.1",
            version_info=MUSL_ABI_NODE_TRANSCRIPT,
        )
        res = _run_exec_env(musl_abi_env, binary)
        _expect(
            res.returncode == 0,
            "a musl artifact whose version needs carry only a GLIBC_ABI_* node "
            "must pass in a musl image, not be read as a 2.36 glibc floor "
            f"(MB.3.66); got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "links musl" in res.stdout + res.stderr
            and "GLIBC_ABI_DT_RELR" in res.stdout + res.stderr
            and "2.36" not in res.stdout + res.stderr,
            "the pass must name the GLIBC_ABI_* node it ignored and must not "
            "quote a 2.36 floor for a file that loads no glibc",
            res.stdout + res.stderr,
        )
        cases_run += 1

        #     The skip is the *loop*, not a whitelist of known nodes: an ABI name
        #     this check has never heard of is also not a glibc floor on a file
        #     that loads no glibc, and refusing it with "cannot decide" would be
        #     the same false refusal wearing exit 2. It is still reported.
        unknown_abi_transcript = MUSL_ABI_NODE_TRANSCRIPT.replace(
            "GLIBC_ABI_DT_RELR", "GLIBC_ABI_FUTURE_FEATURE"
        )
        musl_unknown_abi_env = _fake_env(
            root / "musl-with-unknown-abi-node",
            interp="/lib/ld-musl-x86_64.so.1",
            needed="libgcc_s.so.1",
            version_info=unknown_abi_transcript,
        )
        res = _run_exec_env(musl_unknown_abi_env, binary)
        _expect(
            res.returncode == 0,
            "an ABI node this check does not know is still not a glibc floor on "
            f"a musl ELF: the loop is skipped, not filtered; got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "GLIBC_ABI_FUTURE_FEATURE" in res.stdout + res.stderr,
            "the skipped node must be named, not dropped",
            res.stdout + res.stderr,
        )
        cases_run += 1

        #     ... and the same node on a **glibc** ELF keeps its floor: 2.36 from
        #     `GLIBC_ABI_DT_RELR`, so a 2.31 image is still refused. This is the
        #     leg that forbids the shortcut of exempting ABI nodes outright.
        glibc_abi_env = _fake_env(
            root / "glibc-with-abi-node",
            machine="Advanced Micro Devices X86-64",
            interp="/lib64/ld-linux-x86-64.so.2",
            needed="libgcc_s.so.1\nlibc.so.6",
            version_info=MUSL_ABI_NODE_TRANSCRIPT,
            image_libc="2.31",
        )
        res = _run_exec_env(glibc_abi_env, binary, image="debian:bullseye-slim")
        _expect(
            res.returncode == 1,
            "a glibc ELF that genuinely needs GLIBC_ABI_DT_RELR must keep its "
            f"2.36 floor and be refused by a 2.31 image; got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "needs glibc 2.36" in res.stderr and "GLIBC_ABI_DT_RELR" in res.stderr,
            "the glibc refusal must name the ABI node the loader would not find",
            res.stderr,
        )
        cases_run += 1

        #     ... and the accept side, so a future "always exit 1" cannot pass this
        #     suite: the same 2.36-needing glibc ELF in a 2.36 image passes, and
        #     says which floor it read (`MB.3.67` item 4).
        glibc_abi_236_env = _fake_env(
            root / "glibc-with-abi-node-236",
            machine="Advanced Micro Devices X86-64",
            interp="/lib64/ld-linux-x86-64.so.2",
            needed="libgcc_s.so.1\nlibc.so.6",
            version_info=MUSL_ABI_NODE_TRANSCRIPT,
            image_libc="2.36",
        )
        res = _run_exec_env(glibc_abi_236_env, binary, image="debian:bookworm-slim")
        _expect(
            res.returncode == 0,
            "the same glibc ELF needing 2.36 must pass in a 2.36 image, or the "
            f"refusal above is a constant; got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "needs glibc 2.36" in res.stdout and "provides 2.36" in res.stdout,
            "the pass must print the floor it read and the version it compared it to",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # (e) `MB.3.67` item 3: an ELF whose own headers name **no** C library — no
        #     `INTERP`, no `NEEDED libc.*`, the shape a static-pie artefact has —
        #     still carries whatever version script went into it, and this file
        #     carries a `GLIBC_ABI_DT_RELR` name. The check took that name as a
        #     2.36 floor and refused `alpine:3.21` with "a glibc ELF cannot start
        #     where there is no glibc" printed beside "interpreter none": a floor
        #     read from a libc the file never named. Without that evidence there is
        #     no floor and no verdict — exit 2, never 1.
        no_libc_env = _fake_env(
            root / "no-c-library",
            machine="Advanced Micro Devices X86-64",
            interp="",
            needed="",
            version_info=MUSL_ABI_NODE_TRANSCRIPT,
        )
        res = _run_exec_env(no_libc_env, binary)
        _expect(
            res.returncode == 2,
            "an ELF with no INTERP and no NEEDED libc.* cannot be classified, so it "
            f"must be exit 2 (cannot decide), never 1 (too old); got {res.returncode}",
            res.stdout + res.stderr,
        )
        _expect(
            "cannot decide which C library" in res.stderr
            and "interpreter none" in res.stderr,
            "the exit-2 refusal must say what it read (no interpreter, no libc) and "
            "must not claim a glibc ELF where there is none",
            res.stderr,
        )
        _expect(
            "a glibc ELF cannot start where there is no glibc" not in res.stderr,
            "a file that names no libc must not be refused as a glibc ELF",
            res.stderr,
        )
        cases_run += 1

    return cases_run


def test_test_seam_cases() -> int:
    """`MB.3.64`: the shipped artifact's absence of the test-only boot seam.

    `#[cfg(debug_assertions)]` is an assertion until a check looks at a file, so
    `scripts/check-no-test-seam.sh` is run here against real files — never a real
    build — to prove it is not vacuous:

    * the name present → `--absent` (the shipped direction) must exit **1**;
    * the name absent → `--absent` must exit **0**;
    * the name absent → `--present` (the debug direction) must exit **1**, so the
      absence check cannot pass by the seam having been deleted everywhere;
    * a file this check cannot read (mode `000`) is exit **2** naming the path,
      never `ok … absent` with 0 about a file nothing read (`MB.3.67`);
    * a path that does not exist, and no argument at all, are exit **2**.
    """
    cases_run = 0
    # Mode 000 denies a read to a normal user only: as root the fixture would be
    # read, the guard below would pass for the wrong reason, and the case would
    # pin nothing. `make check` is unprivileged, so say so instead of skipping.
    if os.geteuid() == 0:
        raise AssertionError(
            "the unreadable-artifact case needs a non-root user (mode 000 stays "
            "readable as root): run scripts/test_packaging_gates.py unprivileged"
        )
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

        # `MB.3.67` item 1: a file this check cannot read is not a file with zero
        # occurrences. `grep` exits 2 on it and the pipeline still yields `0` hits,
        # so `--absent` used to print `ok … absent` and exit 0 about a file nothing
        # read — a shipped binary that failed to be measured passing as measured.
        unreadable = root / "unreadable-release-blastit"
        unreadable.write_bytes(with_name.read_bytes())
        unreadable.chmod(0o000)
        try:
            _expect(
                not os.access(unreadable, os.R_OK),
                "the unreadable fixture is readable anyway, so this case would pin "
                "nothing",
            )
            res = run("--absent", str(unreadable))
            _expect(
                res.returncode == 2,
                "an unreadable artifact must be exit 2 (no verdict), never "
                f"'absent' with 0; got exit {res.returncode}",
                res.stdout + res.stderr,
            )
            _expect(
                str(unreadable) in res.stderr
                and "absent, as shipped" not in res.stdout,
                "the exit-2 refusal must name the path it could not read and must "
                "not print an absence verdict",
                res.stdout + res.stderr,
            )
            # The other direction fails for the same reason, not as "the seam was
            # deleted from the source": exit 2 says nothing was read, and exit 1
            # would be a verdict about the file's contents.
            res = run("--present", str(unreadable))
            _expect(
                res.returncode == 2,
                "an unreadable artifact must be exit 2 in the debug direction too, "
                f"not 1 'the seam is gone'; got exit {res.returncode}",
                res.stdout + res.stderr,
            )
        finally:
            unreadable.chmod(0o600)
        cases_run += 1

        res = run()
        _expect(
            res.returncode == 2 and "usage:" in res.stderr,
            f"no argument must be exit 2 with usage; got {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

    return cases_run


def test_portable_elf_shim_cases() -> int:
    """A Python shim's *undefined* `Py_*` is not a `DT_NEEDED` on libpython.

    `libblastit_py3{13,14}-<arch>-<libc>.so` is the one artifact that must carry
    `UND Py_InitializeEx`: stage 1 `dlopen`s the host's `libpython` first and the
    shim second, so the shim binds nothing at load time. The gate exists to
    refuse the *other* shape — a shim that links an interpreter of its own and
    would only run on the machine that built it — and these two fixtures keep the
    two from collapsing into "any mention of Python is refused".
    """
    cc = shutil.which("cc")
    if cc is None:
        print("portable-elf: skipped (no cc to build the two fixtures)")
        return 0

    cases_run = 0
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        libdir = tmp / "lib"
        libdir.mkdir()

        source = tmp / "fake_libpython.c"
        source.write_text(
            "int blastit_fake_libpython(void) { return 0; }\n", encoding="utf-8"
        )
        fake = libdir / "libpython3.13.so.1.0"
        subprocess.run(
            [
                cc,
                "-shared",
                "-fPIC",
                "-Wl,-soname,libpython3.13.so.1.0",
                "-o",
                str(fake),
                str(source),
            ],
            check=True,
        )

        # The refused shape: a shim carrying `NEEDED libpython3.13.so.1.0`.
        linked_c = tmp / "linked.c"
        linked_c.write_text("int blastit_probe(void) { return 0; }\n", encoding="utf-8")
        linked = tmp / "libblastit_py313-linked.so"
        subprocess.run(
            [
                cc,
                "-shared",
                "-fPIC",
                f"-L{libdir}",
                "-Wl,--no-as-needed",
                "-l:libpython3.13.so.1.0",
                "-Wl,-soname,libblastit_py313-linked.so",
                "-o",
                str(linked),
                str(linked_c),
            ],
            check=True,
        )
        res = subprocess.run(
            [str(CHECK_PORTABLE_ELF), str(linked)],
            capture_output=True,
            text=True,
            check=False,
        )
        _expect(
            res.returncode == 1 and "links an interpreter" in res.stderr,
            f"a shim with DT_NEEDED libpython must be refused; got exit {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

        # The accepted shape: the same `Py_*` name, undefined and resolved later.
        shim_c = tmp / "shim.c"
        shim_c.write_text(
            "extern void Py_InitializeEx(int);\n"
            "void blastit_probe(void) { Py_InitializeEx(0); }\n",
            encoding="utf-8",
        )
        shim = tmp / "libblastit_py313-x86_64-gnu.so"
        subprocess.run(
            [
                cc,
                "-shared",
                "-fPIC",
                "-Wl,-soname,libblastit_py313-x86_64-gnu.so",
                "-o",
                str(shim),
                str(shim_c),
            ],
            check=True,
        )
        dynamic = subprocess.run(
            ["readelf", "-d", str(shim)], capture_output=True, text=True, check=True
        ).stdout
        _expect(
            "libpython" not in dynamic,
            "the fixture must not need libpython, or the case proves nothing",
            dynamic,
        )
        undefined = subprocess.run(
            ["nm", "-uD", str(shim)], capture_output=True, text=True, check=True
        ).stdout
        _expect(
            "Py_InitializeEx" in undefined,
            "the shim fixture must carry an undefined dynamic Py_InitializeEx",
            undefined,
        )
        res = subprocess.run(
            [str(CHECK_PORTABLE_ELF), str(shim)],
            capture_output=True,
            text=True,
            check=False,
        )
        _expect(
            res.returncode == 0 and "ok   " in res.stdout,
            "a shim with UND Py_* and no DT_NEEDED libpython must be accepted; "
            f"got exit {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

    return cases_run


def mark_verneed_weak(path: Path, version: str) -> bool:
    """Set `VER_FLG_WEAK` on one `.gnu.version_r` entry (`Elf64_Vernaux`).

    GNU ld does not set the flag for a C `__attribute__((weak))` reference even
    when every reference is weak; LLVM's lld — the linker `rustc` uses for
    `blastit` — does, and that is the shape the package floor has to survive. The
    fixture is patched rather than argued about, and the caller re-reads it with
    `readelf` to prove the patch landed.
    """
    sections_out = subprocess.run(
        ["readelf", "-S", "-W", str(path)],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    sections: dict[str, tuple[int, int]] = {}
    for line in sections_out.splitlines():
        match = re.match(
            r"\s*\[\s*\d+\]\s+(\S+)\s+\S+\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)",
            line,
        )
        if match:
            sections[match.group(1)] = (
                int(match.group(3), 16),
                int(match.group(4), 16),
            )
    if ".gnu.version_r" not in sections or ".dynstr" not in sections:
        return False
    data = bytearray(path.read_bytes())
    verneed_offset, verneed_size = sections[".gnu.version_r"]
    dynstr_offset, dynstr_size = sections[".dynstr"]
    dynstr = bytes(data[dynstr_offset : dynstr_offset + dynstr_size])
    position = verneed_offset
    while position < verneed_offset + verneed_size:
        _, count, _, aux_offset, next_offset = struct.unpack_from(
            "<HHIII", data, position
        )
        aux = position + aux_offset
        for _ in range(count):
            _, flags, _, name_offset, aux_next = struct.unpack_from("<IHHII", data, aux)
            end = dynstr.find(b"\0", name_offset)
            if dynstr[name_offset:end].decode("ascii", "replace") == version:
                struct.pack_into("<H", data, aux + 4, flags | 0x2)
                path.write_bytes(data)
                return True
            aux += aux_next
        if not next_offset:
            break
        position += next_offset
    return False


def test_portable_elf_glibc_cases() -> int:
    """`--max-glibc` measures the *loader-enforced* floor, and only that.

    Two fixtures, because the two shapes are what the `2.34` in
    `build-{deb,rpm}.sh` rests on:

      * a **strong** `GLIBC_2.39` reference must fail `--max-glibc 2_28` (that is
        the assertion; it names the file and the number);
      * the same symbol referenced **weakly** must pass at `2.28` and report a
        *lower* number, because glibc prints `weak version … not found` and runs.

    If the host libc predates 2.39, the fixtures cannot carry the version need and
    the cases are skipped with the reason printed (like `cc`-less hosts already
    are).
    """
    cc = shutil.which("cc")
    if cc is None:
        print("portable-elf --max-glibc: skipped (no cc to build the fixtures)")
        return 0

    def run(*args: str):
        return subprocess.run(
            [str(CHECK_PORTABLE_ELF), *args],
            capture_output=True,
            text=True,
            check=False,
        )

    cases_run = 0
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)

        strong_c = tmp / "strong.c"
        strong_c.write_text(
            "extern int pidfd_spawnp(void);\n"
            "void blastit_strong(void) { pidfd_spawnp(); }\n",
            encoding="utf-8",
        )
        strong = tmp / "libblastit_strong.so"
        subprocess.run(
            [cc, "-shared", "-fPIC", "-o", str(strong), str(strong_c)], check=True
        )
        version_info = subprocess.run(
            ["readelf", "--version-info", str(strong)],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        if "GLIBC_2.39" not in version_info:
            print(
                "portable-elf --max-glibc: skipped — this libc predates 2.39, so a "
                "2.39 need cannot be built"
            )
            return 0

        res = run("--max-glibc", "2_28", str(strong))
        _expect(
            res.returncode == 1
            and "needs glibc 2.39" in res.stderr
            and "weak version needs are excluded" in res.stderr,
            "a strong GLIBC_2.39 need must fail --max-glibc 2_28 and say which file; "
            f"got exit {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

        res = run("--max-glibc", "auto", str(strong))
        _expect(
            res.returncode == 0 and "max-glibc: 2.39 (" in res.stdout,
            f"a strong 2.39 need must measure 2.39; got exit {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

        weak_c = tmp / "weak.c"
        weak_c.write_text(
            "extern int write(int, const void *, unsigned long);\n"
            "extern int pidfd_getpid(void) __attribute__((weak));\n"
            "void blastit_weak(const char *s) { (void)write(1, s, 1); if (pidfd_getpid) pidfd_getpid(); }\n",
            encoding="utf-8",
        )
        weak = tmp / "libblastit_weak.so"
        subprocess.run(
            [cc, "-shared", "-fPIC", "-o", str(weak), str(weak_c)], check=True
        )
        # GNU ld records the 2.39 entry as strong even when every reference is weak;
        # LLVM's lld (what `rustc` links `blastit` with) records `VER_FLG_WEAK`, and
        # that is the shape the floor logic must handle. Mark the entry the way lld
        # does, then assert `readelf` agrees: the fixture has to *be* the shape.
        _expect(
            mark_verneed_weak(weak, "GLIBC_2.39"),
            "could not mark the fixture's GLIBC_2.39 version need weak",
        )
        weak_info = subprocess.run(
            ["readelf", "--version-info", str(weak)],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        _expect(
            "GLIBC_2.39  Flags: WEAK" in weak_info,
            "the patched fixture must show a WEAK GLIBC_2.39 need, or the case proves "
            "nothing",
            weak_info,
        )
        res = run("--max-glibc", "2_28", str(weak))
        measured = [
            line for line in res.stdout.splitlines() if line.startswith("max-glibc: ")
        ]
        _expect(
            res.returncode == 0 and measured and "2.39" not in measured[0],
            "a WEAK 2.39 need must pass at 2.28 and report the older number (the "
            f"loader does not require it); got exit {res.returncode}: {measured}",
            res.stdout + res.stderr,
        )
        cases_run += 1

    # The `NEEDED` allowlist: a real ELF that links something other than
    # libc/libm/libgcc/loader is refused, or a package could declare `Depends:
    # libc6` while `needing` libselinux. `/bin/ls` is that shape on most hosts.
    ls_needed = subprocess.run(
        ["readelf", "-d", "/bin/ls"], capture_output=True, text=True, check=False
    ).stdout
    foreign = [
        line.split("[", 1)[1].rstrip("]")
        for line in ls_needed.splitlines()
        if "(NEEDED)" in line
    ]
    allowed = (
        "libc.so.6",
        "libm.so.6",
        "libgcc_s.so.1",
        # glibc libraries that were separate objects before their 2.34 merge:
        # still part of libc6/glibc, so not a package dependency.
        "libdl.so.2",
        "libpthread.so.0",
        "librt.so.1",
    )
    if any(
        not name.startswith(("ld-linux-", "libc.musl-")) and name not in allowed
        for name in foreign
    ):
        res = run("--max-glibc", "auto", "/bin/ls")
        _expect(
            res.returncode == 1
            and "which no supported distribution guarantees" in res.stderr,
            "an ELF needing an unlisted library must be refused (the allowlist is the "
            f"package's dependency contract); got exit {res.returncode}",
            res.stdout + res.stderr,
        )
        cases_run += 1

    return cases_run


def test_matrix_producer_cases() -> int:
    """Rule 19 (`MB.42.3.3` review): a leg list that no step generates is refused.

    The failure that motivated it: replacing `py-sapi.yml`'s producer step with a
    hard-coded one-leg JSON array kept every other rule green (`ok — 458 checks`),
    because a hand-written array has exactly the shape a generator output has.
    """
    cases_run = 0
    with tempfile.TemporaryDirectory(prefix="blastit-matrix-producer-") as tmp:
        # The workflow fixture, not the count fixture: this case is about the
        # YAML graph (`_copy_matrix_fixture` copies no `.github/`).
        base = _copy_workflow_fixture(Path(tmp))
        clean = _run_auditor(base)
        _expect(
            clean.returncode == 0,
            "the fixture workflows must pass the producer rule",
            clean.stdout + clean.stderr,
        )
        cases_run += 1

        case_dir = Path(tmp) / "hand-written-legs"
        shutil.copytree(base, case_dir)
        sa = case_dir / ".github" / "workflows" / "py-sapi.yml"
        text = sa.read_text(encoding="utf-8")
        start = text.index("      - id: legs")
        end = text.index("  lint:")
        sa.write_text(
            text[:start]
            + "      - id: legs\n"
            + "        run: |\n"
            + '          echo \'py_shims=[{"name":"x86_64-gnu-py313","minor":"3.13","arch":"x86_64","libc":"gnu","shim":"libblastit_py313-x86_64-gnu.so","runs_on":"ubuntu-24.04","builder_image":"","exports":"e","undefined":"u","python":"3.13"}]\' >> "$GITHUB_OUTPUT"\n'
            + text[end:],
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1 and "--matrix py-shims" in res.stdout,
            "a producer that does not run platform_matrix.py must be refused",
            res.stdout + res.stderr,
        )
        cases_run += 1
    return cases_run


def test_repack_cases() -> int:
    """`scripts/repack_universal_wheel.py`: the `py3-none` layout, and its entry points.

    A wheel is written here (a `.data/scripts/blastit`, the package, a `WHEEL` and a
    `RECORD`), repacked, and read back: the name, the tag, `Root-Is-Purelib`, the
    three console scripts, the relocated binary, the `RECORD` digests `pip` checks,
    and that another `(arch, libc)`'s shim is *not* carried into a wheel.
    """
    import importlib

    cases_run = 0
    sys.path.insert(0, str(REPO / "scripts"))
    try:
        repack = importlib.import_module("repack_universal_wheel")
    finally:
        sys.path.remove(str(REPO / "scripts"))
    with tempfile.TemporaryDirectory(prefix="blastit-repack-") as tmp:
        case = Path(tmp)
        built = case / "blastit-0.2.0-cp313-cp313-linux_x86_64.whl"
        metadata = (
            "Wheel-Version: 1.0\nGenerator: maturin (1.9.3)\nRoot-Is-Purelib: true\n"
        )
        with zipfile.ZipFile(built, "w") as archive:
            archive.writestr("blastit/__init__.py", b"")
            archive.writestr("blastit/__main__.py", b"")
            archive.writestr("blastit-0.2.0.data/scripts/blastit", b"#!/bin/sh\n")
            archive.writestr("blastit-0.2.0.dist-info/WHEEL", metadata)
            archive.writestr("blastit-0.2.0.dist-info/METADATA", "Name: blastit\n")
            archive.writestr("blastit-0.2.0.dist-info/RECORD", "")
        blobs = case / "blobs"
        blobs.mkdir()
        (blobs / "blastit-lsp").write_bytes(b"#!/bin/sh\n")
        (blobs / "libblastit_php84-x86_64-gnu.so").write_bytes(b"\x7fELF local")
        foreign = blobs / "libblastit_php84-aarch64-gnu.so"
        foreign.write_bytes(b"\x7fELF foreign")

        # A shim of another `(arch, libc)` is **refused**, not silently skipped:
        # skipping produced a wheel that shipped fewer shims than the leg
        # promised while every count still said six (`MB.42.3.3` review).
        try:
            repack.repack(
                built, case / "refused", platform="manylinux_2_28_x86_64", blobs=blobs
            )
        except ValueError as err:
            _expect(
                "aarch64-gnu" in str(err) and "x86_64-gnu" in str(err),
                "a foreign shim must be refused, naming both targets",
                str(err),
            )
            cases_run += 1
        else:
            _expect(False, "a foreign shim must be refused, not skipped", "")
        foreign.unlink()

        wheel = repack.repack(
            built, case / "out", platform="manylinux_2_28_x86_64", blobs=blobs
        )
        _expect(
            wheel.name == "blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl",
            "the repack must name the file <name>-<ver>-py3-none-<platform>.whl",
            wheel.name,
        )
        cases_run += 1
        with zipfile.ZipFile(wheel) as archive:
            names = archive.namelist()
            wheel_meta = archive.read("blastit-0.2.0.dist-info/WHEEL").decode()
            entry_points = archive.read(
                "blastit-0.2.0.dist-info/entry_points.txt"
            ).decode()
            record = archive.read("blastit-0.2.0.dist-info/RECORD").decode()
            binary = archive.read("blastit/_bin/blastit")
            local = [name for name in names if name.endswith("x86_64-gnu.so")]
            foreign = [name for name in names if name.endswith("aarch64-gnu.so")]
        _expect(
            "Tag: py3-none-manylinux_2_28_x86_64" in wheel_meta
            and "Root-Is-Purelib: false" in wheel_meta
            and wheel_meta.count("Root-Is-Purelib") == 1,
            "WHEEL must carry one Root-Is-Purelib: false and the py3-none tag",
            wheel_meta,
        )
        cases_run += 1
        _expect(
            binary == b"#!/bin/sh\n" and not any(".data/scripts" in n for n in names),
            "the binary must move out of .data/scripts/ into blastit/_bin/",
            " ".join(names),
        )
        cases_run += 1
        _expect(
            local and not foreign,
            "a wheel carries its own (arch, libc) shims and never another target's",
            f"local={local} foreign={foreign}",
        )
        cases_run += 1
        # MINOR 10: a real `stat` mode, not a bare `0o755` (which renders as
        # `?rwxr-xr-x` to `zipinfo`, and `pip` reads a mode from here).
        with zipfile.ZipFile(wheel) as archive:
            modes = {
                info.filename: (info.external_attr >> 16)
                for info in archive.infolist()
                if info.filename.startswith("blastit/_bin/")
            }
        _expect(
            all(mode == 0o100755 for mode in modes.values()) and modes,
            "every blastit/_bin member must carry 0100755 (a regular file, executable)",
            str(modes),
        )
        cases_run += 1
        # MINOR 11: an unknown platform is refused, never guessed as aarch64.
        try:
            repack.repack(built, case / "unknown", platform="linux_riscv64")
        except ValueError as err:
            _expect(
                "names no architecture" in str(err),
                "an unreadable platform must be refused, not guessed",
                str(err),
            )
            cases_run += 1
        else:
            _expect(False, "an unreadable platform must be refused", "")
        expected_scripts = {"blastit", "blastit-lsp", "blastit-manifest"}
        _expect(
            all(f"{name} = " in entry_points for name in sorted(expected_scripts)),
            "entry_points.txt must carry the three console scripts (design §6.1)",
            entry_points,
        )
        cases_run += 1
        rows = {
            line.split(",")[0]: line.split(",") for line in record.splitlines() if line
        }
        ok = "blastit/_bin/blastit" in rows and rows["blastit/_bin/blastit"][1]
        digest, size = rows.get("blastit/_bin/blastit", ["", "", ""])[1:]
        import base64
        import hashlib

        want = "sha256=" + base64.urlsafe_b64encode(
            hashlib.sha256(binary).digest()
        ).decode().rstrip("=")
        _expect(
            ok and digest == want and size == str(len(binary)),
            "RECORD must carry the sha256 and size pip verifies",
            f"{digest or '<missing>'} size={size} want={want}",
        )
        cases_run += 1
        _expect(
            rows["blastit-0.2.0.dist-info/RECORD"][1:] == ["", ""],
            "RECORD must list itself with an empty digest and size",
            record,
        )
        cases_run += 1

        # Every console script target has to exist: a rename in `python/blastit/`
        # would otherwise ship a dead entry point (`blastit-lsp` was one).
        sys.path.insert(0, str(REPO / "python"))
        try:
            for name, target in repack.CONSOLE_SCRIPTS.items():
                module_name, _, attribute = target.partition(":")
                module = importlib.import_module(module_name)
                _expect(
                    callable(getattr(module, attribute, None)),
                    f"console script {name} = {target} does not resolve",
                    target,
                )
                cases_run += 1
        finally:
            sys.path.remove(str(REPO / "python"))
    return cases_run


# --- `MB.42.3.4`: the two publishing workflows and the legs they run ----------


def _workflow_step_script(path: Path, step_id: str) -> str:
    """The `run: |` body of the step whose `id:` is `step_id`, read line by line.

    `test_packaging_gates.py` has no YAML dependency (the auditor owns that), and
    the point of this helper is to execute the **shipped** `run:` text rather than
    a paraphrase of it.
    """
    lines = path.read_text(encoding="utf-8").splitlines()
    start = next(
        index for index, line in enumerate(lines) if line.strip() == f"id: {step_id}"
    )
    run_at = next(
        index for index in range(start, len(lines)) if lines[index].strip() == "run: |"
    )
    body: list[str] = []
    for line in lines[run_at + 1 :]:
        if line.strip() and not line.startswith(" " * 10):
            break
        body.append(line[10:] if line.startswith(" " * 10) else "")
    return "\n".join(body)


def test_package_workflow_cases() -> int:
    """`MB.42.3.4`: a leg hand-removed from, or written into, a package workflow.

    Two failures the publishing workflows have to make loud, mutated on a
    throwaway copy and therefore reverted by construction:

    * the `--matrix install` producer line deleted by hand. The `install` job
      still reads `needs.matrix.outputs.install`, so without rule 19 a release
      would publish with five of six distributions installed and read as green;
    * a distribution written into the YAML (`docker run debian:stretch`). That is
      the second list, the one that keeps advertising a release after the matrix
      dropped it — and it is the thing `packages.yml`/`nightly.yml` are not
      allowed to have.

    Plus the install family itself: 12 legs (six distributions × two
    architectures), and a leg that asks an image for a WSGI import it cannot
    carry is refused by `--check`.
    """
    cases_run = 0
    with tempfile.TemporaryDirectory(prefix="blastit-pkg-wf-") as tmp:
        base = _copy_workflow_fixture(Path(tmp))
        clean = _run_auditor(base)
        _expect(
            clean.returncode == 0,
            "the two package workflows must pass the leg rules",
            clean.stdout + clean.stderr,
        )
        cases_run += 1

        case_dir = Path(tmp) / "install-leg-removed"
        shutil.copytree(base, case_dir)
        nightly = case_dir / ".github" / "workflows" / "nightly.yml"
        text = nightly.read_text(encoding="utf-8")
        producer = (
            '          echo "install=$(uv run --no-project scripts/platform_matrix.py '
            '--matrix install)" >> "$GITHUB_OUTPUT"\n'
        )
        _expect(
            producer in text,
            "the nightly fixture no longer generates the install matrix",
        )
        nightly.write_text(text.replace(producer, "", 1), encoding="utf-8")
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1
            and "--matrix install" in res.stdout
            and "needs.matrix.outputs.install" in res.stdout,
            "deleting the `--matrix install` producer line by hand must fail the audit",
            res.stdout + res.stderr,
        )
        cases_run += 1

        case_dir = Path(tmp) / "install-leg-hardcoded"
        shutil.copytree(base, case_dir)
        packages = case_dir / ".github" / "workflows" / "packages.yml"
        packages.write_text(
            packages.read_text(encoding="utf-8")
            + "\n  legacy_install:\n"
            + "    runs-on: ubuntu-24.04\n"
            + "    steps:\n"
            + "      - run: docker run --rm debian:stretch dpkg -i blastit.deb\n",
            encoding="utf-8",
        )
        res = _run_auditor(case_dir)
        _expect(
            res.returncode == 1
            and "debian:stretch" in res.stdout
            and "second copy" in res.stdout,
            "a distribution written into a package workflow must fail the audit",
            res.stdout + res.stderr,
        )
        cases_run += 1

        for family, expected in (("install", "12"), ("packages", "4")):
            res = subprocess.run(
                [sys.executable, str(PLATFORM_MATRIX), "--count", family],
                capture_output=True,
                text=True,
                check=False,
            )
            _expect(
                res.returncode == 0 and res.stdout.strip() == expected,
                f"--count {family} must be {expected}",
                res.stdout + res.stderr,
            )
            cases_run += 1

        generated = subprocess.run(
            [sys.executable, str(PLATFORM_MATRIX), "--matrix", "install"],
            capture_output=True,
            text=True,
            check=False,
        )
        _expect(
            generated.returncode == 0,
            "the install matrix must generate",
            generated.stderr,
        )
        legs = {leg["name"]: leg for leg in json.loads(generated.stdout)["include"]}
        _expect(
            len(legs) == 12
            and legs["debian-13-x86_64"]["image"] == "debian:trixie"
            and "wsgi" in legs["debian-13-x86_64"]["modes"]
            and legs["fedora-aarch64"]["installer"] == "dnf",
            "the install matrix must carry both architectures and the WSGI leg",
            generated.stdout,
        )
        cases_run += 1

        # A leg that asks an image without CPython 3.13 for a WSGI import is a
        # promise the distribution cannot keep, so `--check` refuses it.
        matrix_fixture = _copy_matrix_fixture(Path(tmp) / "wsgi-leg")
        pm_file = matrix_fixture / "scripts" / "platform_matrix.py"
        text = pm_file.read_text(encoding="utf-8")
        anchor = (
            '        "image": "ubuntu:24.04",\n'
            '        "format": "deb",\n'
            '        "installer": "apt",\n'
            '        "modes": "install static",\n'
        )
        _expect(anchor in text, "the matrix fixture no longer carries ubuntu:24.04")
        pm_file.write_text(
            text.replace(
                anchor, anchor.replace('"install static"', '"install static wsgi"'), 1
            ),
            encoding="utf-8",
        )
        res = _run_matrix(matrix_fixture, "--check", "--quiet")
        _expect(
            res.returncode == 1 and "runs the WSGI gate in ubuntu:24.04" in res.stderr,
            "a WSGI leg on an image without CPython 3.13 must be refused",
            res.stderr,
        )
        cases_run += 1

        # MAJOR 1 of the review: `gh workflow run packages.yml --ref v0.2.0`
        # reports `ref_type=tag`, so a `ref_type`-only test publishes from a
        # dispatch. The **shipped** plan step is executed here, four ways.
        plan = _workflow_step_script(
            REPO / ".github" / "workflows" / "packages.yml", "plan"
        )
        base = subprocess.run(
            [
                sys.executable,
                str(REPO / "scripts" / "publish_packages.py"),
                "base-version",
            ],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        for event, ref_type, publish_input, expected in (
            ("push", "tag", "", "true"),
            ("workflow_dispatch", "tag", "", "false"),
            ("workflow_dispatch", "tag", "true", "true"),
            ("workflow_dispatch", "branch", "", "false"),
        ):
            output = (
                Path(tmp) / f"plan-{event}-{ref_type}-{publish_input or 'unset'}.txt"
            )
            output.write_text("", encoding="utf-8")
            res = subprocess.run(
                ["bash", "-euo", "pipefail", "-c", plan],
                cwd=REPO,
                capture_output=True,
                text=True,
                check=False,
                env={
                    **os.environ,
                    "GITHUB_OUTPUT": str(output),
                    "EVENT_NAME": event,
                    "REF_TYPE": ref_type,
                    "REF_NAME": f"v{base}" if ref_type == "tag" else "develop",
                    "PUBLISH_INPUT": publish_input,
                },
            )
            written = output.read_text(encoding="utf-8")
            _expect(
                res.returncode == 0
                and f"should_publish={expected}" in written
                and f"tag=v{base}" in written,
                f"the shipped plan step must publish={expected} for "
                f"{event}/{ref_type}/publish={publish_input or 'unset'} "
                "(a dispatch on a tag may never publish by accident)",
                written + res.stdout + res.stderr,
            )
            cases_run += 1
    return cases_run


def _derived_floor(*args: str, **overrides: str) -> subprocess.CompletedProcess[str]:
    """`build-wheel.sh --print-glibc-floor` with no ambient floor override.

    The two variables an ambient shell could set are dropped, so a case measures
    the parser and not this machine's environment.
    """
    env = {
        key: value
        for key, value in os.environ.items()
        if key not in ("BLASTIT_MANYLINUX", "BLASTIT_WHEEL_MAX_GLIBC")
    }
    env.update(overrides)
    return subprocess.run(
        ["bash", str(BUILD_WHEEL), *args],
        capture_output=True,
        text=True,
        env=env,
        check=False,
    )


def test_wheel_floor_cases() -> int:
    """`MB.42.13`: the glibc floor is parsed out of the platform, never stripped.

    maturin 1.16 tags a `bindings = "bin"` wheel `manylinux_2_39_x86_64`, so
    `${tag#manylinux_}` was `2_39_x86_64` and the floor parser refused the whole
    build — with the message blaming the shim it had just measured. Every case
    here runs the **shipped** `manylinux_floor` through
    `build-wheel.sh --print-glibc-floor`, so the parse under test is the one a
    release uses:

    * `manylinux_2_39_x86_64` → `2_39`, `manylinux_2_28_aarch64` → `2_28`: the
      architecture suffix is not part of a floor;
    * a newer pip's `manylinux_2_39_x86_64_glibc_2_39` → `2_39`: neither is the
      libc suffix;
    * `musllinux_1_2_x86_64` and an unparseable string fail loudly, naming the
      platform, instead of handing a malformed floor to `--max-glibc` (whose
      refusal then read as a shim that outgrew the floor);
    * `--manylinux` and `BLASTIT_WHEEL_MAX_GLIBC` still win when given, so the
      floor is not weakened by the derivation.
    """
    cases_run = 0
    for platform, expected in (
        ("manylinux_2_39_x86_64", "2_39"),
        ("manylinux_2_28_aarch64", "2_28"),
        ("manylinux_2_39_x86_64_glibc_2_39", "2_39"),
        ("manylinux_2_17_aarch64_gnu", "2_17"),
    ):
        res = _derived_floor("--print-glibc-floor", platform)
        _expect(
            res.returncode == 0 and res.stdout.strip() == expected,
            f"{platform} must derive the floor {expected!r}, not its arch suffix",
            res.stdout + res.stderr,
        )
        cases_run += 1

    for platform, needle in (
        ("musllinux_1_2_x86_64", "names a musl version, not a glibc floor"),
        ("manylinux_x86_64", "cannot derive a glibc floor"),
        ("not-a-platform", "cannot derive a glibc floor"),
    ):
        res = _derived_floor("--print-glibc-floor", platform)
        _expect(
            res.returncode == 1
            and platform in res.stderr
            and needle in res.stderr
            and "outgrew" not in res.stderr,
            f"{platform} must fail loudly, naming the platform and not a shim: "
            f"{needle!r}",
            res.stdout + res.stderr,
        )
        cases_run += 1

    res = _derived_floor(
        "--manylinux", "2_28", "--print-glibc-floor", "manylinux_2_39_x86_64"
    )
    _expect(
        res.returncode == 0 and res.stdout.strip() == "2_28",
        "--manylinux must still win over the platform it chose the tag for",
        res.stdout + res.stderr,
    )
    cases_run += 1

    res = _derived_floor(
        "--print-glibc-floor",
        "manylinux_2_39_x86_64",
        BLASTIT_WHEEL_MAX_GLIBC="2_17",
    )
    _expect(
        res.returncode == 0 and res.stdout.strip() == "2_17",
        "BLASTIT_WHEEL_MAX_GLIBC must still win over the parsed platform",
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
    pe = test_portable_elf_shim_cases()
    pg = test_portable_elf_glibc_cases()
    rp = test_repack_cases()
    mp = test_matrix_producer_cases()
    pk = test_package_workflow_cases()
    wfl = test_wheel_floor_cases()
    # The publishing suite lives in its own module (a 2 700-line gate is where a
    # case hides); this file stays its entry point, so one command still runs all.
    sys.path.insert(0, str(REPO / "scripts"))
    try:
        import test_publish_gates
    finally:
        sys.path.remove(str(REPO / "scripts"))
    pub = test_publish_gates.cases()
    total = wf + pm + wt + ee + ts + pe + pg + rp + mp + pk + wfl + pub
    print(
        f"packaging-gates-test: ok — {total} cases passed "
        f"(workflow={wf}, matrix={pm}, wheel-tag={wt}, exec-env={ee}, test-seam={ts}, "
        f"portable-elf={pe}, max-glibc={pg}, repack={rp}, matrix-producer={mp}, "
        f"package-workflow={pk}, wheel-floor={wfl}, publish={pub})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
