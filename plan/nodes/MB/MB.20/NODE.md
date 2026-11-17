---
id: MB.20
title: "Rework /frameworks for Blastfile: language/connector first, framework considerations inside (docs-only)"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: [MB.21.5.2]
paths:
  - "docs/frameworks/**"
  - "docs/design/frameworks-restructure*"
  - "docs/getting-started.md"
  - docs/troubleshooting.md
  - "docs/deploy/compose-traefik.md"
  - "docs/deploy/digitalocean-paas.md"
  - docs/reference/blastfile.md
  - docs/blastfile.md
  - zensical.toml
  - python/tests/runnable_docs/inventory.toml
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - crates/blastit/src/check.rs
  - scripts/test_doc_tone.py
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T21:50"
updated: "2026-11-17T04:44"
---

# Goal

Rework `docs/frameworks/` completely for `0.2.0`. The owner's verdict on today's tree is that
it "is still the same soup as it was before": five flat pages, every one of them filling the
same nine numbered slots (`## 1. Executor` … `## 9. Deployment notes`), six of which restate
runtime mechanics or hold framework-empty filler (the Database/Ingress/Secrets slots on
`static-and-spa.md` say nothing). That shape is **retired here**, not moved.

The new tree is organized **by language/connector first, and framework-by-framework
considerations inside**: one chooser, one short connector page per runtime, then a page per
framework that answers only what that framework makes you do differently. Every example is
`Blastfile` syntax: `app`, `entrypoint python "module:attr"`, `postgres $DATABASE_URL`, `view … {
… }`, `preload`, `transactions`, and for a built front end `static "/" { root … }` with
`immutable` and a `fallback` file. Never the retired TOML manifest, never a nine-slot legend
repeated per page.

## Deliverable tree (line budgets are hard)

| Page | Lines | Says only |
|---|---|---|
| `docs/frameworks/index.md` | ≤ 50 | chooser: **Python (WSGI)**, **PHP (embedded `libphp`)**, **Static sites & SPAs**; scannable table + direct links; one roadmap line for Node/Tomcat |
| `docs/frameworks/python/index.md` | ≤ 90 | how the **Python (WSGI)** connector works |
| `docs/frameworks/python/django.md` | ≤ 110 | Django considerations |
| `docs/frameworks/python/flask.md` | ≤ 110 | Flask, SQLAlchemy and raw-WSGI considerations |
| `docs/frameworks/php/index.md` | ≤ 100 | how the **PHP (embedded `libphp`)** connector works |
| `docs/frameworks/php/symfony.md` | ≤ 100 | Symfony considerations |
| `docs/frameworks/php/laravel.md` | ≤ 100 | Laravel considerations |
| `docs/frameworks/static-and-spa.md` | ≤ 90 | static sites and SPAs only |

Content contract per page, so a cheap writer cannot drift back into soup:

- **root index**: three groups + static, one line each, a table of `runtime | what you write |
  pages`, and the nine mechanics questions as **questions only** (no answers, no matrix, no
  runtime mechanics, no duplicated lecture).
- **`python/index.md`**: `entrypoint python "module:attr"` with the optional `{ preload
  "<mod>" }`, `uv add blastit`, one worker process per view, the sandbox, the local
  Unix-socket `$DATABASE_URL` the app receives instead of the credential, and the rule that
  parameters travel apart from statement text so one template is one grant — this page carries
  `{ #bind-parameters-server-side }`. Then `blastit dev --learn` → `blastit check`. No
  `collectstatic`, no `create_app`, no framework-specific example beyond a ten-line stub.
- **`python/django.md`**: working `Blastfile` under `## The Blastfile { #manifest }`;
  `preload "myshop.urls"`; `static "/static"` mounted on `STATIC_ROOT` +
  `collectstatic --no-input`; `transactions` for `atomic()` savepoints; `settings.py` with
  `dj_database_url` `conn_max_age=0` and `server_side_binding=True`; `SECURE_PROXY_SSL_HEADER`
  and `ALLOWED_HOSTS`; `pytest-django` under `blastit check`; `manage.py migrate` run outside
  view workers. Nothing about Flask or PHP.
- **`python/flask.md`**: working `Blastfile`; module attribute vs a `create_app()` wrapper (and
  which one `entrypoint python` names); SQLAlchemy + `psycopg` v3 with server-side binding,
  `NullPool` and connection lifecycle under one worker per view; `ProxyFix`; `pytest` under
  `blastit check`. Nothing about Django or PHP.
