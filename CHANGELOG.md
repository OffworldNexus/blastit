# Changelog

What each release changes for whoever runs an app under blastit. The version
lives in `Cargo.toml` (`[workspace.package] version`), and a release is the tag
`v` plus that version — `scripts/check-release-tag.sh` refuses a tag that says
anything else, and [`RELEASING.md`](RELEASING.md) has the steps.

## 0.1.0 (2026-10-27)

The first release.

blastit serves an ordinary WSGI app — Django, Flask, anything — but splits it
into routes, each with its own process, its own Unix user, its own database
grants and only the privileges it needs. Your framework still routes its own
URLs; the one file blastit adds is `blastit.toml`.

### Serving an app

- **A route is a sandbox.** Each route runs as its own user, in its own
  process: a compromised route cannot read another route's memory, signal it,
  or borrow its rights. Per-role seccomp filters drop the syscalls a role does
  not need, and a route without `egress = true` cannot open an outbound
  connection.
- **The shape is the same for every app.** Routes, methods, groups and grants
  come from `blastit.toml`, and boot validation reports unknown groups and
  proxies, duplicate names, bad regular expressions and unattached groups
  before the server starts.
- **An HTTP/1.1 front end** over an inherited listener: keep-alive, chunked
  bodies, `100-continue`, pipelining in order, header validation, request and
  header timeouts, size limits, and `501` for `Upgrade`. Global and per-route
  budgets are counted in threads; timeouts answer `504`, stuck threads are
  recycled, and the admission queue is fair between clients, so one client's
  burst cannot starve the others.
- **Processes are reused, not restarted per request**: lazy spawn, `prefork`
  for the routes that want to be warm, idle retirement, an LRU cap on live
  views (`BLASTIT_MAX_VIEW_PROCESSES`), and retirement above a memory
  high-watermark. Imports happen before the fork (`BLASTIT_PRELOAD`), and the
  heap is immortalised before each fork — measured at roughly 40 % less memory
  per view for a Django app on CPython 3.14.
- **Logs are one JSON line per event**, bounded at 2048 bytes, with a
  `BLASTIT_LOG_FORMAT=pretty` mode for humans and a single writer for the whole
  process tree. Graceful shutdown drains requests before it exits.

### Secrets, the database and grants

- **Your app never holds the real database URL.** It sees an ordinary
  `DATABASE_URL` pointing at blastit's own proxy; only that proxy — a
  Python-free binary that blastit `execve`s with an empty environment — knows
  the credentials, and it is the only process that can reach the database.
- **Every route gets exactly the statements it was granted.** Grants are
  written in `blastit.toml`, matched after normalisation with parameters
  compared as placeholders, so process IDs, counts and whitespace don't matter.
  Table and column grants are checked with PostgreSQL's own parser, so there is
  no parser differential. `transaction_control` allows `BEGIN`, `COMMIT` and
  friends as whole statements; a row lock needs an `update` grant on a column of
  the locked table, as in PostgreSQL; `unrestricted = true` widens a route's
  *own* grant for routes such as a staff-only back-office (never a group), and
  boot says when it is in use.
- **Environment values are templates**: `{{ field }}` with filters, upstream
  configuration parsed once, inputs stripped from the app's environment, and
  secrets that never reach a template, a log line or an error message.
- **A refusal is one log line** naming the route, the proxy and the statement —
  `sql.denied`, with the reason when table grants are in play.

### The day-to-day loop

- **The pytest plugin** records what your test suite actually does (method,
  path, the route blastit would pick, the SQL on the wire) and diffs it against
  `blastit.toml`: paths that match no route, routes never exercised, statements
  run but not granted, and grants never used. It can print the TOML to paste in
  and, with `--blastit-prefer-tables`, the minimal table grants for a route.
  Reports come as text or JSON.
- **`blastit-manifest fold`** folds `sql.denied` events from production logs
  into the grants a manifest is missing — for the shapes only real traffic
  reaches.
- **A live-server suite can run through blastit itself**, so browser tests
  against the real server are checked against the manifest too.
- **Presets** set a whole group of defaults for a known platform, chosen with
  `--preset NAME` or `BLASTIT_PRESET=NAME`: `digitalocean-paas` (root mode, the
  platform's client-IP header, every peer trusted because the service is only
  reachable through the platform's load balancer and internal services) and
  `compose-traefik` (rootless mode behind Traefik, whose appended
  `X-Forwarded-For` entry is the one believed). Every `BLASTIT_*` variable still overrides what a preset
  set, and boot says which preset ran and which values it chose.
- **Configuration is the environment** — `BLASTIT_APP`, `BLASTIT_MANIFEST`,
  `PORT`, budgets, timeouts, log format — plus an optional preset, so the same
  image works on a laptop and on a platform.
- **Mistakes are refused at boot, by name**: an interpreter, standard library or
  proxy binary the routes cannot read, and any `sys.path` entry (or a directory
  above it) another uid could write, which would let one route plant code
  another imports.
- **Your app just works**: the directory blastit starts in is importable,
  `REMOTE_ADDR` is the client's address and `REMOTE_PORT` its port, and the
  `DATABASE_URL` a route gets is one psycopg, Django and SQLAlchemy all read.

### Deploying it

- **Root, or no root at all.** Started as root, init binds the port and drops
  every other process to its own unprivileged user. Started with file
  capabilities or from an ambient-capable launcher, init runs as an
  unprivileged user holding only `CAP_SETUID`, `CAP_SETGID`, `CAP_CHOWN`,
  `CAP_FOWNER`, `CAP_DAC_OVERRIDE`, `CAP_KILL` and, where the kernel allows it,
  `CAP_SETPCAP`. The uid layout, the secret barrier and the sandboxes are the
  same either way, and boot says which mode it is in.
- **An installable package**: `blastit` on PyPI, with a `blastit` command, a
  `blastit-manifest` command and the pytest plugin, for CPython 3.13 and 3.14 on
  Linux x86_64 (`manylinux_2_28`). `pip install blastit` and `uv add blastit`
  need no Rust toolchain at all — those are wheels — and the sdist publishes the
  same thing for a platform without one: it builds the wheel from source,
  database proxy included, when the package is installed.
- **Development builds**, for something newer than the last release: install
  from `develop` itself (`uv add "blastit @
  git+https://github.com/OffworldNexus/blastit@develop"`), or take the wheel CI
  built for a commit from that run's artifacts.
- The wheel is a compiled extension plus a command written in Python, so the
  server runs on the interpreter of the environment it was installed into: your
  virtualenv and its packages are simply there.

### Documentation

- [`ARCHITECTURE.md`](ARCHITECTURE.md): the design, the security model, the
  measurements and what they assume.
- [`ROADMAP.md`](ROADMAP.md): what is built, step by step, and what is next.
- [The documentation site](https://offworldnexus.github.io/blastit/): getting
  started, fitting blastit to your framework (Django, Flask, any WSGI app),
  fitting it to your infrastructure (the presets, anything else, fair queuing),
  then the reference.
