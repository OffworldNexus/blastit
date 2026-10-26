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
has no such call, so its SQL is recorded with no request to belong to and
nothing is diffed. A Flask project fills its manifest from `blastit-manifest
fold` instead.
