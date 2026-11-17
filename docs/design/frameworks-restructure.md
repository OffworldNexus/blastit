---
status: Draft
owner: doc-writer
node: MB.20
reviewers: []
---

# Frameworks restructure: eight pages, one sentence each

Today `docs/frameworks/` is five flat pages filling the same nine numbered headings. Six of
those slots restate runtime mechanics, and the Database/Ingress/Secrets slots on
`static-and-spa.md` hold filler. This outline retires that shape: a chooser, one connector page
per runtime, then one page per framework that says only what that framework makes you write.
Nothing below is page prose yet; each `Says` cell is the section's single claim, and Phase B
turns it into the page.

## The reader's path

- **Django:** root table → `python/django.md` — one click, because the table links every page,
  not only the group.
- **Laravel:** root table → `php/laravel.md` — one click.
- **"How does the Python connector work?"** root table → `python/index.md` — the connector is
  beside the framework pages, never in the way of one.
- **"What does `immutable` mean?"** one link to the reference, from the page that uses it.

## The tree and its budgets

| Page | Lines | One job |
|---|---|---|
| `docs/frameworks/index.md` | ≤ 50 | choose a runtime |
| `docs/frameworks/python/index.md` | ≤ 90 | the Python (WSGI) connector |
| `docs/frameworks/python/django.md` | ≤ 110 | only what Django changes |
| `docs/frameworks/python/flask.md` | ≤ 110 | only what Flask, SQLAlchemy and raw WSGI change |
| `docs/frameworks/php/index.md` | ≤ 100 | the PHP (embedded `libphp`) connector |
| `docs/frameworks/php/symfony.md` | ≤ 100 | only what Symfony changes |
| `docs/frameworks/php/laravel.md` | ≤ 100 | only what Laravel changes |
| `docs/frameworks/static-and-spa.md` | ≤ 90 | static files and deep-URL SPAs |

Old paths `docs/frameworks/{django,flask,php}.md` are deleted, not kept as redirects; every
inbound reference is repointed in the same change.

## Page contracts

### `docs/frameworks/index.md` — ≤ 50

| Section | Says |
|---|---|
| *(intro)* | Blastit starts your app and calls it: pick the runtime you deploy, and read only what your framework changes. |
| `## Pick your runtime` | The table below names the one line each runtime writes and links the connector and every framework page, so every page is one click away. |
| `## Nine things to decide` | Every runtime answers these nine things, each answer has one home — or two where filling the file and proving it are different jobs — and a page below adds only where its framework changes the answer. |
| `## Roadmap` | Node and Tomcat join this tree as their connectors land, each with a connector page and one page per framework in this shape. |

The nine questions, as questions with a link to the one place that answers each: how the code
starts; where the built files live and what is cached for a year; how the app reaches
PostgreSQL without the credential; how a body reaches the code and who validates it; what
happens to environment variables and secrets; what answers the platform health check; where
schema changes run; how the grants are filled in and proven complete; which parts become their
own process and how many run at once.

Some questions need two homes. 5 points at `#stores` and settings `#the-app-and-the-file`; 8 at
`#what-learn-writes` for filling the grants and `ci.md#for-agents-json` for proving them, and says
in one clause that a PHP app has no request audit in 0.2.0; 9 at settings
`#privileges-and-the-uid-layout` for which parts are their own process and `#admission-and-threads`
for how many run at once. The table's page cells keep the full names — "Flask, SQLAlchemy and raw
WSGI", not "Flask" — so a Pyramid or Bottle reader recognises their page.

| Runtime | What you write | Pages |
|---|---|---|
| **Python (WSGI)** | `entrypoint python "module:attr"` | Connector · Django · Flask, SQLAlchemy and raw WSGI |
| **PHP (embedded `libphp`)** | `entrypoint php "public/index.php"` | Connector · Symfony · Laravel |
| **Static sites and SPAs** | `static "/" { root "public" }` | Static and SPAs |

**Not a legend.** No page uses these as headings, no page answers more than its own, and the
list carries no matrix and no default. That is the difference between a reader's checklist and
the retired nine-slot contract.

**Never on this page:** a runtime mechanic explained, a capability matrix, an executor snippet
beyond the table cell, "coming soon".

### `docs/frameworks/python/index.md` — ≤ 90

