# MB.45 — stage the four PHP SAPI shims into the universal wheels

## What changed

- **`scripts/check_wheel_shims.py` (new, stdlib, PEP 723 header).**
  `check_wheel_shims.py [--require {py,all}] [--libc {gnu,musl}] wheel...` reads
  each wheel with `zipfile` and asserts the payload is *in the archive*: the two
  CPython shims always, plus `php83`–`php86` for `all`. With `--libc` the shim
  must be this wheel's own `(arch, libc)` tuple (arch from the
  `py3-none-<platform>_<arch>.whl` name), and a filename naming none of the
  shipped arches is **refused** rather than degraded to `-<libc>.so`. The tag
  lists are derived from `scripts/platform_matrix.py` (`ARCHES`, `PY_MINORS`,
  `PHP_MINORS`), so a new PHP/CPython minor moves this gate with the matrix. The
  remedy is branched per kind: a `py*` miss names the wheel builder
  (`make py-sapi`, the builder image), a `php*` miss names `--shims-from`.
- **`.github/actions/build-wheel/action.yml`:** the hand-rolled `zipinfo` step is
  replaced by one call to that script (`--require "${{ inputs.require-shims }}"`,
  `--libc gnu`), so the promise is read back out of the finished zip on every leg
  — the two CPython shims are asserted for the `py` default too.
- **`.github/workflows/publish-pypi.yml`:** `wheels_gnu`/`wheels_musl` need
  `shims`, download `pattern: shim-${{ matrix.name }}-*` into `dist/shims` and ask
  for `require-shims: all` (+ `shims-from: dist/shims`; the musl leg passes
  `--require-shims all --shims-from dist/shims`). `verify` runs
  `check_wheel_shims.py --require all dist/*.whl` right after the `wheel-*`
  download (with `setup-uv` above it) — both libcs proven before `pypi`, and no
  extra `download-artifact`, so rule 9 is untouched.
- **`scripts/build-musl-wheel.sh`:** the argument loop moved above the `apk`
  guard; `--require-shims py|all` and `--shims-from DIR` (both spellings) are
  this script's own, and `build_wheel_command` assembles one `wheel_cmd` array
  that `--print-build-wheel-args` **prints** and the build path **executes** —
  deleting the execution, or a flag, now fails the case. After the repack it runs
  `check_wheel_shims.py --require "$require_shims" --libc musl "$wheel"`, and an
  unknown promise (`none`, empty) is `exit 2`, never a warning.
- **`scripts/check_workflows.py`:** rule 22 (structural, over `step["with"]`),
  documented in the docstring list. A step calling the local `build-wheel` action
  must not stage `shims-from` without `require-shims: all`, must not promise `all`
  without staging, and the action's `check_wheel_shims.py` step — expanded from
  the call site with `expand_action_inputs`, defaults folded in — must demand the
  same promise **and** a non-empty `--libc`, which must be `gnu` whenever the
  caller builds in `manylinux`. Rule 9's clause now names the wheel case.
- **`scripts/test_packaging_gates.py`:** `test_wheel_shims_cases()` (six cases on
  real zips: six-shim pass, missing `php85` → 1 naming it and `--shims-from`,
  foreign `aarch64` shim in an `x86_64` wheel → 1 with the tuple message,
  `--require py` on a py-only wheel → 0, nonexistent path → 1, `--libc` on an
  arch-less filename → 1); case 20 (download↔`needs:` coupling only, comment
  corrected), case 21 (`require-shims: all` removed from `wheels_gnu`), case 22
  (`--libc gnu` removed from the action), and two `build-args` cases asserting
  the printed `bash scripts/build-wheel.sh …` command.
- **`CHANGELOG.md`:** the PHP bullet no longer says the shims ship only in the
  archives: a wheel carries the four PHP shims of its own `<arch>`/`<libc>` beside
  the CPython ones, and the archives carry all `16` (claim kept true, no `_bin/`
  filenames).
- **`RELEASING.md`:** the wheel clause says a short shim leg skips the release
  wheels instead of publishing one without PHP, and the development-wheel section
  states a `develop` wheel carries the two CPython shims only (`require-shims: py`).

## Evidence

```
$ make workflow-check
workflow-check: rule 21 self-test ok — the two package graphs must not drift
workflow-check: ok — 697 checks over 9 workflows and 1 actions, 0 problem(s)
```

```
$ make packaging-gates-test
publish gates: rpmdev-vercmp skipped (rpm not installed)
packaging-gates-test: ok — 128 cases passed (workflow=22, matrix=15, wheel-tag=4,
  wheel-shims=6, exec-env=12, test-seam=7, portable-elf=2, max-glibc=4, repack=13,
  matrix-producer=2, package-workflow=11, wheel-floor=9, build-args=2, publish=19)
```

`workflow=22` holds case 22 (rule 22's tuple-pin negative) and `wheel-shims=6` the
new gate's own cases; the clean baseline still passes.

Lead verification on the final tree (base `2f2a0d9`): `make fast-check` passed end
to end, `uv run --no-project scripts/platform_matrix.py --check` ok (4 wheels, 4
archives, 16 shims, 8 python shims, 5 lsp assets), `tools/critical_guard.py`
reports no change to a protected critical test, `tools/guard.py` OK and
`tools/plan.py validate` OK.

The acceptance bullet "the built wheel's `zipinfo` lists the four PHP shims" is a
**CI-time** fact: a worktree has no distro `php_config.h`, so only the `shims`
legs produce those four files. It is now enforced, not asserted by hand — every
leg reads its own finished zip back through `scripts/check_wheel_shims.py`, and
`verify` refuses all four wheels before any upload. Against the real
`dist/blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl` of this tree the gate
passes with `--require py` (py313 + py314 `-x86_64-gnu.so`) and fails with
`--require all` naming each missing `php8*`, which is exactly what a wheel built
before this node would do.

## Follow-ups

- **A gnu wheel asserts one floor over shims declared against another.** The
  wheel's embedded ELFs are measured against `--max-glibc 2_28`
  (`scripts/build-wheel.sh`'s post-repack loop), while the PHP shims' declared
  ceiling is `shim_floor = 2_34` (`scripts/platform_matrix.py`) for the `.deb` /
  `.rpm` bundle, built on `debian:bookworm-slim`, `ubuntu:24.04`,
  `debian:trixie-slim` and `ubuntu:26.04`. Today's shims measure 2.17, so the leg
  passes; a PHP build that lands between 2.28 and 2.34 would fail the `wheels_gnu`
  leg alone (the package bundle would still accept it), so the two numbers are one
  policy decision that has not been made.
- `.github/workflows/wheels.yml` intentionally has no shim legs: its wheels keep
  the two-CPython-shim promise (`require-shims: py`), and only a release wheel
  carries PHP.
