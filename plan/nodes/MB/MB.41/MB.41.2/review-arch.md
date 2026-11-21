# review-arch — MB.41.2 (deploy / frameworks / reference / how-it-works / security / troubleshooting)

Base: `48f2952a47cc4fb90543b18a5c7d0115ef607563` (HEAD)..working tree.
Scope read: the 18 changed pages, `e2e/static_assets.py`,
`python/tests/runnable_docs/inventory.toml`, plus `crates/blastit/src/**`,
`crates/blastit-{http,scheduler,manifest,view-php}/src/**`,
`scripts/{build-standalone.sh,platform_matrix.py}`, `docs/{ci,getting-started,blastfile}.md`.

**VERDICT: FAIL** — two deploy snippets that cannot build/boot, plus a weakened
Django grant set. The prose rewrite itself is sound.

## Machine checks run (all green)

* All **8** `text title="Blastfile"` fences (php/index, php/laravel, php/symfony,
  python/index, python/django, python/flask, static-and-spa,
  digitalocean-paas store block) → `target/debug/blastit fmt --check <file>` = `0`,
  i.e. they parse **and** are canonical (`fmt --check` is semantic: a bogus
  directive exits `2`, verified separately).
* `frameworks/{python/index,django,flask}.md` keep their pinned `Shape::Application`
  fences: no `static` mount was added to a pinned Blastfile. `python/index.md:51`
  and `flask.md:59-63` mention a mount in prose only — census unchanged.
* All-static truth: `blastit check` → exit `0`, empty stdout; `blastit check --json`
  → one document, `ok:true`, `routes.total:0`, `requests:0`. `static-and-spa.md:88-92`
  is correct about the document.
* Presets: `blastit --preset {compose-traefik,digitalocean-paas}` → exit `1`, names
  the missing `cap_*`, points at `blastit dev`; both names resolve in
  `config/preset.rs:47-70`; every preset row in `deploy/*.md` matches
  `Preset::settings` (`do-connecting-ip` / `x-forwarded-for`, `*`, `on`, `json`,
  `Root` / `Capabilities`).
* Every `BLASTIT_*` named in the changed pages exists in code (incl. the
  `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` family).
* Capacity defaults match `config/settings.rs:267-315`: `MAX_WORKERS=16`,
  `QUEUE_SIZE=256`, `QUEUE_TIMEOUT=10s`, `VIEW_MEM_ESTIMATE=32MiB`,
  `MAX_BODY_BYTES=100MiB`, `BACKLOG=1024`, `UID_BASE=20000` (so `21000` is right).
* Wheel coverage claim (`python/index.md:17-20`) matches the matrix: 8 wheels =
  2 arches × 2 libcs × CPython {3.13, 3.14}; `scripts/platform_matrix.py --check`
  reports `ok`.
* Every anchor introduced or referenced by these 18 pages resolves against a real
  heading (incl. `php/index.md#install`, `django.md#static-assets`,
  `reference/blastfile.md#blastit-check`, `#static-mounts`, `#grants`,
  `#connection-setup`, `index.md#measure-your-ingress`, `#pooler`,
  `#client-ip-header`, `ci.md#what-it-fails-on`, `getting-started.md#1-install`).
  `make docs-check` (`zensical build --strict`) passes.
* `e2e/static_assets.py` still keys the same page lines: SKIPS still holds
  `npx vite build --outDir public` and `blastit --preset compose-traefik`, the
  banner fence is still compared with `_BANNER_SUMMARY` (page ` 1 route`, live
  `0 routes`), the `DENIALS` table still names exactly the page's statuses
  (`..`/`%2e%2e`/NUL/backslash → `400`; dotfile + symlink escape → `404`), and
  `/.well-known/` is still the one allowed dotfile. Accepting bare
  `blastit check --json` is correct and minimal: `run_check()` already exec'd bare
  `blastit`, so the page and the driver now agree without touching behaviour.

## [BLOCKER] 1 — the DigitalOcean PHP image ships no shim, so `entrypoint php` refuses