| Section | Says |
|---|---|
| *(intro)* | A Python app is one WSGI callable, `entrypoint python` names it, and `uv add blastit` adds the CLI while the app keeps its own driver. |
| `## The Blastfile` | Shows the ten-line stub — `entrypoint python "module:attribute"`, the optional `{ preload "<module>" }` block that imports each module once before the fork so every view shares the copy, one store and one view — and every CLI command here is `uv run blastit`. |
| `## One view, one worker` | Each view is its own sandbox with its own grants and its own worker process, so a hot or risky path gets its own `view` above the catch-all. |
| `## The DATABASE_URL your app receives { #bind-parameters-server-side }` | The app reads a local Unix-socket URL in `DATABASE_URL` and never the credential, parameters travel apart from the statement text so one template is one grant, and the statement to paste is the `sql` in an `add_query` finding. |
| `## Learn the grants` | `uv run blastit dev --learn` writes the views and exact `query` lines the run exercised, `uv run blastit check --learn --dry-run` prints the suite's proposal, `uv run blastit check --learn` writes a route for a path no view matches, a view with no `postgres` block has no socket so give every database view an empty `postgres {}` before the run, and none of it writes `body`, `use` or `transactions`, so read `git diff`, split a view whose paths need different grants, and run `uv run blastit fmt`. |
| `## Check before you commit` | `uv run blastit check` fails on a statement the suite reached that the file does not grant, on a grant the suite never used, on a view it never reached and on a request no view matched, and `--json` prints one `fix.action` per finding ([what each finding means](../../ci.md#what-it-fails-on)). |
| `## Where next` | Django and Flask. |

**Reference link:** `## The DATABASE_URL your app receives` → `../../reference/blastfile.md#grants`.

**Never on this page:** `preload`'s copy-on-write explanation beyond its one clause, any
framework setting (`collectstatic`, `create_app`, `atomic`), a PHP sentence, health, migrations,
worker caps.

### `docs/frameworks/python/django.md` — ≤ 110

| Section | Says |
|---|---|
| *(intro)* | A Django project is a WSGI callable; the [connector](index.md) starts it, and this file and the settings below are the whole adaptation, with `SECRET_KEY` still coming from the environment. |
| `## The Blastfile { #manifest }` | The working file — views, the six-pattern `static` mount, and the last view taking every path nothing else claimed — with `preload` naming the URLconf rather than the settings module, and a body-taking view declared above the catch-all because a body there is a `413`. |
| `## Static files` | Point `STATIC_ROOT` at the mount's `root` and run `uv run python manage.py collectstatic --no-input` once, because a missing root stops the boot; the six `/static/*…` patterns are one per depth, because `*` stops at `/`, and the deepest reaches the admin's `select2/i18n` files. |
| `## Database settings` | `uv add blastit 'psycopg[binary]' dj-database-url`, and the settings are `dj_database_url.config(conn_max_age=0)` plus `server_side_binding = True` in `OPTIONS`. |
| `## Transactions` | Django's `atomic()` and its savepoints need `transactions` in that view's grant, because the savepoint name is generated per request. |
| `## The admin` | The admin's SQL is table reads and writes, not `query` lines: put `view admin GET, POST "/admin/*"` above the catch-all with `body form`, `transactions` for its savepoints, and the `select`, `insert`, `update` and `delete` grant lines for `auth_user`, `auth_group`, `auth_permission`, `django_content_type`, `django_admin_log` and `django_session`, each naming at least one column, while a `pg_catalog` statement stays a `query`. |
| `## Behind TLS` | Do not set `SECURE_PROXY_SSL_HEADER`: Blastit sets `wsgi.url_scheme` from a trusted peer and the raw `X-Forwarded-Proto` still reaches the app, so trusting it again is the bypass; list the origin in `CSRF_TRUSTED_ORIGINS`, and list the host in `ALLOWED_HOSTS`. |
| `## Tests and migrations` | `uv add --dev pytest pytest-django` plus a `manage.py` (or `DJANGO_SETTINGS_MODULE`) is the handshake `check` uses to create `test_<database>`, one test per route because `check` exits `3` when the suite collects nothing, and `manage.py migrate` runs outside the view workers ([the rule](../../deploy/anything-else.md#migrations)). |
| `## Where next` | The Flask and SQLAlchemy page. |

**Reference links:** `## Static files` → `../../reference/blastfile.md#static-mounts`; `## Transactions` → `../../reference/blastfile.md#grants`.

The mount lives inside the page's one `text title="Blastfile"` document, so copying `## The Blastfile` gives a file that also serves the admin's CSS and JS; `docs_fences.rs` pins `docs/frameworks/python/django.md` as `Shape::ViewsAndMount` and reads the mount from that document (the separate statement-list test moves onto it).

The admin's table is written by hand from the reference's directives, not copied from a report:
`table_grants` is a JSON key of the pytest plugin's own report (the `blastit-report.json` document
its `--blastit-report` flag writes), and `blastit check` disables that plugin for its own
`_check_pytest`, so `check --json` never carries it.

**Never on this page:** Flask, PHP, health, secret stripping, `BLASTIT_MAX_WORKERS`, a generic
grant lecture, a `preload` mechanics paragraph.

### `docs/frameworks/python/flask.md` — ≤ 110

| Section | Says |
|---|---|
| *(intro)* | Flask is a decorator around a WSGI callable, a bare WSGI app drops it, SQLAlchemy changes two lines, and the [connector](index.md) has the runtime mechanics. |
| `## Install` | `uv add flask 'psycopg[binary]' blastit` supplies the app, and `uv add --dev pytest` the suite this page assumes. |
| `## The Blastfile` | The working file: the `postgres $DATABASE_URL` store, a `connection` trait carrying `transactions` and the setup statements, one form view with a grant above a catch-all that shows "no database here" and is not the shape for the app's other routes. |
| `## The callable you name` | `entrypoint python` names a module attribute, so an app built by `create_app()` needs a module-level `app = create_app()` for the file to name. |
| `## One connection per request` | A connection closed in `teardown_appcontext` cannot outlive the request that opened it. |
| `## SQLAlchemy` | `postgresql+psycopg://` selects psycopg 3, `NullPool` gives each request its own connection, and the lowercase `select pg_catalog.version()` probe and the `transactions` that `engine.begin()` needs belong in the one `connection` trait every view `use`s. |
| `## Proxy headers` | Nothing to configure: Blastit sets `REMOTE_ADDR` and `wsgi.url_scheme` from a trusted peer, so the app must not re-read `X-Forwarded-*` (`ProxyFix` would let a client choose both). |
| `## Tests` | `uv run blastit check` runs `pytest` from the project root, so the suite must import the same callable the file names. |
| `## Migrations` | `uv run flask db upgrade` applies what Flask-Migrate wrote into `migrations/`. |
| `## Where next` | Django, and static files for a built front end. |

**Reference link:** `## SQLAlchemy` → `../../reference/blastfile.md#connection-setup`.

**Never on this page:** Django, PHP, the `preload` mechanic, health, migrations mechanics, a
second way to pass the URL.

### `docs/frameworks/php/index.md` — ≤ 100

| Section | Says |
|---|---|
| *(intro)* | Blastit runs `public/index.php` inside each view worker: no PHP server beside it, no rewrite rules, one `entrypoint` line, and the host library is the only step that needs `sudo`. |
| `## The Blastfile` | The working file, with `entrypoint php` declared once at top level because every view reaches that one front controller, and a JSON API as its own view above the catch-all, so the catch-all's `form` codec never sees it. |
| `## Run it from an artifact that carries the shim` | Download `blastit-<arch>-unknown-linux-<libc>.tar.gz` from `github.com/OffworldNexus/blastit/releases`, extract it, put the `blastit-<arch>-unknown-linux-<libc>/` directory it creates on `PATH`, and run `blastit dev` from your project, because the shim has to sit beside the binary and the PyPI wheel ships none. |
| `## Install the host library` | Install the distro's NTS `libphp` for a supported minor (8.3–8.5) and the PDO driver on the host or CI runner — Debian 13 and Ubuntu 24.04+: `sudo apt install libphp-embed php-pgsql`; Alpine: `sudo apk add php84-embed php84-pdo_pgsql`; Fedora: `sudo dnf install php-embedded php-pgsql` — which is the one privileged step, because Blastit itself needs no `sudo` and writes nothing under `/etc`; Debian 12 and Ubuntu 22.04 ship 8.2 or older, which the boot refuses by design; `BLASTIT_LIBPHP` must be an absolute path to a library that lives elsewhere; an image you build adds the same library and the shim; and a boot without a matching library stops before it binds and names the package. |
| `## One request at a time` | Route uids are `BLASTIT_UID_BASE + 1000 + i` — `21000`, `21001`, … — each with `gid == uid` and no supplementary groups, so a directory a view writes must be owned by that uid or carry its ACL, and one worker runs one request at a time with no daemon, pool or socket between Blastit and PHP. |
| `## public/ is the static root` | A regular file under `public/` is served first, every other path reaches the front controller, `.php`, `.phtml` and `.phar` files are `404`, and a `.env` never ships inside it. |
| `## The database` | Blastit reads the real `DATABASE_URL` or `DB_*` from its own environment, hands the app a local socket URL or local `DB_HOST`, `DB_PORT`, `DB_USERNAME` and `DB_DATABASE` with `DB_PASSWORD` removed so the real password never enters the app's `.env`, and `$pdo->prepare()` binds a value only when the driver prepares it server-side, because an emulated prepare inlines it. |
| `## Learn the grants` | `blastit dev` refuses at boot naming the root, the `libphp` or the shim; start `blastit dev --learn`, send every route with your client or a `curl` list, stop it with `Ctrl-C`, read `git diff`, and run `blastit fmt`. |
| `## What CI can prove` | `blastit check` validates the file, every `root` and the libphp gate and then exits `2` without a pytest suite, printing no document, so nothing proves a PHP grant is complete in 0.2.0: the gate is `blastit fmt --check`, which proves the file parses and is canonical, and the reviewed diff is the only evidence ([exit codes](../../reference/blastfile.md#blastit-check)). |
| `## Where next` | Symfony and Laravel. |

**Reference link:** `## The database` → `../../reference/blastfile.md#stores`.

`We support supported PHP versions: 8.3, 8.4 and 8.5.` appears here, once, exactly as written,
and nowhere else in the tree.

`## What CI can prove` carries `$ blastit fmt --check` as its console fence (mode `static`), and
states `check`'s exit `2` in prose rather than as output.

**Never on this page:** a framework cache or migration command, a writable-directory list,
health, the static page's directive table.

### `docs/frameworks/php/symfony.md` — ≤ 100

| Section | Says |
|---|---|
| *(intro)* | A Symfony app is a front controller plus a cache built before the first request; start with the [PHP connector](index.md). |
| `## The Blastfile` | The working file, self-contained: `postgres $DATABASE_URL`, the front controller line, the `public` mount, and the catch-all view that reaches it. |
| `## Warm the cache at build time` | Build with `APP_ENV=prod` and a `bin/console cache:warmup` step into `var/cache`, set the same `APP_ENV=prod` and `APP_DEBUG=0` in the app's runtime environment, and `chown -R 21000:21000 var/` in the image. |
| `## Doctrine reads DATABASE_URL` | Doctrine DBAL reads `$DATABASE_URL` and prepares statements server-side by default, so a statement's text is stable without a switch. |
| `## Trusted proxies` | `framework.trusted_proxies` is what makes `X-Forwarded-*` believed and generated URLs use `https`. |
| `## Migrations` | `bin/console doctrine:migrations:migrate --no-interaction` applies the migrations the image shipped. |
| `## Where next` | Laravel. |

**Reference link:** `## Doctrine reads DATABASE_URL` → `../../reference/blastfile.md#grants`.

**Never on this page:** Artisan, a `libphp` explanation beyond one pointer line, PDO's emulate
switch, health, a second static mount.

### `docs/frameworks/php/laravel.md` — ≤ 100

| Section | Says |
|---|---|
| *(intro)* | A Laravel app is a front controller plus caches and a Vite build made before the image starts; the [connector](index.md) has the host install and the gate. |
| `## The Blastfile` | The working file, with a split `postgres { host $DB_HOST; port $DB_PORT; … }` store and `immutable "/build/assets/*"` over Vite's hashed output. |
| `## Bind parameters in PDO` | Set `PDO::ATTR_EMULATE_PREPARES => false` in `config/database.php`; the default emulates prepares. |
| `## Build the caches` | `npm ci && npm run build`, `php artisan route:cache` and `view:cache` run at build time, `config:cache` stays off because it snapshots the build's environment while Blastit renders `DB_*` inside each view worker, and `chown -R 21000:21000 storage bootstrap/cache` in the image. |
| `## Trusted proxies` | Listing the edge in `TrustProxies` is what makes `$request->ip()` and `secure` URLs follow `X-Forwarded-*`. |
| `## Migrations` | `php artisan migrate --force` applies them non-interactively, because Laravel prompts before it migrates in production. |
| `## Where next` | Symfony. |

**Reference link:** `## The Blastfile` → `../../reference/blastfile.md#stores`.

**Never on this page:** Doctrine, a `libphp` explanation beyond one pointer line, health, a
second binding switch, a `.env` file in the static root.

### `docs/frameworks/static-and-spa.md` — ≤ 90

| Section | Says |
|---|---|
| *(intro)* | A built front end is one `static` mount and no app: no `entrypoint`, no worker, no database. |
| `## The Blastfile` | The working file, and `uvx blastit dev` serves it on `127.0.0.1:8080`, where `uvx` fetches blastit into a throwaway cache with no project and no venv. |
| `## The four directives` | `root` is required, `index` defaults to `index.html`, `fallback` answers a deep path that is no file, and `immutable` names the globs whose files never change. |
| `## Build into the root` | `vite build --outDir public` writes where `root` looks, and Vite's own `publicDir` moves to `static/` because the build empties it. |
| `## Deep URLs and caching` | The hashed asset answers `immutable` for a year while a deep path answers `index.html` with `no-cache`. |
| `## Check the roots` | `blastit check` audits every `root` for existence and readability, exits `0` or `2` naming a root it cannot read, and `--json` prints the same document on success while exit `2` leaves stdout empty. |
| `## Deploy it` | `blastit --preset compose-traefik` on the deploy host starts this file with the Compose preset's defaults, and [deployments](../deploy/index.md) own the stack. |

**Reference link:** `## The four directives` → `../reference/blastfile.md#static-mounts`.

The six console fences stay exactly as they are — one dev banner, `vite build`, two `curl -i`,
`check`, `check --json` and the preset — because `e2e/static_assets.py:544-553` asserts that
count and those commands against the page as its input of record.

**Never on this page:** `postgres`, `body`, `DATABASE_URL`, an `entrypoint`, a Database / Ingress
/ Secrets slot, the reference's path-traversal list.

## The deletion list

Every surviving line is already in the page contracts above. This list says only what dies and
where the mechanics went. Line numbers are today's files on `release/0.2.0`.

**Before the slots — the matrix.** `index.md:10-20` (the `## Where the pages differ` matrix at
12-17 and the "other six slots" sentence at 19-20) and the legend at `index.md:7-8,22-35` die
first: they are the flat organization, and each group's differences now live on its own page.
This supersedes `MB.19` acceptance 4 on purpose; `RESULT.md` says so, so the deletion is not read
as a regression.

1. **Executor** — dies: `index.md:37-41` (the generic §1), `django.md:6-36` (except the `uv add`
   line), `flask.md:6-30`, `php.md:6-54` (its connector half survives on `php/index.md`),
   `static-and-spa.md:3-4,6-28`.
   Home: reference `#sections` (`entrypoint python`/`php`, `preload`, one entrypoint per file)
   and `#static-mounts` for the file with no executor. The PHP support sentence survives once,
   on `php/index.md`; the wheel install command does not, because the wheel ships no shim — the
   connector names the release archive and the distro's own package commands instead. `uvx` is
   explained inline where it is used.
2. **Static assets** — dies: `index.md:43-48`, `django.md:38-54` (its general half),
   `flask.md:31-36`, `php.md:55-59` (its general half), `static-and-spa.md:30-69` (the probe and
   symlink list, and the directive table that belongs to the reference).
   Home: `#static-mounts` (defaults, `*` stops at `/`, one glob per depth, and — before the page's
   list dies — the `.well-known` exception to the dotfile `404`) and `#request-precedence`. Django
   keeps `STATIC_ROOT`/`collectstatic` and the six-segment depth; PHP keeps `public/` as the root;
   the static page keeps the four directives in page voice.
3. **Database** — dies: `index.md:50-57`, `django.md:55-69` (the session-trait lecture part of the
   generic grant half; `django.md:70-72` survives as `## The admin`), `flask.md:37-78` (the
   generic binding sentence and the trait lecture), `php.md:61-67` (the general half),
   `static-and-spa.md:71-74` entirely.
   Home: `#stores`, `#grants`, `#traits` and `#connection-setup`. The binding rule is stated
   once, under the `#bind-parameters-server-side` anchor on `python/index.md`; Django keeps its
   install line and settings block, Flask keeps psycopg/SQLAlchemy and `NullPool`, PHP keeps PDO
   and the split store.
4. **Ingress** — dies: `index.md:59-66` (the four-codec paragraph), `django.md:74-79` (the codec
   half), `flask.md:80-84`, `php.md:69-73` (the codec half), `static-and-spa.md:76-79` entirely.
   Home: `#view-directives` (the codec table and the unacknowledged `raw` rule) and
   `#request-precedence` for a mount's `GET`/`HEAD`/`405`. PHP keeps only "a JSON API is its own
   view above the catch-all, with `body json` and its grant", in `## The Blastfile`; Django keeps
   only the proxy settings.
5. **Environment and secrets** — dies: `index.md:68-74`, `django.md:81-85` (`SECRET_KEY`
   survives as one clause), `flask.md:86-90`, `php.md:75-79` (the `.env` warning survives),
   `static-and-spa.md:81-84` entirely.
   Home: `#stores` (what the app receives in each slot) and settings `#the-app-and-the-file`
   (`BLASTIT_ENV_STRIP`). The `execve` stripping itself stays in `ARCHITECTURE.md` and
   `security.md`; no framework page explains it.
6. **Health** — dies: `index.md:76-81`, `django.md:87-90`, `flask.md:92-95`, `php.md:81-84`,
   `static-and-spa.md:86-89`.
   Home: settings `#port-and-health` (`BLASTIT_HEALTH_PATH`, answered before routing), with the
   one consequence that matters: a view written for it is never reached and `check` reports it
   as an unexercised view. No framework page mentions health again.
7. **Migrations** — dies: `index.md:83-87`, `django.md:92-94` (one clause survives),
   `flask.md:97-99` (one clause survives), `php.md:86-89` (split between the two framework
   pages), `static-and-spa.md:91-93` entirely.
   Home: deployments `anything-else.md#migrations` owns the rule (own role, direct URL, outside
   Blastit). Not the Blastfile reference: no directive describes a migration. Each framework
   page names only its command.
8. **Learn and check** — dies: `index.md:89-97` (the finding-kind list), `django.md:96-105`
   (the paste-a-statement paragraph), `flask.md:101-105` (the generic half),
   `php.md:91-100` (the wheel-era aside; the learn and CI pair survives as two short sections),
   `static-and-spa.md:95-105`.
   Home: `#what-learn-writes` and `#blastit-check` for the file mechanics, `ci.md#what-it-fails-on`
   for what each finding means and `ci.md#for-agents-json` for the machine-readable loop.
   Connector pages keep the learn/check split; the PHP one names the `fmt --check` gate.
9. **Deployment notes** — dies: `index.md:99-106`, `django.md:107-110` (`preload`'s copy-on-write
   told a second time), `flask.md:107-110` (survives as `python/index.md`'s `## One view, one
   worker`), `php.md:102-110` (the preset fence and the bare worker cap die; the image's `libphp`
   line survives as one clause in `## Install the host library`; the `21000` uid and its `chown`
   survive on the two framework pages), `static-and-spa.md:107-114` (the preset fence survives
   under `## Deploy it`; the pooler aside dies).
   Home: deployments `index.md` and settings `#admission-and-threads` and
   `#privileges-and-the-uid-layout`. `preload` is explained on `python/index.md` only.

## Anchors and inbound links

- `#manifest` — `python/django.md`, heading `## The Blastfile { #manifest }`, exactly one hit in
  the tree. Both deploy pages' jumps land on the working file.
- `#bind-parameters-server-side` — `python/index.md`, the database heading. `troubleshooting.md`
  keeps its sentence and changes only the path.
- Repoints, all measured: the nav's four flat peer entries (lines 33-39 of the site config) become
  three groups with children; `getting-started.md:66`, `troubleshooting.md:70`,
  `compose-traefik.md:169`, `digitalocean-paas.md:64`. `ADR-0007` names the static page's
  unchanged path; leave it.
- Each connector page links its framework pages once, in `## Where next`; each framework page
  links its connector once, in its intro line. Each page's contract names the reference links it
  carries, one per section, and the chooser's nine questions carry theirs. No page links the same
  target twice.

## Fences and the Rust pin

Console fences are registered per page and index; `text title="Blastfile"` blocks are parse-only
under the strict gate and are not inventory entries.

| Page | Console fences |
|---|---|
| `python/index.md` | none (`uv add blastit` inline) |
| `python/django.md` | 1 `check` → fixture `shop` (carried over), under `## Tests and migrations` |
| `python/flask.md` | 1 `install-external` (carried over), under `## Install` |
| `php/index.md` | 1 `blastit dev --learn` (carried over, same reason) and 2 `blastit fmt --check` (mode `static`, under `## What CI can prove`); the `uv tool install blastit` fence dies, because the wheel ships no shim |
| `static-and-spa.md` | 1–6 all carried over: `example` (dev banner), `example` (`vite build`), `dev-curl` → `static-site` ×2, `example` (`check` and `--json`), `presets` |
| `php/symfony.md`, `php/laravel.md` | none |

Every fence-less page still needs an empty `[[page]]` entry: `python/index.md`, `php/symfony.md`,
`php/laravel.md`. `php.md` fence 3 (a preset command) dies with slot 9, because deployments own
presets and three preset fences remain there.

`crates/blastit-manifest/tests/docs_fences.rs` pins the same pages from the Rust side, so Phase B
patches it in one pass: `PAGES` drops `frameworks/{index,django,flask,php}.md` and gains
`frameworks/{python/index,python/django,python/flask,php/index,php/symfony,php/laravel}.md`; the
root index carries no `text title="Blastfile"` document, so `TOTAL`'s document count goes from
`10` to `12`; `STATEMENT_LISTS` drops `docs/frameworks/django.md` (its mount is now inside the
document), and `the_django_page_static_fragment_still_parses` reads that document's mount —
prefix `/static`, root `staticfiles`, six `immutable` patterns — with the page pinned
`Shape::ViewsAndMount`; the other `Shape` rows are `Application` for `python/index` and
`python/flask`, `ViewsAndMount` for the three PHP pages. Illustration counts are measured, then
pinned — not guessed here.

## Rules for Phase B

- `Blastfile` syntax only, and only keys the reference documents. The retired manifest's name,
  extension and bracket-key spelling appear nowhere, in a page or in this outline.
- No page offers a second transport for a runtime Blastit already owns. On the static page, keep
  the `fallback` directive in a clause of its own. The guard splits a sentence at `;`, `:` and an
  em dash, and a clause naming `php`, `fastcgi`, `libphp`, `daemon` or `route` counts as a
  runtime claim. Write "a deep path", not "a route".
- No apology, hedge, "for now" or "coming soon". Roadmap phrasing for Node and Tomcat only.
- Budgets are ceilings. A section that does not fit is cut to one line, never merged back into a
  nine-slot shape.
- Every command, setting and number must exist in the code or the reference. A fact nobody
  verified becomes a `Follow-ups` item, not a sentence.
- One reference link per section that needs one, never two for the same table; no page restates a
  reference table.
- Every PHP framework page's `Blastfile` fence declares `postgres` — `$DATABASE_URL` on Symfony,
  the split store on Laravel — because a file without a store passes the operator's real URL
  straight through and bypasses the broker.
- The three old paths are deleted in the same change that adds the new ones, so no commit has
  two pages owning one claim.

## Decisions a reviewer should check

1. **The nine questions on the root index** — kept because the node requires them, phrased as a
   checklist with one link each. If they read as a legend, compress them to one paragraph of nine
   inline links; acceptance 3 requires the questions, not a section.
2. **Two sections go beyond the node's content list.** `python/django.md` gets `## The admin`:
   every `startproject` user clicks `/admin/` first, and the answer is the real grant directives,
   never `unrestricted`. `php/index.md` gets `## Run it from an artifact that carries the shim`:
   the wheel ships no shim, so the release archive is what makes the page's first steps work at
   all. The split store stays inside Laravel's `Blastfile` fence, with its explanation on the
   connector.
3. **No page teaches `unrestricted`.** The Django admin path is the reference's
   `select`/`insert`/`update`/`delete` grant lines on the admin's tables; the reference keeps the
   directive and its warning, and the page links `#grants` instead of repeating it.
4. **The PHP artifact is the release archive, not the declared image.** `ghcr.io` tags are in the
   platform matrix but not published in 0.2.0 (`MB.3.18` follow-up), so naming one would be a
   command no reader can run; a self-built image is covered by one clause.
5. **`## Behind TLS` on Django, `## Trusted proxies` on both PHP pages** — a few lines each, and
   the only app-side setting a deploy forces; the alternative is a reader whose URLs and client
   address are wrong in production with no page naming why.
6. **The static page keeps six console fences** — the content contract says "nothing else", but
   `e2e/static_assets.py` reads the page as its input of record and asserts exactly those
   commands. The fences stay; the prose around them does not grow.
7. **Laravel names `config:cache` to leave it off.** The node's content contract says build-time
   `config:cache`; it snapshots `env()` at build time while Blastit renders `DB_*` inside each
   view worker, so the page says so and caches only routes and views. The command still appears,
   so acceptance 7's grep holds.

## Dependencies outside this node's paths

- `scripts/test_doc_tone.py:112` asserts the real `docs/frameworks/php.md` is one of the covered
  pages. Deleting that file turns `make docs-unit` red until the assertion names
  `docs/frameworks/php/index.md` (the module docstring says the same). The file is outside
  `MB.20.paths`: Phase B lands the one-line change with `lead`'s approval, or acceptance 11
  cannot be green.
- `crates/blastit-manifest/tests/docs_fences.rs` pins the deleted paths, their counts, each
  page's shape and the Django static fragment, and `make check` runs it. Phase B needs the
  `PAGES`/`STATEMENT_LISTS`/`TOTAL`/fragment patch described in **Fences and the Rust pin**, and
  the file is outside `MB.20.paths` — same approval.
- `crates/blastit/src/check.rs:458` prints `(see docs/frameworks/django.md)` in a live refusal;
  repoint it to `docs/frameworks/python/django.md`, and repoint the comments at
  `src/dev_learn.rs:952` and `src/stage0.rs:585` to `php/index.md` — `dev_learn.rs` also quotes
  the old sentence and needs it reflowed. The same path appears in test comments at
  `crates/blastit-manifest/tests/static_mounts.rs`, `crates/blastit/tests/critical_php_backend.rs`,
  `critical_php_embed.rs`, `critical_manifest_edit_executor.rs`,
  `critical_learn_executor_blocks.rs`, `critical_static_manifest.rs` and
  `critical_static_cli.rs`; none is read at run time, but they are the citations an implementer
  follows, so one sweep updates them. Outside paths, one line each.
- `docs/reference/settings.md` does not document `BLASTIT_PHP_SHIM_DIR`, although the Stage-0
  refusal names it for a `dev`/`check` boot (`crates/blastit/src/stage0.rs:793-806`). Add the row
  in a follow-up, or a developer who builds the shim locally cannot find the seam.
- `docs/ci.md` has no PHP path: its one workflow is `uv sync` plus `blastit check --json`, which
  exits `2` for a PHP repository. The connector states the `fmt --check` gate, but the CI page
  needs the same paragraph in a follow-up — land it with `lead`'s approval in the same commit as
  the pages, or the released runtime's CI path exists only in a follow-up.
- `docs/deploy/index.md:57` and `docs/deploy/anything-else.md:26,139` still teach `egress = true`;
  the directive is `egress any`. A three-line correction, outside paths, so the reader leaving
  the new tree does not meet the retired spelling.
- `e2e/php2shell.py` reads the page (`--page` at its `self_test` and replay) and
  `e2e/compose.yaml:133` mounts `../docs/frameworks/php.md`. Its self-test asserts
  `install == 1`, `dev-learn == 1`, `skip == 1` and "`blastit check` runs pytest"; the new
  connector keeps `dev --learn`, drops the wheel install and the preset, and adds
  `blastit fmt --check`, so Phase B repoints `PAGE` and the compose mount, removes the two dead
  command kinds, adds `fmt --check` to `SKIPS`/`command_kind` as an explained command (the
  container has no host `libphp`), counts the new kind, and rewrites the four assertions plus the
  docstring's "three `$ ` commands" and "runs pytest" sentences. Outside paths; `make e2e` is red
  until it lands.
- `e2e/static_assets.py` is the static page's input of record and pins its six console fences
  (see that page's contract); `e2e/compose.yaml:89` mounts it. Its docstring and `DENIALS`
  comment cite the page's refusal list, which slot 2 moves to the reference, so reword them in
  the same sweep. The remaining `php.md` mentions in
  `e2e/php/{README.md,Dockerfile,fixture/Blastfile}` are provenance comments and follow the path
  rename.

## Out of scope

No Node/Tomcat page (they inherit this shape when their connectors land), no change to
`docs/deploy/**` beyond the two link lines, no restructure of the reference, no nav or inventory
edit in Phase A.

## Review resolution

Recorded before Phase B. The first two rounds are closed by the page contracts above; this table
records the third round and the lead's fact-check.

| Finding | Resolution |
|---|---|
| `dx` B1: the admin pointed at a flag `check` does not accept | The admin section names the `select`, `insert`, `update` and `delete` grant lines for the admin's tables; no `check` flag and no report path is required. |
| `dx` B2: the PHP archive unpacks into a directory and the binary was spelled two ways | One spelling: extract `blastit-<arch>-unknown-linux-<libc>.tar.gz`, put the directory it creates on `PATH`, run `blastit dev`; every command on the page is bare `blastit`. |
| `dx` M1: `libphp-embed` installs an unsupported PHP on Debian 12 / Ubuntu 22.04 | The install cell names the supported minors, the Debian 13 / Ubuntu 24.04+ package, and the refusal on older releases; the intro says `sudo` is PHP's one privileged step. |
| `dx` M2: the `chown` clause was duplicated and prescribed a shared group | The uid rule (`BLASTIT_UID_BASE + 1000 + i`, `gid == uid`, no supplementary groups, ownership or ACL) is stated once on the connector; each framework page names only its directory. |
| `dx` M3: PHP had no runnable CI path | `## What CI can prove` carries the `fmt --check` fence; the `docs/ci.md` paragraph is a required paired change in **Dependencies**. |
| `dx` M4: Django's working file lacked its `static` mount | The mount is inside the one `text title="Blastfile"` document, the page is pinned `Shape::ViewsAndMount`, and the Rust fragment test moves onto the document. |
| `dx` M5: two fence entries had no section | Flask gains `## Install`; Django's `check` fence lives in `## Tests and migrations`. |
| `ai` B1: `table_grants` is not in `check --json` | The key belongs to the pytest plugin's report; the admin writes the reference's real grant directives for the admin's tables instead of depending on a report. |
| `ai` B2: the SQLAlchemy probe's grant was uppercase | It is lowercase, as the wire sends it (`e2e/sqlalchemy/Blastfile:23`). |
| `ai` B3: Flask lost `transactions` | The `connection` trait carries `transactions` for `engine.begin()`, and `## SQLAlchemy` says why. |
| `ai` M1: Django trusted a header the app should not | `SECURE_PROXY_SSL_HEADER` is named as the setting not to add; the ingress is trusted in `BLASTIT_TRUSTED_PROXIES`, and the deploy's settings are `CSRF_TRUSTED_ORIGINS` and `ALLOWED_HOSTS`. Fixed after the check's snapshot: the current `## Behind TLS` cell reads "Do not set `SECURE_PROXY_SSL_HEADER`". |
| `ai` M2: Flask's `ProxyFix` re-read untrusted headers | The proxy section says nothing to configure and names `ProxyFix` as the thing not to add. |
| `ai` M3: PHP had no learn driver and no statement about unproven grants | `## Learn the grants` drives a route sweep and `Ctrl-C`; `## What CI can prove` says the reviewed diff is the only completeness evidence. |
| `ai` M4: a view with no store block has no socket | `## Learn the grants` says every database view gets an empty `postgres {}` before the run; Flask's catch-all is labelled a demo. |
| `ai` M5: the static list's new home contradicted `.well-known` | Deletion slot 2 requires the `.well-known` exception in `#static-mounts` before the list is deleted. |
| `ai` M6: the new `fmt --check` fence is an unclassified e2e command | The e2e dependency bullet adds the kind, the skip and the docstring rewrite. |
| Lead fact-check: `table_grants` nests under `proxies.<store>` in the plugin's report, and its `blastfile` is a string of directives | The admin names the reference's real directives (`select`/`insert`/`update`/`delete`, at least one column, `pg_catalog` as `query`); the report key is recorded as the plugin's, not `check`'s. |

The `ai` check read an earlier snapshot (486 lines). The `## Behind TLS` cell it quoted for M1
was replaced in the same minute, before that review file was saved; the cell now reads "Do not
set `SECURE_PROXY_SSL_HEADER`". The other eight lines are resolved as recorded above.

## MINORs carried into prose

Applied while writing the pages; no further review round.

- Deletion ranges are section-level: delete the prose, keep the fences and tables each contract
  names (`vite build`, the two `curl` fences, the static preset, the Django settings code).
- The PHP "no request audit / exit `2`" sentence appears in the connector, `docs/blastfile.md` and
  the reference; keep the reference's and trim one of the other two.
- `docs/deploy/index.md:57` and `docs/deploy/anything-else.md:26,139` still say `egress = true`;
  correct them to `egress any` with approval.
- `docs/troubleshooting.md:80` still names `SECURE_PROXY_SSL_HEADER` as the fix when `is_secure()`
  is false; align it with the trusted-peer rule (`BLASTIT_TRUSTED_PROXIES`).
- If the chooser's nine questions read as a legend at ≤ 50 lines, compress them to one paragraph
  of inline links; the node requires the questions, not the layout.

## Reviews

- `docs/design/frameworks-restructure.review-dx.md`
- `docs/design/frameworks-restructure.review-ai-ergonomics.md`
