# Gate: MB.56.4 — container & multi-platform E2E consolidation

Base `dea33ad` → `fab30f9`. Four sub-slices; full `make check` run **once** here (plus the local Docker suites).

## Evidence

```
make check                       → EXIT=0
make e2e-universal               → 6 passed, 0 skipped, 0 failed (one artifact, four runtimes)
uv run e2e/run_apps.py           → 10 passed, 0 failed, 0 skipped
uv run e2e/run_prod.py           → 7 passed, 0 failed, 0 skipped
uv run e2e/run_packaging.py      → 4 PASS / 3 loud SKIP (deb proven; rpm needs rpmbuild)
```

## What changed

| Before | After |
|---|---|
| `e2e/run.py`, `scenario.py`, `grants.py`, `check_logs.py`, `sqlalchemy*`, `static_*`, `demo/`, `php/` | `e2e/apps/` (PHP shop, Django, Flask, static/SPA) + `e2e/run_apps.py` |
| `e2e/rootless/*`, `e2e/live.py`, `live.yml` | `e2e/run_prod.py` (root + rootless presets, isolation, fair queuing, drain) |
| `e2e/packages/*`, `prebuilt_wheel.py`, `venv-smoke*`, `scripts/test_packages.sh`, `composer_smoke.sh`, two dead php-sapi runners | `e2e/run_packaging.py` (deb/rpm/install.sh/musl/wheel/composer) |
| host-tree `smoke (root)`, `guards`, `pytest-plugin` jobs | story-named jobs: §2 apps on PR, §3 packaging and §6 presets nightly/tag, R42 `critical-guard`, bounded `root-only`, 16 PHP matrix legs failing on skip |

Real defects the new oracles found and forced fixes for: the two presets proved the *other*
page's store shape; `docs/security.md` named two deleted harnesses; `make packages-test`
passed no artifact; `e2e/universal_binary.py`'s PHP leg pointed at a deleted fixture;
five packaging call sites could exit 0 having proved nothing (`--require-legs` now fails
on a skip); and the R42 skip ban plus the root-only assertion had vanished from CI.

Documented concession: a local stack cannot hold a cluster CA, so `verify-full` + the
the 25061 pool of the DigitalOcean recipe are recorded, not mirrored.
