# E2 — documentation site plan

What the site is, who reads it, and which page answers which question, in the
order that reader asks it. Every question has exactly one home: the other
places that touch it link there instead of repeating the prose.

`ARCHITECTURE.md` is the specification and the only source of truth for every
fact on the site; where the code and the architecture disagree, the code wins
(see "Code versus docs" at the end, repeated in the pull request body).

## Readers

| # | Reader | What they bring | What they need from us |
|---|---|---|---|
| (a) | a Django developer adopting blastit in an existing project | a working Django app, pytest-django, a `DATABASE_URL` | the two settings Django needs, the first `blastit.toml`, and the loop that fills the grants in |
| (b) | a Flask developer, or anyone on another WSGI framework | a WSGI callable and a database | what blastit needs from an app, and what to do where the Django tooling does not apply |
| (c) | the person deploying it | a platform: App Platform, Docker Compose behind Traefik, or something else (Kubernetes, a VM with systemd, nginx in front) | which preset, what it assumes, and the parameters for everything else |
| (d) | a security reviewer | the isolation claims and a suspicion of marketing | the trust boundaries, the mechanisms, and what is *not* covered |
| (e) | someone evaluating in 5 minutes | curiosity and a browser tab | what it is, what it costs, and whether their app even qualifies |

## The pages

Menu order is deliberate: the two orthogonal sections — *fit it to your
framework* and *fit it to your infra* — sit on top of the classic ones.

| Page | Answers |
|---|---|
| `index.md` — **Home** | what blastit is, why, who it is for and who it is not for |
| `getting-started.md` — **Getting started** | the shortest real path to a request served by blastit |
| `frameworks/index.md` — **Fit it to your framework** | what blastit needs from an app; how routes map to URL paths; ASGI |
| `frameworks/django.md` | the Django lifecycle, install → manifest → run → test → live server |
| `frameworks/flask.md` | the same for Flask and other WSGI frameworks, with what changes |
| `frameworks/day-to-day.md` | everyday life once it is installed, and upgrading |
| `infra/index.md` — **Fit it to your infra** | picking a preset |
| `infra/digitalocean-paas.md` | `digitalocean-paas`: what it sets, what each value assumes, a full app spec |
| `infra/compose-traefik.md` | `compose-traefik`: the same, with a compose file and the rootless capability recipe |
| `infra/anything-else.md` | the parameters, and how they articulate, for any other ingress |
| `reference/settings.md` — **Reference** | every `BLASTIT_*` variable and `PORT` |
| `reference/cli.md` | `blastit`, `blastit-manifest`, `python -m blastit`, `python -m blastit.fold` |
| `reference/manifest.md` | the manifest format, key by key |
| `reference/log-events.md` | the event stream, event by event |
| `reference/pytest-plugin.md` | the pytest plugin's options and report |
| `how-it-works.md` — **How it works** | the tour, with diagrams, linking to `ARCHITECTURE.md` |
| `security.md` — **Security model** | the trust boundaries and the mechanisms |
| `troubleshooting.md` — **Troubleshooting** | what a symptom means |
| `changelog.md` — **Changelog** | placeholder until E4 adds `CHANGELOG.md` |

## Question → home

### (a) Django developer

| # | Question | Home |
|---|---|---|
| 1 | What is blastit, in one paragraph, and what does it cost me? | `index.md#what-it-is` |
| 2 | Does my app qualify? (Linux, CPython, WSGI) | `index.md#who-it-is-for` |
| 3 | What must I change in the project itself? | `frameworks/django.md#two-settings` |
| 4 | How do I install it? | `frameworks/django.md#install` |
| 5 | Which URLs become which routes? | `frameworks/index.md#routes-and-url-paths` |
| 6 | What does the first `blastit.toml` look like? | `frameworks/django.md#first-manifest` |
| 7 | What is a proxy, and where does `DATABASE_URL` go? | `reference/manifest.md#proxies` |
| 8 | Exact `queries`, or `tables`? | `reference/manifest.md#grants` |
| 9 | When do I need a group, and when `unrestricted`? | `reference/manifest.md#groups` |
| 10 | How do I run it on my laptop? | `frameworks/django.md#run-it-locally` |
| 11 | How do I fill the grants in from my test suite? | `frameworks/django.md#fill-in-the-manifest` |
| 12 | How do I check the traffic a browser makes? | `frameworks/django.md#the-live-server` |
| 13 | I added a view / a query. Now what? | `frameworks/day-to-day.md#adding-a-view` |
| 14 | I got a denial in development. | `frameworks/day-to-day.md#a-denial-in-development` |
| 15 | I got a denial in production. | `frameworks/day-to-day.md#a-denial-in-production` |
| 16 | How do I get the grants out of a production log? | `frameworks/day-to-day.md#fold-the-log` |
| 17 | How do I upgrade blastit? | `frameworks/day-to-day.md#upgrading` |
| 18 | Where do I put it, and does it fit my platform? | `infra/index.md#picking-a-preset` |

