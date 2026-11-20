# End-to-end tests

`run_apps.py` is the application and runtime suite: a real PostgreSQL 18, a real
PHP front controller, Django and Flask on CPython 3.13 and 3.14, and a static
site in a container with **neither Python nor PHP installed**.

```console
$ uv run --no-project e2e/run_apps.py        # every leg; free ports 28080+
$ uv run --no-project e2e/run_apps.py --keep # keep the work directory
```

It checks the deploy pages' pooler promises first (`docs_deploy.py`), builds the
fixtures it needs (`e2e/apps/`), starts one `postgres:18`, and prints one
`PASS`/`FAIL`/`SKIP` line per leg. A missing host fact (Docker, a CPython minor,
the shims) is a loud `SKIP` naming the command that installs it — never a silent
pass. The binary under test is `--binary`, `$E2E_BLASTIT`,
`target/release/blastit`, `target/debug/blastit`, or `PATH`; it is mounted into
each container rather than rebuilt there.

## The fixtures

| Path | What it serves |
|---|---|
| `apps/php/` | A Symfony-shaped `public/index.php` with `public/assets/`: a public catalogue read, a parameterised order write, routes that run SQL no view was granted (a cross-table read, two `INSERT`s), and `Blastfile.splitstore` for the `$DB_*` spelling. `Blastfile` is the shop; `Blastfile.php2shell` is the store-less probe manifest. |
| `apps/python/django/` | A Django project using raw cursors, `manage.py collectstatic` into `staticfiles/`, `dj_database_url.config(conn_max_age=0)`, a shared `connection` trait, and the same refusals (cross-table read, customers insert, order insert with an ungranted `note`). |
| `apps/python/flask/` | The same shop through SQLAlchemy with `NullPool`, whose own connection statements (including psycopg's `hstore` lookup) and `BEGIN`/`COMMIT` are the shared trait. |
| `apps/static/site/` | A static + SPA mount: `immutable "/assets/*"`, `fallback "index.html"`, `ETag`/`304`, and the per-class refusals (`..`/NUL/backslash → `400`; dotfile, symlink, `.php` → `404`). |
| `apps/static/nospa/` | The same mount without a `fallback`: a deep path is a 404. |
| `apps/postgres/schema.sql` | The one schema every leg reads; applied with the real credentials, outside blastit. |

## What the legs prove

- **PHP (8.4):** the public read works, a parameterised `WHERE id = ?` and a
  parameterised `INSERT` run, an ungranted cross-table `SELECT`, an ungranted
  `INSERT` into `customers` and an order `INSERT` naming the ungranted `note`
  column are refused with `403` and SQLSTATE `42501`, `exit()` and a fatal error
  end only their own request, and the `php2shell` matrix (`e2e/php2shell.py`) is
  neutral. A second container runs the same views from `$DB_HOST`/`$DB_PORT`/
  `$DB_USERNAME`/`$DB_PASSWORD`/`$DB_DATABASE` and proves the worker sees no
  password and no rendered URL.
- **Django and Flask (CPython 3.13 and 3.14):** the catalogue and the order
  write work through each framework's own parameter binding; each view runs only
  its declared statements; the same five refusals answer `403` with SQLSTATE
  `42501`; the rendered URL uses the libpq `host` query parameter (a wrong host
  is a 500 for SQLAlchemy); the static mount (Django's `collectstatic` output,
  Flask's `static/`) is served with the year-long cache; and every refusal
  produces a `sql.denied` event without leaking the database password.
- **Static (Debian trixie, no interpreter):** `GET /` carries an `ETag` and
  `If-None-Match` answers `304`; a hashed asset is `immutable`; a deep history
  path is the `index.html` fallback with `no-cache`; `..` (plain and
  percent-encoded), NUL, a backslash and an escape attempt are `400`; a real
  `.git/`, `.env`, out-of-root symlink and `index.php` are `404`; a secret file
  is served only at its own path; `HEAD` works; the printed banner names the
  mount as the page shows it; `blastit check --json` exits `0`; a mount without
  `fallback` answers a deep path with `404`; and the image has no `python3` or
  `php` to fall back to.

Range/`206` requests are deliberately **not** asserted here: they are open human
item `H-017`.

Every leg uses ports from `28080`, writes its scratch trees under
`${TMPDIR:-/tmp/opencode}/blastit-e2e-apps`, and removes only the containers,
images, network and volumes it created — it never touches a foreign container.

## Other harnesses

These stay beside the app suite for the slices that own them:

| File | What |
|---|---|
| `universal_binary.py` | One staged bundle, four runtimes (`make e2e-universal`). |
| `php2shell.py` | The CGI/FastCGI refusal matrix; replayed by the PHP leg and on its own. |
| `php_sapi_probe.py` | The shim's real-load probe (`make php-sapi-e2e`). |
| `docs_deploy.py` | The deploy pages' pooler promises; a leg of `run_apps.py`. |
| `venv-smoke*`, `prebuilt_wheel.py`, `packages/` | Packaging and wheel checks. |

The `php2shell` driver is stdlib-only and takes the fixture, probe matrix, docs
page and port explicitly; the PHP leg runs it inside the `libphp` container so
the page's `127.0.0.1` commands are verbatim.

The production/deployment harness is `run_prod.py` in the next slice
(`MB.56.4.2`); this suite covers the application and runtime stories only.
