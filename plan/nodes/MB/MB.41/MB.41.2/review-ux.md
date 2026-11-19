# review-ux — MB.41.2 (docs: deploy / frameworks / reference / how-it-works / security / troubleshooting)

Base: `48f2952a47cc4fb90543b18a5c7d0115ef607563`..working tree. Voice of record: README,
`docs/{index,getting-started,blastfile,local,ci}.md`.

The framing work is right: `deploy/index.md`, `how-it-works.md`, `security.md` and the
framework connectors now read "Linux application server behind your cloud ingress", the
`## What this platform does not do for you` apologies are gone, `troubleshooting.md` lost its
`501`/WebSocket/HTTP2 row, and the three connectors share one section order
(Install → Blastfile → Static assets → Learn → Check in CI → Deploy). What fails is the new
multi-language tab content: on two of the three deploy pages the tabs are not buildable and the
shared fences behind them are Python-only.

## BLOCKER

1. **`docs/deploy/anything-else.md:161-170` — the PHP and Static tabs are not Dockerfiles that run.**
   PHP is `RUN apt-get install -y libphp-embed php-pgsql    # plus the release archive`: no
   `apt-get update`, and the release archive its own comment promises is never fetched, so the
   image has no `blastit`. Static is a bare `FROM debian:trixie-slim  # plus the release archive`
   with no instruction at all. *Fix:* copy the working shape from
   `digitalocean-paas.md:94-110` (multi-line `RUN apt-get update && apt-get install -y
   --no-install-recommends ca-certificates curl libphp-embed php-pgsql && rm -rf
   /var/lib/apt/lists/*`, then `curl -fsSLO …/blastit-x86_64-unknown-linux-gnu.tar.gz && tar -xzf
   blastit-*.tar.gz`, and keep the extracted directory with its shims rather than one moved file).

2. **`docs/deploy/anything-else.md:172-177` — the "same for all three" `docker run` is Python-only.**
   `--health-cmd 'python -c "import urllib.request;…"'` cannot run in the PHP or static image the
   tabs just built (no CPython), so the container is permanently unhealthy and any probe-based
   restart loop kills it. The prose at :153 asserts the fence is identical for all three runtimes.
   *Fix:* `--health-cmd 'curl -fsS http://127.0.0.1:8080/_blastit/health'` (both non-Python tabs
   already need `ca-certificates curl`), or move the health command into the three tabs.