`docs/deploy/digitalocean-paas.md:99`

```dockerfile
RUN curl ... && tar -xzf blastit-*.tar.gz && mv blastit-x86_64-unknown-linux-gnu/blastit /usr/local/bin/
```

`scripts/build-standalone.sh:11-20` packs `blastit` **plus**
`libblastit_php<digits>-<arch>-<libc>.so` into one directory; the shim is found
*beside the executable* unless `BLASTIT_PHP_SHIM_DIR` names another directory
(`config/settings.rs:221-224`). `mv` of the single binary orphans every shim, so
the boot refuses `entrypoint php` — the very failure the sibling page documents
(`docs/frameworks/php/index.md:25`: "The shim must sit beside the binary") and
that `docs/getting-started.md:21-22` avoids by putting the *directory* on `PATH`.

**Fix:** mirror getting-started — `RUN tar -xzf blastit-*.tar.gz` then
`ENV PATH=/app/blastit-x86_64-unknown-linux-gnu:$PATH` (or copy `blastit` **and**
`*.so` into one directory). Same edit is needed before this tab can be called
"one image per runtime".

## [BLOCKER] 2 — the DigitalOcean Static image calls `curl` on a base that has none

`docs/deploy/digitalocean-paas.md:105-108`

```dockerfile
FROM debian:trixie-slim
WORKDIR /app
COPY . .
RUN curl -fsSLO https://github.com/.../blastit-x86_64-unknown-linux-gnu.tar.gz && ...
```

`debian:trixie-slim` ships neither `curl` nor `ca-certificates`. The PHP tab one
line above installs both; the Static tab does not, so the build dies with
`/bin/sh: 1: curl: not found` — a docs command that cannot work, which is a defect,
not style.

**Fix:** add the same
`RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*`
(or drop the archive fetch and copy a prebuilt binary).

## [MAJOR] 3 — `django.md` silently weakens the admin grants; the admin then gets `sql.denied`

`docs/frameworks/python/django.md:37-42` (base `51b10a1`, same block)

```diff
-        select auth_user(id, password, last_login, is_superuser, username, first_name,
-                         last_name, email, is_staff, is_active, date_joined),
-               auth_group(id, name), auth_permission(id, name, codename, content_type_id),
-               django_content_type(id, app_label, model),
+        select auth_user(id, username, password, last_login, is_superuser, is_staff, is_active)
+        select auth_group(id, name), auth_permission(id, name, codename)
```

Grant directives are **column-level** (`reference/blastfile.md#grants`), so the
Django admin's own changelist (`list_display` selects
`first_name, last_name, email, date_joined` on `auth_user`) and its permission
lookup (`auth_permission.content_type_id → django_content_type`) are now denied.
The page still presents the block as *the* golden path for the admin and the
following "The admin's grants" section claims these are "its tables" — it never
says the list is deliberately short. This is a truth regression introduced for
line width; nothing in the rewrite asked for it.

**Fix:** restore the previous columns/tables (or state explicitly that the
columns are illustrative and the admin's real set comes from `check`/`--learn`).

## [MAJOR] 4 — the new per-runtime tabs promise parity the snippets do not deliver

`docs/deploy/compose-traefik.md:132-146`, `docs/deploy/compose-traefik.md:280-283`,
`docs/deploy/anything-else.md:154-170`

* compose-traefik's PHP tab is one `apt-get install -y libphp-embed php-pgsql` (no
  `apt-get update`, no archive unpack, no shims), yet `:280-283` promises "a PHP
  or static image runs the copied `blastit-init`" — no snippet in the page creates
  that copy, and the pasted `command:` in the compose file is still the Python one.
* anything-else's three "Plain Docker" tabs are 2–4 line fragments with **no
  `COPY . .` and no `CMD`**, then the text says "the `docker run` below is the same
  for all three" — the image built from those tabs has neither the app nor a
  command, so `docker run` starts an interactive `python:3.13-slim`.
* anything-else's PHP tab (`:164`) is `RUN apt-get install -y libphp-embed php-pgsql
  # plus the release archive`: the comment is the only mention of the archive, and
  again there is no `update`, so the layer fails on a slim base.