### (b) Flask developer, or another WSGI framework

| # | Question | Home |
|---|---|---|
| 1 | Does blastit need Django? | `frameworks/index.md#no-framework-integration` |
| 2 | What does blastit require of my app? | `frameworks/index.md#what-blastit-needs-from-an-app` |
| 3 | Do the paths blastit matches have to be my Flask routes? | `frameworks/index.md#routes-and-url-paths` |
| 4 | Is there a Flask install and manifest path? | `frameworks/flask.md#install-and-first-manifest` |
| 5 | Does the pytest plugin work for Flask? | `frameworks/flask.md#no-pytest-plugin-for-flask` |
| 6 | Then how do I fill the manifest in? | `frameworks/flask.md#filling-the-manifest-without-the-plugin` |
| 7 | What do connected pools and inlined parameters do to me? | `frameworks/flask.md#database-clients` |
| 8 | Can I use FastAPI / ASGI? | `frameworks/index.md#asgi-and-fastapi` |
| 9 | And after the first deploy? | `frameworks/day-to-day.md#everyday-life` |

### (c) The person deploying it

| # | Question | Home |
|---|---|---|
| 1 | Which preset is mine? | `infra/index.md#picking-a-preset` |
| 2 | What exactly does a preset set, and what is it overriding? | `infra/index.md#what-a-preset-is` |
| 3 | On App Platform: what does `digitalocean-paas` set, and why is trusting every peer safe? | `infra/digitalocean-paas.md#what-it-sets` |
| 4 | Give me the app spec and the Dockerfile. | `infra/digitalocean-paas.md#a-full-example` |
| 5 | Behind Traefik: what does `compose-traefik` set, and why `x-forwarded-for`? | `infra/compose-traefik.md#what-it-sets` |
| 6 | Give me the compose file and Traefik's config. | `infra/compose-traefik.md#a-full-example` |
| 7 | How does rootless mode work, exactly? | `infra/compose-traefik.md#rootless` |
| 8 | My ingress is not one of those two. Which client-IP header do I set? | `infra/anything-else.md#client-ip-and-trusted-proxies` |
| 9 | Root, or capabilities? | `infra/anything-else.md#root-or-rootless` |
| 10 | How many workers and view processes? | `infra/anything-else.md#memory-and-process-caps` |
| 11 | Should I turn fair queuing on? | `infra/anything-else.md#fair-queuing` |
| 12 | How do I get the logs out, and what is in them? | `infra/anything-else.md#logs` |
| 13 | What is the uid layout, and does it collide with mine? | `infra/anything-else.md#the-uid-layout` |
| 14 | Kubernetes / plain Docker / systemd / nginx in front — show me. | `infra/anything-else.md#worked-examples` |
| 15 | Which health path do I point the platform at? | `reference/settings.md#port-and-blastit_health_path` |
| 16 | It will not boot. | `troubleshooting.md` |

### (d) Security reviewer

