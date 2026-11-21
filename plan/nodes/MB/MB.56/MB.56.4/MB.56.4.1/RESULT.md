# RESULT — MB.56.4.1 (e2e/apps + run_apps.py)

## What shipped

`e2e/run_apps.py` is the application/runtime end-to-end suite: the deploy-pages
leg, one `postgres:18`, the PHP front controller in a Debian `libphp8.4`
container (URL and `$DB_*` split-store manifests), the Django and Flask shops on
the host's CPython 3.13 and 3.14, and a static + SPA leg in a Debian container
with no interpreter. It asserts both the user outcome and the boundary
(per-view grants, refused columns/statements including ungranted `INSERT`s, no
credential in a worker or a denial body, `exit()`/fatal recovery, `php2shell`,
static confinement), prints one `PASS`/`FAIL`/`SKIP` per leg, and removes only
the containers, images, network and scratch tree it created.

`uv run --no-project e2e/run_apps.py` (ports from `28080`, work under
`${TMPDIR:-/tmp/opencode}/blastit-e2e-apps`).

## Old → new map

| Old | New |
|---|---|
| `e2e/run.py` + `e2e/compose.yaml` + `e2e/Dockerfile{,.prebuilt}` | `e2e/run_apps.py` (direct Docker + host legs) |
| `e2e/scenario.py` (Django driver) | `e2e/apps/python/django/` + the `django 3.13/3.14` legs |
| `e2e/sqlalchemy/{app.py,Blastfile,setup.py}` + `e2e/sqlalchemy_check.py` | `e2e/apps/python/flask/` + the `flask 3.13/3.14` legs |
| `e2e/static_assets.py` + `e2e/static_site/` | `e2e/apps/static/{site,nospa}/` + the `static` leg (documented statuses, banner and `check --json` asserted) |
| `e2e/php/*` | `e2e/apps/php/` (`Blastfile`, `Blastfile.php2shell`, `Blastfile.splitstore`, probes, `entry_probe.py`, Dockerfile) |
| `e2e/grants.py` + generated `e2e/demo/Blastfile` | hand-owned manifests beside each app, checked with `blastit fmt --check` |
| `e2e/check_logs.py` | the legs read `sql.denied` from their own live JSON log |
| `e2e/app.log` | removed; logs live in the run's scratch tree |
| `e2e/live.py`, `e2e/rootless/**`, `.github/workflows/live.yml`, `ci.yml`'s rootless job | **deleted**; the production/deployment harness moves to `MB.56.4.2`'s `run_prod.py` |
| `Makefile` `e2e`, `ci.yml` `e2e` | build the release binary + CPython shims and run `e2e/run_apps.py` |
| `e2e/docs_deploy.py` (only `run.py` called it) | first leg of `run_apps.py` (`docs deploy`, 47/47) |

Kept: `e2e/universal_binary.py`, `e2e/php2shell.py` (now driven by the PHP leg,
with `--port` and a store-less probe manifest), `e2e/php_sapi_probe.py`,
`e2e/venv-smoke*`, `e2e/prebuilt_wheel.py`, `e2e/packages/`.

## Review fixes

- `e2e/live.py`, `e2e/rootless/**` and `live.yml` were **deleted**, not kept:
  they executed the deleted `scenario.py`/`check_logs.py` and a wheel-installed
  `blastit`, so no “kept” claim would have been true. The `Makefile` and `ci.yml`
  now build `target/release/blastit` + the CPython shims and run `run_apps.py`;
  the rootless/live jobs and artifacts are gone. The production stories move to
  `MB.56.4.2`'s `run_prod.py` (handoff, not silently dropped).
- Ungranted writes are now proven in all three fixtures: `GET
  /customers/insert` (a table with no insert grant), `GET /orders/note-insert`
  (from a view with no insert grant) and `POST /orders/note` (the granted
  `insert orders(customer_id, total)` with the ungranted `note` column). The
  apps answer `403` with SQLSTATE `42501`; `assert_denied` requires both, treats
  a transport error as a failure, and checks every denial body is
  credential-free.
- The PHP image builds its own shim from `crates/blastit-php-sapi/c`, so the
  leg no longer needs host `php-sapi`; the `$DB_*` split-store manifest runs a
  second container and proves `/products` is `200`, `/env` shows no rendered URL
  and `db_password_in_env=false`.
- Static fixtures are real (`.git/config`, `.env`, `secret`, `index.php`, an
  out-of-root symlink) and the leg asserts the documented per-class statuses
  (`..`/NUL/backslash → `400`; dotfile/symlink/`.php` → `404`), the page's
  banner, and `blastit check --json` → `0`. `Range`/`206` is **not** asserted:
  open human item **H-017**.

## Documentation contract

- `docs/frameworks/python/flask.md` now carries psycopg's `hstore` OID lookup in
  the `connection` trait, with the clause “or set `use_native_hstore=False`”.
- The Django fixture matches its page: `STATIC_ROOT = BASE_DIR/"staticfiles"`,
  `STATIC_URL = "/static/"`, `django.contrib.staticfiles`, a committed
  `manage.py`, `collectstatic --no-input` before `dev`, and
  `dj_database_url.config(conn_max_age=0)` + `server_side_binding`.
- The Flask fixture uses the page's `create_engine(url, poolclass=NullPool)` and
  `root "static"` with the asset under `static/`.

## Evidence

```console
$ uv run --no-project e2e/run_apps.py
PASS  docs deploy               documented pooler promises hold
PASS  binary                    …/target/release/blastit (16903664 bytes)
PASS  fmt --check               7 manifests are canonical
PASS  postgres                  postgres:18 on 127.0.0.1:28080 (schema applied)
PASS  django 3.13               django 3.13
PASS  flask 3.13                flask 3.13
PASS  django 3.14               django 3.14
PASS  flask 3.14                flask 3.14
PASS  php 8.4                   php 8.4
PASS  static                    static (no interpreter)

10 passed, 0 failed, 0 skipped (a SKIP names the host fact it needs)
```

The PHP leg also replayed `e2e/php2shell.py` inside the container (25 matrix
checks + the docs page), and every app leg asserted live `sql.denied` events for
the ungranted statements.

## Follow-ups (outside `e2e/**`, `Makefile`, `ci.yml`, `docs/frameworks/**`)

1. `AGENTS.md`, `docs-site-plan.md` and `docs/design/*` still mention
   `e2e/live.py`/`rootless`; those are documentation slices.
2. `e2e/packages/` and `venv-smoke*` remain packaging fixtures; the wheel/live
   packaging slices own them.
3. PHP minors 8.3/8.5/8.6 stay with `make php-sapi-e2e` and the `e2e_dev` PHP
   legs; this suite proves 8.4, the NTS `libphp` the image ships.