- **`php/index.md`**: `entrypoint php "public/index.php"` declared once at top level; the
  embedded host NTS `libphp` (`libphp-embed`, `php85-embed`/`php84-embed`/`php83-embed`,
  `php-embedded`) and `BLASTIT_LIBPHP`; one request at a time inside the view worker with no
  separate daemon, pool or socket; `public/` as `static "/" { root "public" }` returning `404`
  for `.php`/`.phtml`/`.phar`; PDO/libpq wiring via `$DATABASE_URL` or split `DB_*`; `blastit
  dev --learn` → `blastit check` (which validates file, roots and the libphp gate). The support
  statement appears **once, verbatim, here** and nowhere else:
  `We support supported PHP versions: 8.3, 8.4 and 8.5.`
- **`php/symfony.md`**: working `Blastfile`; `public/index.php` front controller; Doctrine DBAL
  reading `$DATABASE_URL` with server-side prepared statements; `APP_ENV=prod` and a build-time
  `bin/console cache:warmup` with a pre-warmed `var/cache`; trusted proxies;
  `doctrine:migrations:migrate` outside the app uid. Nothing about Artisan.
- **`php/laravel.md`**: working `Blastfile` with a split `postgres { host $DB_HOST; port
  $DB_PORT; … }` store and a Vite `/build/*` mount; `PDO::ATTR_EMULATE_PREPARES => false` in
  `config/database.php` so PDO binds parameters instead of interpolating literals; build-time
  `php artisan config:cache`, `route:cache`, `view:cache`; `TrustProxies`; `php artisan migrate`
  outside the app uid. Nothing about Doctrine.
- **`static-and-spa.md`**: working `Blastfile` with `static "/" { root "public"; immutable
  "/assets/*"; fallback "index.html" }`, `root`/`index`/`fallback`/`immutable` semantics,
  `vite build --outDir public`, cache headers, deep-URL SPA routing, `blastit check` auditing
  every `root`. No Database, Ingress or Secrets slot, no filler, no `entrypoint`.

# Context

- Parent: **`MB`**; the flat tree this node rewrites was built by `MB.19` (done), whose nine-slot
  contract and capability matrix are **superseded on purpose** (see Scope supersession). `MB.21`
  replaced the v1 manifest with `Blastfile`; `MB.21.5.1` purged the retired format from the site
  and `MB.21.5.2` executes the doc fences live and flips `docs_fences` strict.
- **Measured today** (`release/0.2.0`): `docs/frameworks/` is flat — `index.md` (112),
  `django.md` (110), `flask.md` (110), `php.md` (110), `static-and-spa.md` (114) — and
  `zensical.toml:33-39` lists them as four peer entries plus the index. Each page carries all
  nine numbered headings; `index.md` answers all nine generically and each framework page
  answers most of them again.
- **Split point for PHP is real**: `php.md` today mixes connector mechanics (embedded SAPI, host
  library, `404` on `.php`, `public/` root) with Symfony-only and Laravel-only advice. Symfony
  differs from Laravel on warmup/caches (`bin/console cache:warmup` + `var/cache` vs
  `config:cache`/`route:cache`/`view:cache` + writeable `bootstrap/cache`), migration command
  (`doctrine:migrations:migrate` vs `php artisan migrate`), writable directories, and the
  binding switch (`PDO::ATTR_EMULATE_PREPARES => false` is a Laravel `config/database.php`
  decision; Doctrine binds server-side by default).
- **Anchors that must survive** (inbound links in the table below): `#manifest` and
  `#bind-parameters-server-side`.
- **Inbound references measured (all must be repointed)**

  | Where | Points at | New target |
  |---|---|---|
  | `zensical.toml:33-39` | four flat peer entries | `frameworks/index.md` + three groups with children |
  | `docs/getting-started.md:66` | `frameworks/django.md`, `frameworks/flask.md` | `frameworks/python/…` |
  | `docs/troubleshooting.md:70` | `frameworks/index.md#bind-parameters-server-side` | `frameworks/python/index.md#bind-parameters-server-side` |
  | `docs/deploy/compose-traefik.md:169` | `../frameworks/django.md#manifest` | `../frameworks/python/django.md#manifest` |
  | `docs/deploy/digitalocean-paas.md:64` | `../frameworks/django.md#manifest` | `../frameworks/python/django.md#manifest` |

  `docs/decisions/ADR-0007-…:11` names `docs/frameworks/static-and-spa.md` in backticks and that
  path is unchanged: leave the historical record alone.

