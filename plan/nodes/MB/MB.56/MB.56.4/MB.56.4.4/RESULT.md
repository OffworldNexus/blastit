# MB.56.4.4 — wire the workflows to the §2/§3/§6 stories

## What changed

- **`ci.yml`** — the host-tree `smoke (root)` job and the duplicate `docs-gate`
  job are gone (the containerized `run_prod.py` is the privileged witness;
  `docs.yml` owns the pages). The R42 guard stays as the cheap `critical-guard`
  job (`make critical-skip-check`), and a **bounded** `root-only` job runs one
  test binary as uid 0 with `BLASTIT_REQUIRE_ROOT_TESTS=1`, so
  `e2e_cli`'s `a_binary_owned_by_another_uid_is_refused` can no longer pass as a
  `SKIP`. The surviving jobs are named for the story they prove, and the
  `§2 apps` job now tees its log and **fails on `SKIP`** unless every
  Django/Flask/PHP/static leg printed `PASS`.
- **`php-sapi.yml`** — job names carry `§2`; the matrix still declares PHP
  8.3/8.4/8.5/8.6 × `x86_64`/`aarch64` × `gnu`/`musl` (16 legs), the boot-level
  `e2e_dev` PHP legs run per leg and the guard fails the job on `SKIP`.
- **`py-sapi.yml`** — new `wsgi` job (`§2 WSGI views`) runs
  `e2e/run_apps.py --legs wsgi` on the CPython minors the matrix declares, with a
  guard that fails on `SKIP`; shim legs keep their ELF-contract proof lines.
- **`nightly.yml`** — new `prod` job (`§6 production presets`) runs
  `e2e/run_prod.py` (both presets, real containers) with the same `SKIP` guard.
- **`packages.yml`** — new `user-space` job (`§3 install.sh --user` +
  `composer` wrapper) on each architecture's native runner; the package jobs are
  named by story; `publish` now needs `user-space`.
- **`publish-pypi.yml`** — new `install-stories` job runs
  `run_packaging.py --leg install` and `--leg alpine` against the release
  archives; the `validate` job no longer waits for a `.github/workflows/live.yml`
  that does not exist (its `needs` set is now `CI` + `Docs`).
- **`wheels.yml` / `docs.yml` / `ci.yml`** — every wheel smoke is
  `e2e/run_packaging.py --leg wheel` (including `.github/actions/build-wheel`),
  so no workflow keeps a second copy of the install commands; `docs.yml` owns the
  page/nav check, the strict build, `make docs-unit` and the served `install.sh`.
- **`e2e/run_apps.py`** — `--legs wsgi|php|static` selects a family (the WSGI job
  runs one family, not the whole suite).
- **`e2e/run_packaging.py`** — the `wheel` leg installs into the images its own
  matrix leg declares (`runtime_images`: `*-slim` for manylinux, `*-alpine` for
  musllinux) and installs `libgcc` in the Alpine images, so one driver proves all
  four wheel legs. `--require-legs` makes any `SKIP` a failure, and every CI call
  site passes it (`ci.yml`'s wheel, the `build-wheel` action, `wheels.yml`'s musl
  leg, `packages.yml`'s `install`/`composer` steps, `publish-pypi.yml`'s
  `install`/`alpine` steps), so no packaging gate can exit 0 having skipped.
- **`Makefile`** — `packages-test` now builds its `--leg … --deb/--rpm` argument
  list in the shell (`set --`, with `--require-legs`) instead of expanding empty
  make variables, which had silently run all six legs with no artifact.
  `PACKAGES_OUT` is `target/packages` (the directory `discover()` and
  `e2e/README.md` name, so `make packages` is a working recovery path), staging
  stays under `${TMPDIR:-/tmp}/opencode`; `make help`'s `e2e` line names the §2
  application suite.
- **`scripts/platform_matrix.py`** — `--matrix hosts` / `--count hosts`: the
  native runner per architecture, so `packages.yml`, `py-sapi.yml`,
  `publish-pypi.yml` and `nightly.yml` spell no runner name.
- **Docs** — `e2e/README.md` gained a story→workflow table, documents `--legs`
  and `--require-legs`, and names `target/packages` as where `make packages`
  writes; `docs/testing/README.md` gained the same table with each suite's story
  and CI cadence (PR / nightly / tags) and no longer calls `make e2e` the
  multi-UID suite; `docs.yml` /
  `publish-pypi.yml` header comments no longer cite the retired jobs.

## Verification

```console
$ uv run --no-project --with pyyaml python -c '…parse + needs + paths…'
parsed .github/workflows/ci.yml jobs: lint, test, msrv, e2e, standalone, matrix, lsp, wheel-smoke
parsed .github/workflows/docs.yml jobs: build, deploy
parsed .github/workflows/nightly.yml jobs: stamp, matrix, prod, shims, py_shims, packages, install, publish, prune
parsed .github/workflows/packages.yml jobs: plan, matrix, shims, py_shims, packages, install, user-space, publish, release
parsed .github/workflows/php-sapi.yml jobs: matrix, lint, legs
parsed .github/workflows/publish-pypi.yml jobs: guard, matrix, shims, binaries_gnu, binaries_musl, lsp, wheels_gnu, wheels_musl, install-stories, sdist, verify, validate, testpypi, pypi, release
parsed .github/workflows/py-sapi.yml jobs: matrix, lint, legs, wsgi
parsed .github/workflows/wheels.yml jobs: matrix, wheels_gnu, wheels_musl
# no "PROBLEM:" line: every needs: target exists and every scripts/ e2e/ … path exists

$ uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared; …
$ uv run --no-project scripts/platform_matrix.py --matrix hosts
{"include":[{"name":"x86_64",…,"runs_on":"ubuntu-24.04"},{"name":"aarch64",…,"runs_on":"ubuntu-24.04-arm"}]}

$ make fast-check          # exit 0 (tail: install-test: all cases passed)
$ make docs-check          # exit 0 (zensical: No issues found)
```

## Follow-ups

- `scripts/wheel-smoke.sh` / `scripts/wheel_container_smoke.sh` remain for the
  developer `make wheel-smoke` target; no workflow calls them any more.
- `docs/testing/AUDIT-*.md` still quote the retired `smoke (root)` /
  `pytest-plugin` jobs as history; audits are dated records and were left as-is.
