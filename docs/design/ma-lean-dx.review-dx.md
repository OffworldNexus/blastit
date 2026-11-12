# DX review — Blastit 0.1.1 user documentation

Lens: **`dx` (human developer experience)**. Subject: the public docs for the Python WSGI + PostgreSQL
0.1.1 scope, written docs-first. Read-only review; this file is the only artefact.

Sources read: the 17 pages named in the brief, plus `docs/VISION.md`, `docs/how-it-works.md`,
`docs/security.md`, `docs/reference/log-events.md`, `plan/nodes/MA/MA.1/NODE.md`, `zensical.toml`,
and the repo's `e2e/rootless/{Dockerfile,compose.yml}` only to verify build facts.

## Step counts (the brief's first question)

| Flow | What the docs present | Actions the user actually performs | Verdict |
|---|---|---|---|
| (a) Laptop | 3 numbered steps (`getting-started.md:5,15,38`) | 6: `uv init --app`, `uv add blastit`, write `hello.py`, write `blastit.toml`, `uv run blastit dev`, `curl` — plus **3 undocumented** to add PostgreSQL (get a server, create `shop`, export `DATABASE_URL`) | Nominally ≤ 3; really 6. Zero `sudo` confirmed: the acceptance grep is clean and no local/CI/guide page shows `sudo`/`setcap`/`chmod` (deploy pages do, correctly). |
| (b) CI/CD | 1 command (`ci.md:6`) | 6: have a pytest suite, commit `uv.lock`, add a workflow, add a PostgreSQL service, set `DATABASE_URL`, run — the supplied workflow is not a valid workflow (see F7) | "One command" is true only after five setup actions that the page never counts. |
| (c) Deployment | 1 preset flag (`deploy/index.md:15`) | DO: 5 actions / 2 files (build, push, `app.yaml`, `doctl apps create`, manifest edit). Compose: 8 actions / 3 files / 156 lines | DO is close to a recipe; Compose is three files and two screens, not the "one-screen recipe" the node promises. |

## BLOCKER

### F1 — The landing-page manifest cannot boot: a grant on an undeclared proxy
`docs/index.md:20-31` shows `version`, `app`, `[[route]]` and `[route.db.tables] products = …`, but no
`[proxy.db]`. `docs/reference/manifest.md:15` says "a grant on an undeclared proxy aborts the boot";
`:254` repeats it as a boot abort. The first manifest a visitor copies fails before serving a request.

**Fix:** add the three-line `[proxy.db]` block already used at `docs/local.md:30-32`, or make the
homepage example grant-free. This is the highest-traffic code block in the project.

### F2 — The SQLAlchemy sample cannot connect: psycopg3 installed, psycopg2 dialect selected
`docs/frameworks/flask.md:83-90`:

```python
engine = create_engine(os.environ["DATABASE_URL"], poolclass=NullPool)
```

The install line at `:9` adds `psycopg[binary]` (psycopg 3), never `psycopg2`. SQLAlchemy's
`postgresql://` defaults to the psycopg2 dialect, so this raises `ModuleNotFoundError: No module named
'psycopg2'`. The bullet at `:90` then tells the user to use `postgresql+psycopg://`, which they cannot
put into `DATABASE_URL`: Blastit rewrites that variable using the default template
`postgresql://{{ user }}@/…` (`docs/reference/manifest.md:126`). The documented SQLAlchemy path is
broken twice over.

**Fix:** make the code authoritative:
`create_engine(url.replace("postgresql://", "postgresql+psycopg://", 1), poolclass=NullPool)`, and
either show that replacement in `[proxy.db.output]` or state that the app must do it. Do the same for
Flask-SQLAlchemy at `:98-100`.

### F3 — The Compose image build fails: `setcap` is not in the base image
`docs/deploy/compose-traefik.md:116-134` runs `setcap` in a `python:3.13-slim` image with no package
install. Debian slim does not ship `libcap2-bin`; the repo's own rootless image installs it for this
exact reason (`e2e/rootless/Dockerfile:54-56`). A copy-paste build stops at `setcap: not found` before
the deployment discussion begins.