**Fix:** either give each tab the same 4 working lines the DigitalOcean page has
(base + runtime install + archive unpack with the shims kept together + `CMD`),
or drop the tabs and keep one working snippet per page — a decorative tab is worse
than no tab, because the reader now believes PHP/static need no extra work.

## [MINOR] 5 — `static-and-spa.md:92` contradicts the banner printed on the same screen

"so the banner and the document count none" — the fence six lines above prints
`http · scheduler · dbproxy · 1 route`, and `e2e/static_assets.py` pins that page
line to ` 1 route` on purpose. Only the JSON document counts `0`. Say "the
document counts none" (the banner line is MB.21.5.1/MB.41.3's edit, not this one's).

## [MINOR] 6 — PHP `check` is described as a green no-op; it is exit `2`

`docs/frameworks/php/index.md:93`, `php/laravel.md:77`, `php/symfony.md:69`
("then audits nothing without a pytest suite"). Verified:
`blastit check` on a PHP manifest → `blastit check: ... pytest is not installed`
and **exit `2`**; `reference/blastfile.md#blastit-check` says `2`. A reader who
adds the command to CI gets a red build, not silence. Restore "exits `2` without a
pytest suite" (the old wording was accurate).

## [MINOR] 7 — `reference/cli.md:16-18`: "the same binary from the wheel" is untrue

The wheel links CPython (`crates/blastit-py/Cargo.toml:48`: `features =
["python"]`); the release archive is built `--no-default-features --features
php,static` and refuses a WSGI manifest at exit `1`
(`scripts/build-standalone.sh` header). Same CLI and same file, different
artifacts. Say "the same CLI from the wheel" and keep the PHP/static sentence.

## [MINOR] 8 — `e2e/static_assets.py:13` docstring still names `uvx blastit dev`

The page now says `blastit dev` (and line 17's prose was updated); the module
docstring is the page's contract, so it should follow.

## [MINOR] 9 — trailing whitespace in `inventory.toml`

`python/tests/runnable_docs/inventory.toml:265,285` end with `" ` (space after the
closing quote). Harmless to `tomllib`, but noisy in a file that is otherwise clean.

## [MINOR] 10 — `anything-else.md:213` shows a literal password in a sample env

`DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop` in
`/etc/shop/blastit.env`, while every other page uses `REPLACE_WITH_…`. Pre-existing
(not in this diff), but this node owns the security-honesty sweep: use
`REPLACE_WITH_OWNER_PASSWORD` so no sample ever prints a plausible secret.

## Security honesty — otherwise clean

`blastit dev` is still labelled "single-uid simulation", "Not a sandbox.
Production `blastit` refuses to run like this." (`static-and-spa.md:28-30`), and
`how-it-works.md:139-147` keeps the "no uid boundary in dev" statement. Production
fail-closed is stated (`python/index.md:68-70`, `anything-else.md:44-45`,
`security.md:140+`). Nothing claims blastit terminates TLS, speaks HTTP/2, does
WebSockets, or is an edge proxy: TLS/HTTP-2 are now pinned to the ingress
(`deploy/index.md:3-5`, `anything-else.md#edge-and-tls`, `security.md:44-47`), and
the `501`-on-`Upgrade` claim was removed from both `security.md` and
`troubleshooting.md`. Note the code still answers `501`
(`crates/blastit-http/src/head.rs:309`), so the removed troubleshooting row leaves
a real status undocumented — sanctioned by this node's brief, recording it here
only so the loss is deliberate. No `unwrap`/`expect` concerns (docs only).

## Architecture / dedup

Docs-only node: no globals, no `env::var` outside stage 0, no mock-trait or
guard-script concerns, no new `scripts/check_*.py`. The three deploy pages now
duplicate the same compose-vs-DO command lists, and the duplicates **agree**
(grant/role/pool ordering, `25060` direct vs `25061` pooled, `never swap the two`).
The only divergence is the one finding 4 names.
