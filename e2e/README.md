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
| `apps/prod/` | The production fixture `run_prod.py` serves: `/whoami` (uid, client address, scheme), `/fast` and `/slow` (fair queuing and drain), `/catalog/*` and `/warehouse/*` (one grant per view), `/env`, `/shell` and `/egress`. `Blastfile` is the **block** store the DigitalOcean page writes (`$DB_*`); `Blastfile.url` is the **URL** store the Compose page writes; `Dockerfile.rootless` adds a `0500` capable `blastit-init` at `/usr/share/blastit/lib`, beside the shim. |
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

## The production suite

`run_prod.py` is `TESTING_PLAN.md` §6: both deployment presets, in real
containers, against the same binary.

```console
$ make e2e-prod                                # the target below
$ uv run --no-project e2e/run_prod.py          # both legs; free ports 28080+
$ uv run --no-project e2e/run_prod.py --keep   # keep the work directory
```

- **`root preset`** (`--preset digitalocean-paas`, `apps/prod/Blastfile`, the
  **block** store: `DB_HOST`/`DB_PORT`/`DB_USERNAME`/`DB_DATABASE`/`DB_PASSWORD`,
  no `DATABASE_URL`) runs blastit as container root beside `postgres:18` and
  proves: the boot's `privilege: root` and the preset's settings; per-view OS
  uids ≥ 20000 from `process.spawned` and from `/proc/<pid>/status`; every child
  holding no capability with `NoNewPrivs: 1`; the view's `Seccomp: 2`; no
  database credential in any `/proc/<pid>/environ` **and** a scan that
  demonstrably reads a view (it finds the view's own `DB_HOST`); `exec` and an
  undeclared `fsockopen` refused in the view while the same calls work through
  `docker exec`; a trusted `X-Forwarded-Proto: https` reaching the view; one
  grant per view (the catalogue view refused `orders` and the warehouse view
  refused `products`, both SQLSTATE `42501`); fair queuing answering a second
  client's fast request ahead of a slow client's backlog *that has demonstrably
  queued* (`queued_ms`); and `SIGTERM` finishing a request whose view is in
  `/proc/<pid>/stat` state `S` before the container exits `0`.
- **`rootless preset`** (`BLASTIT_PRESET=compose-traefik`, `Blastfile.url`, the
  **URL** store: `DATABASE_URL=postgresql://…@pgbouncer/…`) builds one image
  from `apps/php/Dockerfile` and a second from
  `apps/prod/Dockerfile.rootless` (the binary at `/usr/share/blastit/lib`,
  `setcap` on the `0500` copy beside it), then proves: `privilege: capabilities`
  and the seven-capability list; nothing capable on the shared binary, the
  shim the runtime actually `dlopen`s or `python3`; uid `10001` refused that copy
  while `10000` runs it; zero capabilities and `NO_NEW_PRIVS` in every child; the
  URL store rendered as a socket with no password; a trusted
  `X-Forwarded-For`/`X-Forwarded-Proto` believed on a direct connection and a
  forged chain ignored through Traefik; the same refusals and sandbox as the
  root leg; PgBouncer on the path (the app fails with `pg.refused` while it is
  stopped and recovers); fair queuing through the in-container client; a renamed
  or rewritten PHP entry refused before `libphp` opens it; and `SIGTERM`
  draining `blastit-init` with the in-flight request answered.

Only the containers, volume, network and images this run created are removed
(`blastit-prod-*`, then `docker rmi`). Docker, a binary, `postgres:18`,
`traefik:v3.7.16` and `edoburu/pgbouncer` are pulled or skipped loudly with the
command that installs them; **the rootless image build is a FAIL, not a SKIP**,
when the daemon cannot store file capabilities in an image layer (rootless
Docker, or a storage driver without `security.capability` xattrs), because that
is the deployment promise itself.

Three harness facts worth knowing before reading a failure:

* the scratch tree is `${TMPDIR:-/tmp/opencode}/blastit-e2e-prod` (not the app
  suite's `blastit-e2e-apps`);
* the root container runs with `--cap-add SYS_PTRACE` **only** so the harness can
  read a `PR_SET_DUMPABLE(0)` view's `/proc/<pid>/environ`; the children still
  drop every capability, and the leg proves the scan works by finding the
  view's own `DB_HOST` before it asserts the password is absent;
* `BLASTIT_MEM_HIGH_WATERMARK=100` is a *percentage of total memory*, so at
  `100` the watermark is effectively disabled — deliberate here, because the
  runner's page cache is not "memory in use" (see `docs/knowledge/LESSONS.md`).

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