- **Fence inventory measured**: `python/tests/runnable_docs/inventory.toml` registers fences
  **per page path and fence index** — `index.md` none, `django.md` 1 (`check`, fixture `shop`),
  `flask.md` 1 (`install-external`), `php.md` 1–3 (`static`, `static`, `presets`),
  `static-and-spa.md` 1–6 (`example`, `example`, `dev-curl` fixture `static-site`, `dev-curl`,
  `example`, `presets`). `python/tests/runnable_docs/run.py` refuses a published page or console
  fence the inventory does not name, so the split re-indexes these entries with the same mode and
  the same written `reason` for every mode that means "not executed". Fixtures that exist keep
  executing: Django's `check`/`shop` and the static `dev-curl`/`static-site` fences stay live.

## Scope supersession

This node supersedes `MB.19` acceptance 4 (one capability matrix framework × executor × static ×
DB) and its nine-heading contract. That matrix **is** the flat organization being retired; each
group's differences now live on its own page. `RESULT.md` must say so, so a reviewer does not
read the deletion as a regression. Prose removed from `index.md`/`php.md` is not lost: the
mechanics it restated live in `docs/reference/blastfile.md`, and `RESULT.md` carries a one-line
map from each removed passage to that home.

# Acceptance

```console
$ find docs/frameworks -name '*.md' | sort
$ wc -l docs/frameworks/index.md docs/frameworks/python/*.md docs/frameworks/php/*.md docs/frameworks/static-and-spa.md
$ grep -rn '^## [1-9]\. ' docs/frameworks/ || true
$ grep -rn 'We support supported PHP versions: 8.3, 8.4 and 8.5.' docs/frameworks/php/
$ grep -rn 'coming soon\|for now\|currently only\|does not yet\|we are sorry' docs/frameworks/
$ make docs-check
$ make docs-unit
$ make docs-test
$ uv run --no-project tools/plan.py validate
```

1. **Tree exactly as tabled**: the eight pages above exist at those paths; the three old flat
   pages `docs/frameworks/{django,flask,php}.md` are gone; `git status --short docs/frameworks`
   shows renames/additions only, no orphan page. Each group is reachable from
   `docs/frameworks/index.md` in one click and every page is in the nav.