**Fix:** add, before the `RUN useradd …`:

```dockerfile
RUN apt-get update && apt-get install -y --no-install-recommends libcap2-bin \
    && rm -rf /var/lib/apt/lists/*
```

### F4 — "Measure your ingress" is impossible: Blastit loads one WSGI callable, not one per route
`docs/deploy/index.md:36-56` says "Add this view and route" and labels `whoami.py` as an
`application`, then adds `[[route]] name = "whoami"`. But `docs/reference/manifest.md:12` has one
global `app` and `docs/frameworks/index.md:3` says "Blastit loads one callable". A second module is
never imported; `/whoami` is routed to the real app and 404s. Three pages depend on this procedure
(`anything-else.md:10`, `fair-queuing.md:33`, `troubleshooting.md:66`).

**Fix:** show the view added to the existing app (Django `urlpatterns`, Flask `@app.get`), or define a
first-class `blastit dev --whoami` / ingress-probe command in the docs. Do not invent a second `app`.

### F5 — "No credentials in the app" contradicts the security page
`README.md:11-14` and `docs/index.md:47` promise "No credentials in the app … the real database URL
stays in a proxy". `docs/security.md:14-15` says the opposite: "code the app runs **at import time** —
imports run before the sandboxes exist, with the real credentials still in the environment".
`docs/how-it-works.md:104-111` says stage 1 imports the app secret-free. The stale paragraph directly
contradicts VISION §1 and the headline the whole product sells.

**Fix:** correct `security.md:14` to match `how-it-works.md` (imports happen in secret-free stage 1;
the real residual risk is non-DB secrets, which `:15` already covers). Security lens should confirm
the grade; from a DX/trust standpoint this is a blocker.

## MAJOR

### F6 — The deployment overview misstates the DO preset
`docs/deploy/index.md:24-25`: "Both presets trust every peer (`*`) and read the **rightmost** forwarded
entry". `docs/deploy/digitalocean-paas.md:11,19-20` says the opposite: DO reads `do-connecting-ip`
because the edge overwrites it, and "`X-Forwarded-For` is appended to, so the preset does not read
it." A reader cannot tell which header is actually trusted on the flagship platform.

**Fix:** say "Both trust every peer; DO reads `do-connecting-ip`, Compose reads the rightmost
`X-Forwarded-For`", and link the two tables.

### F7 — The GitHub Actions snippet is not a valid workflow
`docs/ci.md:91-112` is presented as the complete `.github/workflows/check.yml` but starts at `jobs:`;
there is no `on:` trigger, so GitHub will not run it. `uv sync --frozen` also assumes a committed
`uv.lock`, which the page never requires.

**Fix:** ship a complete file (`name:`, `on: [push, pull_request]`, `permissions: contents: read`),
and add "commit `uv.lock`" to the setup list. This is the only CI integration the docs offer.

### F8 — The canonical `blastit check` output contradicts its own JSON
`docs/ci.md:27` prints `8/8 routes`, while `:47` prints `"routes": {"total": 8, "exercised": 7}`, and
`:52` reports an `unexercised_route` for `checkout`. The human summary lists four problems but omits
the unexercised `checkout` finding that only the JSON has. A user (or an agent diffing the two outputs)
cannot tell whether 8/8 passed.

**Fix:** make both outputs the same run: human summary `7/8 routes`, a `route checkout: never
exercised` line, and the same four findings in both.