| # | Question | Home |
|---|---|---|
| 1 | What is the threat model, and what is explicitly out of scope? | `security.md#threat-model` |
| 2 | What does a compromised route get, and what can it not do? | `security.md#what-a-compromise-buys` |
| 3 | How is a route's identity established on the database socket? | `how-it-works.md#identity-not-passwords` |
| 4 | How are statements checked — exactly? | `reference/manifest.md#how-a-statement-matches` |
| 5 | What does `unrestricted` give away? | `reference/manifest.md#unrestricted` |
| 6 | Could a log line leak a secret or a parameter? | `reference/log-events.md#what-is-never-logged` |
| 7 | Can a client forge its own address? | `infra/anything-else.md#client-ip-and-trusted-proxies` |
| 8 | How is the secret kept out of application memory? | `security.md#the-credential-barrier` |
| 9 | Which claims are *measured*, and where? | `security.md#evidence` |

### (e) Evaluating in 5 minutes

| # | Question | Home |
|---|---|---|
| 1 | What is it? | `index.md#what-it-is` |
| 2 | Why would I want it? | `index.md#why` |
| 3 | Is it for me? Is it for my language, my platform, my framework? | `index.md#who-it-is-for` |
| 4 | What does it look like in practice? | `index.md#what-it-looks-like` |
| 5 | What is the release status? | `index.md#status` |
| 6 | Show me the shortest path. | `getting-started.md` |
| 7 | How much of my own code does it change? | `frameworks/index.md#what-blastit-needs-from-an-app` |

## Decisions taken while writing

* **`docs_dir` is `docs`, with `exclude_docs` for the material that is not a
  page.** `docs/brand/` holds `brand.css`, `logo.svg` and the favicons, and
  `README.md` and `ARCHITECTURE.md` already link into it; duplicating them into
  a second source tree would let the two copies drift, and moving them would
  break those links. So `archive/`, `spikes/` and the brand working files
  (`mascot-artwork.md`, `check_seams.py`, the large reference PNGs) are
  excluded, and the brand assets are served from where they already live.
* **`ARCHITECTURE.md` is linked, not copied.** It is the specification and it
  has to stay in one place; a copy in `docs/` would go stale on the next PR.
  `how-it-works.md` is a tour that links to the real file on GitHub. (Its
  relative links into `docs/archive/` and `docs/spikes/` would also have to be
  rewritten to be includable as a page.)
* **Diagrams**: Mermaid, with `pymdownx.superfences` custom fences, which is
  Zensical's native Mermaid support.
* **Framework differences**: content tabs (`pymdownx.tabbed`), so Django and
  Flask sit in the same place instead of in two parallel pages.
* **`docs-site-plan.md` is at the repository root, not under `docs_dir`**, so it
  is not published as a page.

## Code versus docs

Checked against `crates/`, `python/` and `e2e/` while writing. Each item is a
place where the code and a document would say different things; the site
follows the code.