3. **`docs/deploy/compose-traefik.md:133-172` — language-specific content in the shared non-tabbed
   fence, with the tabs as orphan fragments.** The `=== "Python (WSGI)"` / `=== "PHP"` /
   `=== "Static & SPA"` tabs carry one line each, and the untabbed `` ```dockerfile `` at :148 is a
   complete **Python** image (`FROM python:3.13-slim`, `pip install`, `python3.13-init`,
   `setcap … python3.13-init`). A PHP reader sees a fragment that appears nowhere in the file they
   are told to build. *Fix:* put the whole Dockerfile inside each tab (shared prologue repeated), or
   keep one runtime-agnostic capability dance untabbed and place only the interpreter-install and
   `setcap` target lines in tabs *inside* that fence.

4. **`docs/deploy/digitalocean-paas.md:99` — the PHP image drops the SAPI shim.**
   `mv blastit-x86_64-unknown-linux-gnu/blastit /usr/local/bin/` extracts the archive and keeps one
   file, leaving `libblastit_php*.so` behind; `frameworks/php/index.md:20-22` and
   `getting-started.md:31-32` both state the shim must sit beside the binary, so `entrypoint php`
   refuses at boot (`no shim libblastit_php…`). *Fix:* `cp -a blastit-x86_64-unknown-linux-gnu /opt/blastit`
   + `ENV PATH=/opt/blastit:$PATH` (and the same shape in the Static tab), or copy the archive's
   shims beside `/usr/local/bin/blastit`.

5. **`docs/deploy/digitalocean-paas.md:103-110` — the Static tab calls `curl` without installing it.**
   `FROM debian:trixie-slim` then `RUN curl -fsSLO …` fails at build with `curl: not found`
   (the PHP tab installs `ca-certificates curl`; this one installs nothing). *Fix:* add
   `RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*`
   before the download.

## MAJOR

6. **`docs/frameworks/static-and-spa.md:33` vs `:90-92` — the page now contradicts itself and the tool.**
   The banner fence still prints `http · scheduler · dbproxy · 1 route`, and the new sentence says
   "A mount is not a route, so the banner and the document count none": live `blastit dev` prints
   `0 routes`, and `e2e/static_assets.py:126-141` documents that the page's `1 route` is a stale
   pin whose edit must be deliberate. *Fix:* change :33 to `0 routes` and update the two pins in
   `e2e/static_assets.py:366-384` in the same commit — or, minimally, drop "the banner and" so the
   sentence only describes the `--json` document.

7. **`docs/deploy/compose-traefik.md:281-283` — invents a binary no artifact defines.**
   "a PHP or static image runs the copied `blastit-init`" names nothing in the repo, docs or ADR,
   and `compose.yaml:260` shows only `/usr/local/bin/python3.13-init -m blastit`. *Fix:* "a PHP or
   static image copies the archive's `blastit` to `blastit-init`, gives that copy the same file
   capabilities and runs it" — and show the `command:` it means.

8. **`docs/frameworks/python/django.md:38` — the admin's grant drops the table every admin page reads.**
   The rewrite removed `django_content_type(id, app_label, model)` and `auth_permission(…,
   content_type_id)`; the admin's `ContentType` manager reads `django_content_type` on the first
   page, so the documented golden path reports `missing_grant` immediately — which the prose at
   :78 then half-admits ("Add the join tables when `check` reports them"), the one-denial-at-a-time
   workflow this release is supposed to end. *Fix:* restore `select django_content_type(id,
   app_label, model)` and `select auth_group(id, name), auth_permission(id, name, codename,
   content_type_id)`, one table per line.

## MINOR

* `docs/reference/settings.md:111` — honest reference, keep the fact, drop the edge-diagram relic:
  "Blastit speaks HTTP/1.1; your cloud ingress terminates TLS and HTTP/2. A request carrying
  `Upgrade` is refused `501`." (`crates/blastit-http/src/head.rs:308-309` proves it.) Not an
  apology, so it stays on the page — just say cloud ingress, not "a load balancer".
* `docs/frameworks/index.md:41` — PHP's CI gate "plus an HTTP smoke run" names nothing a reader can
  run; the PHP pages say the gate is `blastit fmt --check` plus the denial log. Use that.
* `docs/deploy/digitalocean-paas.md:59-214` — steps 2-6 are the Django path (`python manage.py
  migrate` at :188, `ALLOWED_HOSTS`); add one line under `### 1. Image and manifest` scoping the
  tabs to the image and saying a static site skips the cluster/pool/migrate steps.
* `docs/deploy/anything-else.md` (whole page) — no Mermaid diagram on a deployment guide; one
  `flowchart LR` of the three targets (Docker → ingress → Blastit → PgBouncer → PostgreSQL) would
  match every sibling.
* `docs/deploy/compose-traefik.md:298` — "Before the first deploy" is a plain paragraph while
  `digitalocean-paas.md:224` uses `!!! warning "Before the first deploy"`; wrap it for parity.
* `docs/how-it-works.md` — 156 lines, past the 150-line ceiling; `## Dev mode`'s last paragraph
  (:149-151) can fold into the first.
* `e2e/static_assets.py:13` — docstring still says "the banner fence (`uvx blastit dev`)"; the page
  is `blastit dev` now.
* `python/tests/runnable_docs/inventory.toml:265,285` — trailing whitespace after the new `reason`
  strings (`git diff --check` fails).

## Checked and clean

Tabs are exactly `=== "Python (WSGI)"` / `=== "PHP"` / `=== "Static & SPA"` with 4-space bodies on
all three pages; no `uvx`, no `version = 2`, no `blastit.toml`, no `php-fpm`, no empty `postgres {}`
used as a working example, no 6-glob staircases, no diary headings; `blastit dev --help` and
`blastit check --help` confirm every flag in `reference/{cli,blastfile}.md` (`--learn`, `--dry-run`,
`--json`, `-- ARGS…`, `--preset`, `--version`); `uv run --no-project e2e/static_assets.py
--self-test` passes 17/17, so every fence on `static-and-spa.md` is still replayable.