### F9 — The `--learn` examples don't add up
`docs/local.md:59-63`: `--learn` reports "2 routes, 41 statements, 2 table grants", and the very next
`check` reports `3/3 routes`. `docs/frameworks/django.md:97-99` has a third shape ("3 statements, 2
table grants") with no route count. The numbers are the evidence that "nothing was forgotten"; when
they move between two consecutive commands, the evidence looks fabricated.

**Fix:** use one stable example per page (same route/statement totals before and after), and one
output grammar across `local.md`, `django.md` and `ci.md`.

### F10 — Log format default disagrees across three pages
`docs/reference/settings.md:36` says `BLASTIT_LOG_FORMAT` defaults to `json`; `docs/local.md:13` says
"prints pretty logs"; `docs/reference/cli.md:28-29` says "Logs are pretty by default;
`BLASTIT_LOG_FORMAT=json` overrides that." A developer setting the variable from the reference cannot
predict what `blastit dev` will print, and `deploy/index.md` asks them to read boot events.

**Fix:** document the per-command default explicitly: "`json` in production, `pretty` in
`blastit dev`/`check`; the variable wins in both." One table row, no ambiguity.

### F11 — The Traefik recipe has no working TLS path
`docs/deploy/compose-traefik.md:29-46` attaches one router to `entryPoints: [web, websecure]` with
`tls: {}` and no certificate resolver. Traefik then attempts TLS on `:80` (plain HTTP fails) and
serves its self-signed default on `:443`; browsers warn, and the page's claim that
`wsgi.url_scheme` is `https` (line 19) is true only behind a certificate users were never told to
configure. The repo's own e2e file uses two routers for this reason (`e2e/rootless/traefik.yml`).

**Fix:** split `web` (redirect) and `websecure` (TLS) routers, add a `certificatesResolvers` block (or
an explicit "bring your own cert/ACME" callout), and show the redirect middleware.

### F12 — "Measure your ingress" is ordered before the thing that creates the ingress
`docs/deploy/index.md:31-32` says "Do it before you write the configuration"; `:36` says "ask for it
through the real ingress". You cannot measure your real ingress before deploying behind it, and the
two bundled presets already know their headers (`digitalocean-paas.md:11`,
`compose-traefik.md:11`). This sends every "anywhere else" user in a circle.

**Fix:** scope the procedure to the "Anything else" page and order it "deploy once behind the ingress →
hit `/whoami` → set `BLASTIT_CLIENT_IP_HEADER`". Remove it from the preset checklist.

### F13 — Local PostgreSQL appears from nowhere
`docs/local.md:42-45` jumps to `export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop`, and
`docs/frameworks/flask.md:74` / `django.md:88` do the same. No page says how to get a server, create
the `shop` database or the `shop` role. This is the first real app step and the first place a laptop
run fails.

**Fix:** one canonical line under "Add PostgreSQL": e.g. `docker run --rm -e POSTGRES_PASSWORD=secret
-e POSTGRES_DB=shop -p 5432:5432 postgres:18` (or the distro's `createdb`), then the export. Note that
Docker on Linux may need the `docker` group; if that is not "zero sudo", say so.

### F14 — No `.env` story, despite claiming "no environment variables"
Every guide uses `export DATABASE_URL=…` (`local.md:43`, `flask.md:74`, `django.md:88`) and the README
says "no environment variables" (`README.md:47`). A modern 12-factor project keeps `.env` and expects
the dev server to load it; `blastit dev` will abort with "an input variable is missing", and neither
`local.md` nor `troubleshooting.md:18` tells them how to load it.

**Fix:** add one row: "Blastit reads the process environment only. Load `.env` with
`uv run --env-file .env blastit dev` (or export it)." Then state whether `.env` is deliberately not
auto-read.

### F15 — The Django guide omits `ALLOWED_HOSTS` (and the proxy-scheme setting)
`docs/frameworks/django.md:10-15` claims "Two settings" and never sets `ALLOWED_HOSTS`. A production
Django app with `DEBUG=False` returns `400 DisallowedHost` for every request, including the first
smoke test after following the guide. `SECURE_PROXY_SSL_HEADER` is only mentioned in
`troubleshooting.md:69`; `CSRF_TRUSTED_ORIGINS` is absent.

**Fix:** make it three settings and show
`ALLOWED_HOSTS = [os.environ["ALLOWED_HOST"]]` (or `["*"]` only behind a trusted ingress) plus
`SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")` for TLS deployments; mention
`CSRF_TRUSTED_ORIGINS` once.

### F16 — The Django quickstart normalises two escape hatches
`docs/frameworks/django.md:61-65` gives `/admin/.*` `unrestricted = true`, and `:107-109` says
`--learn` puts shared statements into a `[group.<name>]` — the demo's session group contains
`auth_user` (including `password`) selects. `docs/local.md:98-99` tells users "Do not reach for
`unrestricted = true` first: it gives away every statement … including the ones reading password
hashes." The first manifest a Django user copies does exactly that, twice, and attaches the
password-hash grant to routes that share the group.

**Fix:** move the admin route out of the quickstart (or make it a boxed "last resort" example with the
`manifest.warning` cost); show `groups = ["session"]` attached only to authenticated routes (as the
repo's `e2e/demo/blastit.toml` does for `login`/`logout`/`notes`), never as a global group.

### F17 — `blastit check` requires pytest, and "no tests" is mapped to "suite failed"
`docs/ci.md:9` says it runs `pytest -q`; `docs/ci.md:69-74` says "Your suite"; `troubleshooting.md:34`
maps a suite failure to "fix the suite". Nothing says pytest must be installed, and a project with no
tests exits 5, which the docs classify as "a finding, or the suite failed" (`ci.md:66`). A first-time
CI run with `blastit check` on the Getting Started project fails with no applicable fix.

**Fix:** add a prerequisites line ("`uv add --dev pytest`; a suite that exercises every route") and a
troubleshooting row: "pytest exit 5 / no tests collected → add a test, or use `blastit dev --learn`".
If there is a no-suite mode, document it.

### F18 — The one error the product lives on is not actionable
`docs/local.md:79-99` shows a truncated pretty line, then says "`sql` is the statement as normalised —
exactly what a grant has to contain … `reason` separates …" — but the sample line has no `reason`
field and ends in `…`, so the user cannot copy the statement the grant must contain. The "fix" is
prose ("Add it to `queries`"), not the exact TOML edit. It also claims the request fails "with a
`500`"; `docs/how-it-works.md:73` says the app receives SQLSTATE `42501`, which Django/Flask may turn
into any status (or catch).

**Fix:** show the complete line with `reason`, then the literal edit:
`[route.products.db] queries = ["SELECT …"]`. Say "the app receives a PostgreSQL permission error
(42501); an uncaught one is a 500" instead of asserting the status. Add one sentence that `--learn`
prints the same normalisation.

### F19 — The DO page says "Add this" where it must say "replace this"
`docs/deploy/digitalocean-paas.md:92-99` says "Add this to the `blastit.toml` from the Django guide"
and shows a second `[proxy.db]` table. Appending it produces a duplicate TOML table and a boot abort;
the user must replace the existing `[proxy.db]` block.

**Fix:** "Replace the `[proxy.db]` block from the Django guide with:" and show the full replacement.

### F20 — The DO/Django deploy leaves the view-memory estimate at the generic default
`docs/deploy/anything-else.md:14` says `BLASTIT_VIEW_MEM_ESTIMATE` defaults to `32 MiB` and "Django:
about 140 MiB". The DO spec (`digitalocean-paas.md:73-86`) sets no memory limit and no estimate; the
derived cap is computed from 32 MiB while each Django view really costs ~4× that, which is the
documented OOM recipe (`troubleshooting.md:79`). The compose page has the same omission.

**Fix:** set `BLASTIT_VIEW_MEM_ESTIMATE=140MiB` (and a memory limit) in both presets' examples, or
derive it in the preset and say so.

### F21 — `blastit dev --host` exists in the promised behaviour but not in the docs
`docs/local.md:13` and `docs/reference/cli.md:23` say `blastit dev` binds `127.0.0.1` only; the
security model says dev "refuses to bind non-loopback addresses unless `--host` is explicitly passed"
(`security-invariants`; no public page mentions the flag). A developer in a VM, container or on a phone
has no documented way to reach the server, and the CLI reference is supposed to be complete.

**Fix:** document `--host ADDR` (with the warning) in `cli.md` and one line in `local.md`; add the
"cannot reach it" symptom to troubleshooting.

### F22 — "No grant" and "an empty grant" are conflated
`docs/local.md:51`: "A route with no grant for a proxy gets no listener, so `queries = []` denies every
statement". Those are two different states with two different failures: no `[route.db]` → no socket →
`No such file or directory` (`troubleshooting.md:54`); `[route.db] queries = []` → a socket that denies
every statement. `flask.md:64` says the empty list "is the point", which is the opposite advice.

**Fix:** state both states in one two-row table ("omitted → no listener, app sees ENOENT; empty →
listener, every statement denied") and make the quickstarts pick one deliberately.

### F23 — The Kubernetes example trusts a pod CIDR the settings page calls a footgun
`docs/deploy/anything-else.md:140-141` sets `BLASTIT_TRUSTED_PROXIES=10.244.0.0/16`, while
`docs/reference/settings.md:128-131` warns that naming a network makes the resolver compare from the
right and fall back to the **leftmost, client-invented** entry when every entry is inside it. An
ingress-controller hop plus a client-supplied `X-Forwarded-For` inside the pod CIDR is exactly that
case.

**Fix:** use `*` with "publish no port" as the presets do, or show the precise ingress-pod address and
repeat the fallback warning next to the example.

## MINOR

- **Three drifting quickstarts.** `README.md:19-45`, `docs/index.md:18-37` and
  `getting-started.md:5-50` each show a different app (`hello:application`, `myshop.wsgi:application`,
  `hello:application`) and different commands. One canonical quickstart in `getting-started.md`;
  README/index should link to it. The README's last console block also puts `$ curl` directly after
  the blocking `blastit dev`, implying one terminal.
- **Install after the code.** `docs/frameworks/django.md:8-29` shows `import dj_database_url` before
  the `uv add … dj-database-url` that provides it. Swap the sections; a top-to-bottom reader currently
  gets `ImportError`.
- **`psycopg2` is dismissed without a path.** `docs/frameworks/index.md:29` says "not usable: it
  cannot bind server-side" with no migration pointer. Add "install `psycopg[binary]`; Django 4.2+ and
  SQLAlchemy pick it up with `postgresql+psycopg://`".
- **Bare `python` in a command.** `docs/reference/cli.md:86` says
  `python -m blastit.fold …`; the repo forbids bare `python` and it may resolve to the wrong
  interpreter. Use `uv run python -m blastit.fold …`.
- **VISION still promises a command the docs say does not exist.** `VISION.md:42` lists
  `blastit test` / `blastit check`; `cli.md:48` says "It is the only CI command: there is no
  separate `blastit test`." Reconcile VISION (or drop the alias promise) so MA.5 does not build it.
- **Length and ordering.** `deploy/compose-traefik.md` (156 lines) and `deploy/anything-else.md`
  (158) exceed the ~150-line guide target; `fair-queuing.md:1-29` and `compose-traefik.md:7-25` lead
  with theory/warnings before any runnable action. Trim or move the caveats below the first command.
- **Vague troubleshooting rows.** `troubleshooting.md:14` ("the manifest, the environment rewrite or
  the db sockets") and `:21` ("move the interpreter somewhere world-readable") name no exact fix or
  command. Replace with the concrete `BLASTIT_*` / `chmod`/path action or link the section that has it.
- **Two canonical ways to select a preset.** `deploy/index.md:15-16` uses `--preset`; every preset page
  sets `BLASTIT_PRESET` in the environment (`digitalocean-paas.md:84`, `compose-traefik.md:56`). Pick
  one for tutorials (the env var, because containers use it) and mention the flag in the reference.
- **Dev privilege mode is not in the event model.** `settings.md:49-51` says the `boot` event reports
  the privilege mode, but `log-events.md:46-48` only enumerates `root`/`capabilities`. The security
  invariant's `unprivileged-dev` value is absent, so an operator diffing dev vs prod logs cannot tell
  them apart. Add the third value (or say dev prints a distinct `mode`).

## Verdict

The docs are lean and the zero-`sudo` promise holds, but three copy-paste recipes (homepage manifest,
SQLAlchemy, Compose image) fail outright and the ingress-measurement procedure cannot be executed at
all; fix those five blockers before MA.5 treats these pages as the design source of truth.