| Where | The document says | The code says |
|---|---|---|
| `ARCHITECTURE.md` §2.4, §6 | a catch-all is `path = ".*"` (a string) | `path` is an array: `path = ['.*']`. `parse.rs`'s `string_list` refuses a string (`WrongType: expected an array`). |
| `ARCHITECTURE.md` §8.2 | the whole configuration reference | `BLASTIT_FRAME_MAX` (default 1 MiB) and `SERVER_PORT` are read by `config.rs` and are in neither table. |
| `ARCHITECTURE.md` §2.1, §8.2 | the capability set | `BLASTIT_EXEC` is read in `host.rs`, not `config.rs`, and only a standalone install uses it. |
| `README.md` | `blastit --preset digitalocean-paas` | both `--preset NAME` and `--preset=NAME` are accepted, and the last one wins (`cli.rs`); the site documents both. |
| `ARCHITECTURE.md` §6 | `[route.db] functions = [...]` next to `[route.db.tables]` | both are keys of the same grant table; `functions` is a key of `[route.<proxy>]`, not of `tables` (`parse.rs`). |
| `ARCHITECTURE.md` §10 | *"Root, PID 1, Docker-default capabilities minus `NET_RAW`"* is what App Platform gives the container | measured, not asserted: the live e2e reads `CapEff` through `/debug/caps/` and `e2e/live.py` asserts the preset, not the capability mask. Treated as measured by the spike (§9.1 #1), quoted as such. |

Also worth saying in the docs because the code makes it true, and the README
does not: **the pytest plugin is Django-only.** It defines "a request" as
`django.core.handlers.base.BaseHandler.get_response`; a Flask or FastAPI app
has no such call, and asking it for a manifest there stops the session with
`ERROR: blastit: the plugin needs Django in the test environment to record
requests: No module named 'django'`. A Flask project fills its manifest from
`blastit-manifest fold` instead.

## Found by running it, and what the reviewers changed

The first nine came out of following the site's own pages with a wheel built from
this checkout. **The first eight are being fixed in code by E2b**, so the site now
describes the fixed behaviour and marks each such sentence with an `<!-- E2b -->`
comment in the Markdown (invisible on the page) for whoever checks it once E2b
lands. The rest are findings about the site itself, and are fixed here.

| # | What happened | How the site says it now |
|---|---|---|
| E2b-1 | The pytest live server hands `WSGI_APPLICATION` to `BLASTIT_APP` as it stands, but Django defines that setting as a *dotted* path (`myshop.wsgi.application`), so the boot failed with ``BLASTIT_APP: must be `module:attribute` `` | [Django, the live server](docs/frameworks/django.md#the-live-server) says it works with the `startproject` layout, marked `<!-- E2b -->` |
| E2b-2 | The live server ran with its cwd and `PYTHONPATH` set to the **directory of the settings module**, so a packaged settings module left `myshop` unimportable | same |
| E2b-3 | A **relative** `blastit_manifest` broke the live server, which runs in another directory (`blastit.toml: No such file or directory`) | same |
| E2b-4 | `BLASTIT_APP` had to be importable and `python -I` drops `PYTHONPATH`, so a project that was not installed needed `PYTHONPATH=$PWD` | [Getting started](docs/getting-started.md#3-start-it) and [Troubleshooting](docs/troubleshooting.md) say the directory blastit was started from is on `sys.path`; the run command has no `PYTHONPATH` |
| E2b-5 | A Python under a private home directory made a **view** lose the standard library (`ModuleNotFoundError: No module named 'stringprep'`) | refused at boot, naming the interpreter and the fix — [Getting started](docs/getting-started.md#1-install-it) |
| E2b-6 | A `SELECT set_config(…)` granted by a **global** group was reported in `unused_global` although `connection_setup` showed `granted_in: 2` | [The plugin reference](docs/reference/pytest-plugin.md) — connection-setup statements are reported once, and are not dead grants |
| E2b-7 | `REMOTE_ADDR` carried the peer's port as `ip:port` | a bare IP, with the port in `REMOTE_PORT` — [Anything else](docs/infra/anything-else.md#client-ip-and-trusted-proxies), [Settings](docs/reference/settings.md) |
| E2b-8 | `BLASTIT_LOG_FORMAT=pretty` did not print child events, and `fold --write` refused a route whose grant was only `[route.<proxy>.tables]`; `python -m blastit.fold` and a bare `path` string were the other two | [Log events](docs/reference/log-events.md), [Day-to-day life](docs/frameworks/day-to-day.md#fold-the-log), [The CLI](docs/reference/cli.md), [The manifest](docs/reference/manifest.md) |
| 9 | The compose example in [Docker Compose behind Traefik](docs/infra/compose-traefik.md) mounted the Traefik file at `/etc/traefik/traefik.yml`, which is Traefik's **static** default: Traefik exited. PostgreSQL 18 also refused `pgdata:/var/lib/postgresql/data` | the file is `dynamic.yml`, mounted at `/etc/traefik/dynamic.yml` and named by the provider; the volume is `/var/lib/postgresql`. Both verified by bringing the stack up ([below](#verifying-the-examples)) |
| 10 | `curl` is not in `python:3.13-slim`, so the plain-Docker health check as written could not run | the health check is a `python -c` one-liner, and the section says why |
| 11 | A Python installed under a private home directory also hid the *standard library* from the views (the same finding as E2b-5, from the other side) | [Getting started](docs/getting-started.md#1-install-it) |
| 12 | A `__pycache__` directory owned by **root** appears in the project, because stage 1 runs `python -I` as root and `PYTHONDONTWRITEBYTECODE` does not reach it | [Troubleshooting](docs/troubleshooting.md) |
| 13 | Mermaid diagrams **do** render, but the theme paints each one into a **closed shadow root**, so no DOM query can see the `<svg>`: a checker looking for one sees nothing. Pixels are the only honest check | verified with a screenshot ([below](#verifying-the-examples)); a `rel="preload"` of the Mermaid runtime was added so the diagram appears sooner |
| 14 | "Every page logs a form field without an `id` or `name`": the served DOM has **no** such field. Every `<input>` the server renders carries an `id` (the drawer and search toggles, the three palette radios), and the field Chrome flags is the theme's search input, which its JavaScript builds inside a closed shadow root — invisible to `document.querySelectorAll`. Nothing in `overrides/` or in the content adds a form field, so there is nothing here to fix; a DOM-patching script was written, found to have nothing to patch, and removed again | reported, not patched |

## Verifying the examples

Three things were checked by running them, not by reading them:

* **The compose example, for real.** A Django app image built from the page's own
  Dockerfile (with the wheel from this checkout substituted for the PyPI install,
  which does not exist yet), the page's `compose.yaml`, and the page's
  `dynamic.yml`, started with `docker compose -p e2docs up -d`. Traefik routed
  `Host: shop.example.com` to blastit; `GET /debug/request/` through Traefik
  reported `remote_addr: 172.29.0.1` and `scheme: http`, and **neither changed**
  when the client sent `X-Forwarded-For: 6.6.6.6` and `X-Forwarded-Proto: https`
  — Traefik overwrites both by default, which is what the page claims. The same
  request through the TLS entry point reported `scheme: https`, from Traefik's
  self-signed certificate, which the page now says out loud. The `config` boot
  event said `preset: "compose-traefik"` with exactly the four settings the page
  lists, and `privilege: "capabilities"`. Then `down -v` and the image removed.
* **The README's commands, in a container.** `python:3.13-slim-trixie`,
  `uv init && uv add <the wheel>`, the two-file example, then the run command as
  root: `hello from blastit`, `route: hello`, `uid: 21000`, `status: 200`. One
  caveat, and it is E2b-4: today the command needs `PYTHONPATH=/app` as well,
  which E2b makes unnecessary.
* **Mermaid, in a real browser.** A headless Chrome driven over CDP: every page
  with a fence has `window.mermaid` loaded, no `<pre class="mermaid">` left
  unconsumed, and a screenshot of the region shows the diagram drawn.

## Round 2

Round 2 checked every item in `round1.md` again against the merged site and the
source. Three things came out of it:

* **The README is now 50 lines, from 110** — the banner, the links row, a
  three-sentence pitch, one Get started (10 lines of code: `uv init && uv add
  blastit`, `hello.py`, `blastit.toml`, the run command), the
  "Read the documentation →" line, and the licence. Everything the old README
  carried — the status paragraph, the install variants, the preset discussion,
  the pytest plugin, `fold`, rootless, the development section — is on a page of
  the site, and each of those pages is reachable from the documentation's own
  menu. Nothing was lost: the status block is Home's, the install variants are
  Getting started's, the Development section's two facts are `RELEASING.md` and
  `ROADMAP.md`, both linked from the Changelog page.
* **One E2b sentence had been missed**: the Flask page's run block still exported
  `PYTHONPATH=$PWD`, which E2b-4 makes unnecessary. Fixed, and marked like the
  others.
* **Mermaid is verified on the deployed site**, not just locally: the live
  `how-it-works/` HTML contains a `rel="preload"` of
  `https://unpkg.com/mermaid@12/dist/mermaid.min.js`, the theme's
  `bundle.77dd0ce9.min.js` (which contains the loader that injects it), and the
  three diagram sources as `<pre class="mermaid"><code>…` — and a screenshot of
  the live page shows the flowchart drawn. A DOM query still shows nothing,
  because the theme renders into a **closed shadow root**; that is a property of
  the theme, not of this site, and there is no supported way to change it.

The one round-1 item this PR leaves as it found it is the console warning about a
form field with no `id` or `name`: the field is the theme's search input, built
by the theme's JavaScript inside a closed shadow root, and nothing in
`overrides/` or in the content adds a form field. The evidence is in the pull
request.