2. **The nine-slot boilerplate is gone**: `grep -rn '^## [1-9]\. ' docs/frameworks/` prints
   nothing, and no page carries a nine-item legend. Since pages no longer share structure, prove
   non-duplication instead — this must print nothing, and a reworded restatement is the
   reviewer's call, recorded in `RESULT.md`:

   ```console
   $ for f in docs/frameworks/index.md docs/frameworks/python/*.md docs/frameworks/php/*.md docs/frameworks/static-and-spa.md; do \
       grep -E '^[A-Za-z`].{80,}' "$f"; done | sort | uniq -d
   ```

3. **Root chooser** (`≤ 50` lines): names the three groups by runtime, their pages, and one line
   for static; a scannable table; the nine mechanics questions appear as questions only, never as
   answers; no capability matrix; no runtime mechanics; the Node/Tomcat sentence is phrased as
   roadmap, not as "coming soon".

4. **Python connector** (`≤ 90` lines) states only what is identical for Django and Flask:
   `entrypoint python "module:attr"` + `{ preload … }`, `uv add blastit`, one worker per view
   inside its sandbox, the local Unix-socket `$DATABASE_URL`, and the parameter-binding rule under
   a heading carrying `{ #bind-parameters-server-side }`. It names no framework setting
   (`collectstatic`, `create_app`, `atomic`) and shows at most a ten-line `entrypoint` stub.

5. **Python framework pages** (`≤ 110` lines each) each contain a working `Blastfile` fence
   (`text title="Blastfile"`) and only their own considerations — items listed in the content
   contract above. `python/django.md` carries the anchor as `## The Blastfile { #manifest }`, so
   `grep -n '{ #manifest }' docs/frameworks/python/django.md` prints exactly one hit and both
   deploy pages' `#manifest` jumps land.

6. **PHP connector** (`≤ 100` lines) states only the connector: `entrypoint php
   "public/index.php"` once at top level, host NTS `libphp` install and `BLASTIT_LIBPHP`,
   per-view worker isolation with no daemon/pool/socket, `public/` as `static "/" { root
   "public" }` with `404` on `.php`/`.phtml`/`.phar`, PDO/libpq `$DATABASE_URL` or split `DB_*`,
   `blastit dev --learn`/`blastit check`. The support statement appears exactly once, verbatim, in
   `docs/frameworks/php/` — the grep above prints one hit and no stale `8.3 and 8.4` remains:
   `grep -rn '8\.3 and 8\.4' docs/frameworks/` prints nothing.

7. **PHP framework pages** (`≤ 100` lines each) disagree where the frameworks disagree — warmup
   and cache story, migration command, writable directories, binding switch — and each keeps its
   own working `Blastfile`. Neither restates `libphp` mechanics;
   `grep -rn 'libphp' docs/frameworks/php/symfony.md docs/frameworks/php/laravel.md` prints
   nothing beyond a deliberate one-line pointer to the connector page (state which in
   `RESULT.md`). `php/laravel.md` names `PDO::ATTR_EMULATE_PREPARES => false`;
   `php/symfony.md` names `cache:warmup` and `doctrine:migrations:migrate`; `php/laravel.md`
   names `config:cache`/`route:cache`/`view:cache` and `php artisan migrate`.

8. **Static & SPA page** (`≤ 90` lines) carries `static "/" { root "public"; immutable "/assets/*";
   fallback "index.html" }`, the directive semantics and the SPA deep-URL story, and nothing
   else: `grep -n 'postgres\|body json\|DATABASE_URL' docs/frameworks/static-and-spa.md` prints
   nothing.

9. **Navigation, references and rendering**: `zensical.toml` gains the nested groups (Python
   with Django and Flask children, PHP with Symfony and Laravel children, Static and SPAs) and
   every inbound reference in the measured table is repointed; `#manifest` and
   `#bind-parameters-server-side` still resolve; `troubleshooting.md` links the Python index
   anchor. `make docs-check` is `--strict`, so a nav entry with no file or a page outside the nav
   fails. `visual-checker` PASSes the tree in light, dark and 390×844: the group drawer opens,
   all three bridged anchors land, no raw pipes or clipped fences.

10. **DX-first order, provable on disk**: before any page prose, `doc-writer` wrote
    `docs/design/frameworks-restructure.md` (page → sections → the one sentence each says, plus
    the deletion list for the nine slots) and two **goldfish** critics (`dx` and
    `ai-ergonomics`, fresh context, brief: "can I get from 'I use Django/Laravel' to the sentence
    that answers me in two clicks, and does any sentence appear twice?") wrote
    `…review-dx.md` and `…review-ai-ergonomics.md`, each ending `VERDICT: PASS`. Evidence:
    `git log --oneline -- docs/design/frameworks-restructure* docs/frameworks` shows the outline
    and both reviews in an earlier commit than the first page, and `LOG.md` records both verdicts
    before the pages were written. No page was written while an outline finding was open.

11. **Gates and language**: `make docs-check`, `make docs-unit`, `make docs-test` are green, with
    every page and every console fence registered per page path and fence index in
    `python/tests/runnable_docs/inventory.toml` (same mode and written `reason` as today's entry);
    fences with fixtures keep executing (`check`/`shop`, `dev-curl`/`static-site`) and Blastfile
    fences parse under the strict `docs_fences` gate. No page exceeds its budget, `make doc-tone`
    is green, and the retired-manifest guard in `python/tests/test_docs_assets.py` stays green —
    no page names the retired TOML manifest or the retirement vocabulary the guard lists. The
    grep above prints nothing: no apology, no hedge, no "coming soon".

# Notes

- Order of work: outline → `dx` + `ai-ergonomics` goldfish reviews → fix the outline → write
  `python/index.md` and `php/index.md` → split the framework pages → rewrite `index.md` as the
  chooser → `zensical.toml` + inbound links → `inventory.toml` → `make docs-check`/`docs-unit`/
  `docs-test` → `visual-checker`.
- The connector guard in `python/tests/test_docs_assets.py` refuses the word `fallback` on a page
  that names the embedded runtime, so keep those two in separate sentences and write "the static
  responder answers with `index.html`".
- `entrypoint php` is declared **once at top level**; a PHP page that shows it inside a `view`
  block is wrong. `static "/"` is the mount, `root "public"` the directory.
- Pages must use `Blastfile` syntax throughout: `keyword args { directives }`, no `=`, no
  `version` header, `$IDENT` for environment variables. A manifest-format fence fails
  `docs-test` and the guard.
- Do not touch `docs/deploy/**` beyond the two inbound-link lines in `compose-traefik.md` and
  `digitalocean-paas.md`; `MB.18` just restructured that tree. Do not touch
  `docs/frameworks/node.md`/`jvm.md` — they do not exist yet and inherit this shape when `MB.4`
  /`MB.5` land (state that in one line on the root index).
- Keep `docs/reference/blastfile.md` as the single home of product mechanics; a page may add one
  link to it and must not restructure it. No new manifest key, no invented product behaviour, no
  invented numbers (a fact nobody verified, e.g. exactly which `var/cache` subdirectories must be
  writeable in a pre-warmed Symfony image, becomes a `Follow-ups` item in `RESULT.md`).
- `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` is the source for the `libphp` facts;
  `docs/blastfile.md` is the one-page tour the connector pages mirror (add a cross-link there
  only if it helps a reader, one line).
