---
status: In review
owner: architect
node: MB.3.3
amended: MB.3.9.2
reviewers: [security, implementability, testability, consistency]
---
# Blastit 0.2.1 Technical Design: FastCGI, Param Lockdown and the Injected Upstream Contract

`docs/frameworks/php.md` is the input of record and is normative: every command, TOML
key, status, header and exit code on that page is a promise this document has to make
true. The design adds **no** manifest key, CLI flag, environment variable, log field or
banner line the page does not show. Where the page is silent the design picks the
fail-closed answer and says so.

It has two halves that ship together: the **FastCGI** half (grammar, session, param
lockdown, pool, attribution, CVE table) and the **backend contract** half — one
object-safe `Upstream` trait in `crates/blastit-backend`, one injected
`Arc<dyn Upstream>` per route built at boot, crate-by-seam layering and one shared
conformance table. Static, WSGI and FastCGI are all implementations of that trait
(owner requirement, 2026-11-14).

> **Amended 2026-11-14 (`MB.3.9.2`, owner decision).** `[route.fastcgi]` has **no
> `listen` key**. `blastit dev` spawns and supervises the pool, so blastit owns the
> socket; a user-authored path could point two routes at one pool, mis-set
> `listen.mode`, or place the socket's parent somewhere a route uid can write, and
> blastit would then have to police all of it. The socket is **derived** per route
> inside blastit's private run directory (`<runtime>/fastcgi/<route_index>/fpm.sock`,
> §4.1 `R3`, §4.8.1), and a manifest that still writes `listen = "…"` is an unknown
> key naming the key (no `0.2.1` release ever carried it, so there is no migration
> class). `workers` is **optional**: absent means the route's per-view cap of the *same*
> budget WSGI already uses (`Config::view_worker_cap()`, the full per-view cap — not a
> divided share), and an explicit value above
> that budget is a boot error naming both numbers (§4.1 `R4`). `ADR-0008` decisions 1, 2 and
> 6 carry the dated amendment (`Amends`). Who spawns the pool in `dev`/`check`, the discovery
> order, cleanup and the error contract are **`MB.3.9.3`**'s amendment (it depends on
> `MB.12`'s runtime-provider contract); this document pins only the grammar, the
> derivation and the socket's provenance.

Companion ADR: [ADR-0008](../decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md).

---

## 1. Developer Experience Target (Local, CI/CD, Deploy)

### 1.1 Every page promise and where this design settles it

| Page promise (`docs/frameworks/php.md`) | Settled by |
|---|---|
| the manifest with `[route.static]` + `[route.fastcgi]` (`entry`, optional `workers`; **no `listen`**) | §4.1 |
| "a file under `public/` is answered by the static responder; any other path reaches the front controller" | §4.9 |
| "No `.php` file under `public/` is served; only `entry` runs" | §4.9 (`Refusal::EntryScript`, `404`) |
| "A FastCGI route is its own executor: no `app`" | §4.2, §4.8 |
| "`[route.ingress]` applies to it; a body without one is a `413`" | §4.5, §4.10.3 |
| Local: no `sudo`, no `/etc`, no hand-written pool file; `blastit dev` owns the pool and derives its socket (page steps: `MB.3.9.1`; spawn/cleanup: `MB.3.9.3`) | §4.1 `R3`, §4.8.1 |
| Local: `workers` omitted → the deployment's per-view budget sizes the pool, and `dev` prints the number it chose | §1.2, §4.1 `R4`, §4.8.1 |
| Local: `blastit dev` banner, `1 route` | §1.2 |
| `curl /users/42` → `200 application/json` from `public/index.php` | §4.5, §4.10 |
| `immutable` asset → `public, max-age=31536000, immutable` | §4.9 (shipped responder, unchanged) |
| `?-d+allow_url_include=1` → `200` with `"allow_url_include":"0"` and the literal `query_string` | §4.6, §5.2 `P5` |
| "a hostile FastCGI param is refused, not forwarded; `SCRIPT_FILENAME` is pinned to `entry`" | §4.6, §4.7 |
| `dev --learn` records the front controller's `404` and writes `[route.static] root = "public"` | §4.10.2 |
| `check -- vendor/bin/phpunit` → `24 requests, 0 statements, 1/1 routes, clean` | §4.10.3 |
| `BLASTIT_URL=http://127.0.0.1:8080` for the suite | §4.10.3 |
| a route nothing requested, or a body route with no input rule, is a finding (`1`) | §4.10.3 |
| `check --json` document with `unexercised_route` / `fix.action: add_test` | §4.10.3 |
| Deployments: one pool per route, as that route's uid, derived `0600` socket, parent `0511` init-owned | §4.8.2 |
| `blastit --preset compose-traefik`; production still exits `1` unprivileged | unchanged (`ADR-0005`), §4.8.2 |
| "`0.2.1` adds `[route.fastcgi]`, the PHP `--learn` write and `check -- <suite>`" | §4.1, §4.10 |

### 1.2 The banner is unchanged

The page's banner is exactly three lines, the third carrying `1 route`
(`crates/blastit/src/dev_learn.rs::banner`). A FastCGI route is a route and is counted;
no pool line and no "waiting for php-fpm" line. `dev_learn::banner` is untouched, so the
protected exact-three-line assertion in `critical_fastcgi_learn.rs` still passes.

Boot output gains exactly two things, neither of them a banner line:

1. **`dev` prints one line per FastCGI route naming the resolved pool size**, on the
   existing dev-diagnostic channel (`eprintln!("blastit dev: …")`, exactly the channel
   `dev_fail` and the `BLASTIT_DEV_EXPOSE` guard already use):
   `blastit dev: fastcgi route "app": pool workers 16 (derived from BLASTIT_VIEW_MAX_WORKERS=16)`
   — where the named variable is **the one that actually supplied the cap**, so an
   unset `BLASTIT_VIEW_MAX_WORKERS` prints `(derived from BLASTIT_MAX_WORKERS=16)` instead of
   a variable nobody set — or `… pool workers 8 (workers = 8, budget 16)` when the key is
   written. **stderr**, not stdout; **no new structured event, no new JSON key for the
   resolved worker count, no new event kind**, and the banner's stdout stays the same three
   lines. `dev` is where the developer has no other way to see the
   number, so it is where the line goes; **production prints nothing extra**, because its
   durable record is the generated pool file's `pm.max_children=<resolved>` (§4.8.2) and
   the over-budget refusal names both numbers (§4.1 `R4`).
2. **one fatal diagnostic** in the existing `./blastit.toml:<line>: …` shape (§4.2).

The derived socket path appears **only in a boot diagnostic on stderr** (`exit 1`/`2`) — never
in success output, `--json`, `http_obs.jsonl` or a `--learn` proposal; it is inside blastit's
private run directory and the developer cannot act on it (§4.8.1, R3).

### 1.3 CI/CD

`blastit check` for a manifest with no `app` (and `BLASTIT_APP` unset) does **not**
import Python or require pytest when `Manifest::can_boot_without_app()` is true. With
`check -- vendor/bin/phpunit` (matching the page's `check -- <suite>` spelling; `cli.rs`
stores the trailing argv slice as `suite_cmd`) it runs the suite command after `--` with
`BLASTIT_URL=http://127.0.0.1:8080`, cwd = the manifest directory, `env_clear` + the
prepared app environment, and the child's dbproxy sockets; the suite speaks HTTP and the
front controller runs.

Before branching to a runner, `check::execute` validates FastCGI routes alongside static
roots — `for i in fastcgi_routes { manifest.check_fastcgi_entries(&config.manifest_dir, &DevProbe { uid: euid }, i, ProbeUid::Dev) }`
and `manifest.check_fastcgi_workers(config.view_worker_cap())` (the **pool leaf's** function, gated
on `MB.12`) — so a missing `entry` or an
explicit `workers` above the budget fails immediately with exit `2` naming
`./blastit.toml:<line>`. The **socket** probe and the `BLASTIT_CHECK_POOL_PARENT_FD`
handoff keep their shape but follow the derived path (`R3`); their ordering (the leaf exists
only after the dev pool starts) is `MB.3.9.3`'s amendment.

`check::serve_and_run` (`crates/blastit/src/check.rs`, `MB.3.6.4.2`) owns the observation
and runtime-root handoff: it creates `<CheckRoot>/dev` (`0700`, owned by `euid()`),
`<CheckRoot>/http_obs.jsonl` (`0600`) and a `socketpair` `(obs_read, obs_write)`
(`FD_CLOEXEC` cleared on `obs_write` right before spawning the child `dev`, then `obs_write`
dropped immediately in the parent), prepares `<suite>`'s environment (`prepare_env`)
against the deterministic per-route socket directory `<CheckRoot>/dev/views/0`, and
spawns `Host::stage1()`'s `Reexec` — `program` then `prefix`, i.e. `/proc/self/exe` for a
standalone binary and `<python> -I -m blastit` for a wheel (`host.rs:150-220`;
`python/blastit/__main__.py` is the `-m blastit` entry point) — followed by
`dev --port 8080` (never passing `--learn` to the child, so
`slot_offset == 0` and child `dev` never edits `blastit.toml`) with
`BLASTIT_CHECK_DEV_ROOT=<CheckRoot>/dev` (`check::DEV_ROOT`),
`BLASTIT_CHECK_HTTP_OBS=<CheckRoot>/http_obs.jsonl` (`check::HTTP_OBS`),
`BLASTIT_CHECK_SQL_OBS_FD=<obs_write_fd>` (`check::SQL_OBS_FD`) and
`BLASTIT_CHECK_POOL_PARENT_FD=<route_index>:<fd>` (`check::POOL_PARENT_FD`, repeatable, one
per FastCGI route; §4.8.1). `Config::for_dev()`
(`MB.3.6.3.1`) reads all four `BLASTIT_CHECK_*` variables in `stage0::run_dev` before
`BLASTIT_*` environment stripping: `stage0::run_dev` immediately restores `FD_CLOEXEC` on
`check_sql_obs_fd` (`fcntl(F_SETFD, FD_CLOEXEC)`), adopts `<CheckRoot>/dev` via
`Dev::adopt` (`verify_private_dir`) instead of minting a random nonce root, binds every
declared `postgresql` proxy in `<CheckRoot>/dev/views/<i>/.s.PGSQL.<port>` when
`options.learn || config.check_sql_obs_fd.is_some()` (so `prepared` and the child's
`views/0` listener agree even under `check --learn -- <suite>`), passes `check_sql_obs_fd`
only in `fork_dbproxy_dev`'s `ChildIo.keep` (`ListenerMode::Learn`), and **closes
`check_sql_obs_fd` in stage 0 immediately after `fork_dbproxy_dev` returns** — before
`exec_stage1_dev`, so neither stage 1 nor any `http`, `scheduler`, `view` or `php-fpm`
process ever holds `obs_write_fd` and no route can forge `WireSqlObs` lines — while passing
`check_http_obs` through `Config` → `stage1` → `SchedulerConfig.http_obs`. `serve_and_run`
redirects the child `dev`'s stdout (and `<suite>`'s stdout) onto stderr when `--json` is
set, polls `127.0.0.1:8080` for readiness (an empty TCP connect/close writes no
`http_obs.jsonl` line; an early child exit or busy port exits `2`), runs `<suite>`,
terminates the child (`SIGTERM`, 2 s grace, `SIGKILL`), joins `ObsReader` on `obs_read`
for `WireSqlObs`, and reads `http_obs.jsonl` into `CheckObsV1::new_with_upstream`. The
parent starts **no** dbproxy of its own, so there is one app environment and one proxy.

`check` branches in one total order:

1. `manifest.is_all_static()`: 0.2.0's exit `0`, empty stdout (`--json` prints the zero
   report; `--learn` exits `2` with `nothing to learn: every route is static`);
2. `config.app.is_none() && manifest.can_boot_without_app()` (not all-static, so ≥ 1
   FastCGI route): external-executor mode — with `-- <suite>` runs `serve_and_run`;
   with no command after `--` exits `2` with
   `route "<first_fastcgi_route>" runs an external executor: blastit check -- <suite>`;
3. everything else (`config.app.is_some()`, or `config.app.is_none() &&
   !manifest.can_boot_without_app()`): the existing pytest path (`--` arguments forwarded
   verbatim to `pytest`, preserving `docs/ci.md`'s `blastit check -- -k products` and
   `critical_static_cli`'s app-less `MIXED` `"no interpreter to run pytest on"` exit `2`).

Findings and exit codes are unchanged (`1` for an error finding, `3` for non-zero suite
exit); the `--json` document is unchanged (§4.10.3).

### 1.4 Deployments

The production `blastit` starts one `php-fpm` master per FastCGI route as that route's
uid, under the existing **view** role — no new role, uid, supervisor or socket family
(§4.8.2). Init creates the derived per-route pool directory `<runtime>/fastcgi/<route>`
(`0700`, init-owned) from its own runtime dirfd at boot, **before any route process
exists**, pins it, makes each bound pool socket `0600` owned by the scheduler, and
re-freezes the parent `init_uid:init_gid 0511` before publishing `Ctl::PoolReady`; no
manifest value contributes a byte of that path, so no other route's process can reach or
pivot it. `blastit --preset …` prints the same preset as today and refuses to run
unprivileged (`exit 1`), unchanged.

---

## 2. Goals & Non-Goals

**Goals.** `[route.fastcgi]` with the page's keys — `entry` and an optional `workers`;
a FastCGI session that cannot
send a param the harness did not compute; one language-agnostic backend contract with
**all three** implementations injected per route; crate-by-seam layering; one
conformance table; honest `dev`/`check`/`--learn` attribution for a route with no
Python; a pool that runs as the route's uid without `sudo`, with a socket blastit itself
derives, and without weakening the role model beyond one recorded seccomp addition.

**Non-Goals.** No `php-cgi`, no CGI `argv` path, no query rewriting (`CVE-2012-1823` is
impossible by construction). No connection reuse or multiplexing (§4.5.2). No top-level
`[fastcgi.<name>]` block, no `env` key, no per-route `php.ini`, no `fastcgi_param`-style
user map (§6). **No user-authored socket path, socket name or socket mode:** `listen` is
not a key, and no manifest value reaches the pool socket's path, parent, owner or mode.
No new role, no root php-fpm, no second static responder, no change to
the static responder's behaviour on a route without `[route.fastcgi]`.

---

## 3. Requirements

- **R1 Grammar.** `[route.fastcgi]` has exactly `entry` (required string) and `workers`
  (optional integer). `listen` is **not** a key: a manifest that writes it gets
  `ManifestError::FastCgiListenDerived` (`route "app" (index 0): fastcgi listen: unknown
  key; the pool socket is created by blastit`), which carries no value, and no manifest
  value can influence the socket (R3). Unknown key fatal. `fastcgi` is a reserved route key.
  `[route.static]` keeps its 0.2.0 keys.
- **R2 Entry.** `entry` is relative to `blastit.toml`, ends in `.php` (ASCII
  case-insensitive), has no `..`, `\`, control byte, is not absolute, resolves to a
  regular file **beneath** `root` (when both blocks are declared) or beneath its parent
  (otherwise) with `openat2(RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS)` and no symlink in the
  final component; readable by the route uid (production) or invoking uid
  (`dev`/`check`).
- **R3 Derived socket.** The pool socket is a **pure function of the route's index and
  blastit's run directory**; no manifest value is an input:
  - production: `<runtime>/fastcgi/<route_index>/fpm.sock` under `/run/blastit`, created by
    init (§4.8.3 `B1`/`B2`);
  - `dev`/`check`: `<run_root>/fastcgi/<route_index>/fpm.sock`, where `<run_root>` is the
    run root **blastit itself created for this invocation** — `Dev::adopt`'s `0700`
    caller-owned root (`dev_learn.rs:180`), or `<CheckRoot>/dev` for `check`. Which root
    `MB.3.9.3`'s provider hands the `dev` pool (it may move it to a project-local
    `.blastit/run`) is that node's call; the suffix `fastcgi/<route_index>/fpm.sock` and
    the "created by blastit, `0700`, caller-owned" predicate are this document's;
  - `<route_index>` is the index of the `Route` in `Manifest::routes` (routing order) —
    the same integer `--fastcgi-parent-fd=<route>:<n>`, `PinnedSocket` and
    `SchedulerConfig.pinned_sockets` already carry — **never** the FastCGI-only ordinal
    (a manifest of `fastcgi, wsgi, fastcgi` pins `0` and `2`). So no route *name* is ever
    interpolated into a path and there is no name-escaping rule.
  The path is never **derived from** `blastit.toml` and no key, environment variable or CLI
  flag selects it. It may appear **only in a boot diagnostic** (stderr, `exit 1`/`2`) and
  never in success output, `--json`, `http_obs.jsonl` or a `--learn` proposal (§1.2).
  Modes, one per level (§4.8.3): `<runtime>` keeps the shipped root-owned `0755`
  (`php-fpm`'s absolute path walk needs `x`), `<runtime>/fastcgi` is
  `init_uid:init_gid 0711` (traverse-only, init the only writer), and
  `<runtime>/fastcgi/<i>` is created `0700 init`, `0755 route_uid` only in the bind window,
  `0511 init` steady. `dev`/`check` never enter that window: both stay
  `0700 invoking_uid`. The socket is `0600` owned by the scheduler (production) or the
  invoking uid (`dev`/`check`). `listen.mode` is therefore not a manifest concept:
  blastit's `0600` plus the §4.8.3 owner discipline *is* the mode.
  The `dev`/`check` probe is **two-phased**: phase 1 (`SocketProbe::parent`) creates and pins
  the parent **before** the pool starts; phase 2 (`SocketProbe::check_leaf`) runs **after**
  it and requires the leaf to exist, be a socket, not be a symlink, be `0600`, and be
  connectable by the invoking uid (`R20`). Who starts the pool, and in what order, is
  `MB.3.9.3`'s amendment.
- **R4 Workers.** The **key** — optional, and an explicit value in `1..=64` —
  is this node's. The **resolution** and the pool **ceiling** below belong to the **pool
  mechanism**: they are implemented by the pool leaf and are **gated on `MB.12`**'s
  embed-vs-pool decision (an embedded SAPI has no `pm.max_children` for `workers` to size,
  in which case `MB.12` retires the resolution and keeps only the grammar). For the pool
  mechanism: both an explicit `workers` and the resolved size are bounded by `1..=64`
  (`pub const MAX_POOL_WORKERS: u32 = 64` — pool leaf, gated on `MB.12`). The ceiling is not
  cosmetic — `pm = static` **pre-forks** `pm.max_children` children at bind time, so an
  omission under `BLASTIT_MAX_WORKERS=512` would spawn 513 `php-fpm` processes per route and
  size `RLIMIT_NPROC`/`RLIMIT_NOFILE` from the same number. `Manifest::check_fastcgi_workers`
  therefore refuses `policy.resolved_workers(cap) > MAX_POOL_WORKERS` with
  `FastCgiWorkersCapExceeded` (naming the resolved value, the ceiling, and the budget
  variable), and refuses an explicit `workers` outside `1..=64` with
  `FastCgiWorkersInvalid`. The **resolved** pool size is
  `policy.workers.unwrap_or(cap)` — one pure formula owned by the **pool leaf, gated on `MB.12`**
  — where `cap` is
  `Config::view_worker_cap()` (`crates/blastit/src/config.rs:1407-1411`) =
  `BLASTIT_VIEW_MAX_WORKERS`, defaulting to `BLASTIT_MAX_WORKERS`
  (`config.rs:674,677`; defaults `16`/`None` at `config.rs:841-842`). The resolved value
  is written to `pm.max_children`, is `RouteState.worker_cap` (that route's admission
  cap, exactly as `view_worker_cap` is a view's), and sizes `RLIMIT_NPROC` (§4.8.2).
  **An explicit `workers` above `cap` is a boot error naming both numbers** — never a
  clamp (§4.1's key table, `ADR-0008` *Amends*), because a clamp would silently deliver a
  concurrency the manifest does not declare and make `pm.max_children`, `RLIMIT_NPROC` and
  `RouteState.worker_cap` disagree. Two FastCGI routes do **not** divide the budget:
  each resolves to the **per-view cap** (`view_worker_cap()`, not a divided share), so their
  `pm.max_children` sum is `2 × cap`, the same arithmetic as two WSGI views; the scheduler's
  global `slots < max_workers` gate plus each route's own permit stays the admission truth
  for every backend (§4.2.1 `Capacity`).
- **R5 Composition.** `entry` is under `static.root` when both blocks are declared.
  `spa = true` with `[route.fastcgi]` is fatal. `prefork = true` starts the pool at boot.
  `[route.ingress]`, `prefork`, grants and inline `groups` are legal on a FastCGI route
  (it is an executor). The 0.2.0 dead-declaration rule keeps both arms: a static route
  with no fastcgi that can never reach an app (`spa = true` or no `app`) may not declare
  them.
- **R6 Methods.** The existing token grammar; `CONNECT` and `TRACE` on a FastCGI route
  are boot-fatal (PHP never observes them as a request).
- **R7 Param set.** The complete set is §4.6.1. `SCRIPT_FILENAME`/`DOCUMENT_ROOT` are
  computed from declared values, never request bytes; `PHP_VALUE`, `PHP_ADMIN_VALUE`,
  `auto_prepend_file`, `allow_url_include`, `open_basedir`, `disable_functions`,
  `REDIRECT_STATUS` and `HTTP_PROXY` are never emitted.
- **R8 Refusal matrix.** Each row of §4.7 is binary: one input, one owner, one
  client-visible status or the exact param stream.
- **R9 Wire.** One connection and one `BEGIN_REQUEST` per request, `requestId = 1`,
  `keepConn = 0`. One `PARAMS` record (encoded ≤ 65 535) plus its zero-length
  terminator; one zero-length `STDIN` terminator. `STDOUT` is a CGI head (`Status:` or
  default `200`) then body; response `Content-Length` is stripped; `END_REQUEST.appStatus`
  and `reserved[3]` are ignored; a stream without `END_REQUEST` is a `502` before the
  head and a `RespAbort` after it.
- **R10 Cancellation.** Client `CANCEL`, timeout or drain sets the cancel token, sends
  `ABORT_REQUEST` best-effort, closes the connection, and releases the slot only when
  the session task reports `Done` (a parked file read keeps its slot, exactly 0.2.0).
- **R11 Pool.** No new role: one `php-fpm -F -y` master per route as the view role's
  uid/gid, a private runtime dir, a generated pool file whose `listen=` line is the
  derived path of `R3` (§4.8.2), restart with backoff, `pm = static`,
  `pm.max_children = <resolved workers>` (`R4`),
  `pm.max_requests = max(BLASTIT_VIEW_MAX_REQUESTS, 500)` (a pool always recycles; the
  scheduler's view counter keeps its own `0`-means-off semantics), `clear_env = yes`,
  `security.limit_extensions = .php`, `catch_workers_output = yes`, the
  `php_admin_value` backstops, `request_terminate_timeout`.
- **R12 Static coordination.** The shipped responder answers first; `.php`/`.phar` under
  the configured root are `404` (`Refusal::EntryScript`) and never forwarded on a
  FastCGI route; only `entry` executes; the entry is never served as a file.
- **R13 Uniform contract.** All backends implement `Upstream`; dispatch never `match`es
  on backend kind; the scheduler holds one non-optional stage chain per route, built once
  at boot; a missing builder is a boot error naming the manifest line; no fallback
  backend.
- **R14 Layers.** `blastit-backend` depends only on `blastit-http` (`BodyFraming`),
  `blastit-manifest` types and `serde` (`#![forbid(unsafe_code)]`, zero I/O in its own
  source `crates/blastit-backend/src` — `OwnedFd` inside `PinnedSocket` is an inert `std`
  handle). Implementations depend inward. `blastit-scheduler` depends on `blastit-fastcgi`'s
  **pure** modules and never on a socket type from it; `blastit-fastcgi` has
  `#![forbid(unsafe_code)]`, no socket, and zero production dependencies.
- **R15 Conformance.** One table in `blastit-backend/src/conformance.rs`: `cases()` with
  capability-filtered cases; each driver iterates it; classes are `refusal`, `body_cap`,
  `observation`, `error_mapping`; a driver reports and counts `Skipped`; scheduler tests
  use a fake `Upstream`.
- **R16 Ports.** Static keeps `400`/`404`, `no-store`, `immutable`/`no-cache`,
  `ETag`/`304`/`HEAD`, index/SPA precedence and forward-on-miss. WSGI keeps status,
  hop-by-hop stripping, body credit and `app.output`. The protected suites
  `critical_static_assets.rs`, `critical_static_resolve.rs`, `critical_rewrite.rs`,
  `critical_ingress_enforcement.rs` pass **unmodified**.
- **R17 Attribution.** `--learn`/`check` consume one observation stream. Exercise is
  per executor: a route with an executor is exercised by a `wire_request` line; an
  all-static route by a `static_file` line. No per-language branch.
- **R18 Observability.** No query string, cookie, `Authorization`, body or param value is
  ever written by the harness; the one recorded header field is the `Content-Type`
  essence (`WireRequest.content_type`). `app.output` is app-controlled data (capped)
  emitted through the scheduler's events sink by init; `Observation::Param` carries a
  name and a decision only; `WireRequest.path` is the path without the query.
- **R19 Budgets.** Params: ≤ 128, encoded ≤ 65 535, one pair ≤ 65 531; observations:
  ≤ 64 total with a `dropped` counter; pool stderr: ≤ 60 lines/request, ≤ 2048
  bytes/line (and ≤ 2048-byte partial-line accumulator); CGI head: ≤ 64 KiB, ≤ 100
  headers, ≤ 8 KiB/value; request body buffer: `BLASTIT_MAX_BODY_BYTES` per admitted
  FastCGI request, bounded across the process by `BLASTIT_MAX_WORKERS ×
  BLASTIT_MAX_BODY_BYTES` under the existing half-body queue cap; response credit and
  `BLASTIT_MAX_WORKERS` unchanged.
- **R20 Fail closed.** The **derived** pool leaf (`R3`) missing, not a socket, a symlink,
  over-permissive or owned by another uid is a **boot error in `dev`** (`exit 1`, phase 2 of
  §4.8.1 runs in the same process that spawned the pool) and an **early child exit → `exit 2`**
  in `check` (`check` observes its child `dev` exiting, exactly like a busy port); a derived
  parent that is not the private directory blastit created is a boot error in every mode; an explicit
  `workers` above the budget is a boot error (`R4`); a pool that died after boot
  (`Unavailable`,
  including `connect` `ENOENT`/`ECONNREFUSED` before any head) is `503` with backoff; a
  mid-exchange upstream failure (`Fault`) before any head is `502`; the first request to
  a lazy pool waits for bind (bounded 5 s), then `503` + `Retry-After: 1` — never a
  spurious `502`. Never a fallback backend.

---

## 4. Architecture & Data Flow

### 4.1 Manifest grammar

```toml
[[route]]
name = "app"
path = ['.*']

[route.static]
root = "public"
immutable = ["/assets/*"]

[route.fastcgi]
entry = "public/index.php"
```

`workers` is optional and usually omitted; the pool is sized from the deployment's
per-view budget (§4.8.1). There is no `listen`: the socket is derived and blastit owns
it (`R3`), so a manifest cannot point two routes at one pool, choose a socket mode, or
place a world-writable parent:

```toml
[route.fastcgi]
entry = "public/index.php"
workers = 8   # optional; must be <= BLASTIT_VIEW_MAX_WORKERS (default BLASTIT_MAX_WORKERS)
```

| Key | Type | Default | Meaning | Fatal at boot |
|---|---|---|---|---|
| `entry` | string | required | front controller, relative to `blastit.toml` | missing, empty, absolute, `..`, `\`, control byte, not `.php`, not a regular file, symlink final component, outside `root`, unreadable by the route uid |
| `workers` | integer | **derived** = `Config::view_worker_cap()` (`BLASTIT_VIEW_MAX_WORKERS`, else `BLASTIT_MAX_WORKERS`, default `16`) | the route's per-view cap (**not** a divided share): `pm.max_children`, `RouteState.worker_cap`, `RLIMIT_NPROC` base | explicit `0` or `> 64` (`FastCgiWorkersInvalid`); **`resolved_workers(cap) > 64`**, which an omission can hit under a large budget (`FastCgiWorkersCapExceeded`, naming the value, the ceiling and the variable); **an explicit value above `view_worker_cap()`** (`FastCgiWorkersOverBudget`, naming both numbers); a non-integer/negative/`> u32::MAX` is the parser's existing `WrongType`. *(The derivation, the ceiling and the two budget variants are the **pool leaf**'s, gated on `MB.12`; the key and its `1..=64` grammar bound are this node's.)* |
| `listen` | — | — | **not a key.** The socket is derived per route (`R3`); it is never printed | writing it is `ManifestError::FastCgiListenDerived`, which carries **no** `listen` value: `route "app" (index 0): fastcgi listen: unknown key; the pool socket is created by blastit` |

`workers` is the one value a deployment may legitimately need to lower: PHP worker RSS
is tens of megabytes, so a route whose front controller is memory-heavy may need four
workers where the deployment's budget is sixteen, without shrinking every WSGI view's
concurrency (the only other knob, `BLASTIT_VIEW_MAX_WORKERS`, is deployment-global). It
is documented as **usually omit**, blastit logs the resolved value once at boot (§1.2),
and it can only ever *lower* the route below the budget it inherits — never raise the
deployment's admission ceiling. Delete it and the operator's only tool would be a global
knob that taxes every route.

Per-route, not a named top-level block: the page writes `[route.fastcgi]`, like
`[route.static]` and `[route.ingress]`; `app` is manifest-global and a FastCGI route has
none, so a named block adds a second naming rule for no configuration win (`ADR-0008`
decision 2). Error variants carry the route and the manifest line:

| Variant | Example message |
|---|---|
| `FastCgiEntryInvalid { location, entry, rule: String }` | `route "app" (index 0): fastcgi entry "index.php": must be under static root "public"` |
| `FastCgiWorkersInvalid { location, workers, why }` | `route "app" (index 0): fastcgi workers 0: must be at least 1` |
| `FastCgiWorkersCapExceeded { location, workers, limit }` *(pool leaf, gated on `MB.12`)* | `route "app" (index 0): fastcgi pool size 512 exceeds the FastCGI pool ceiling 64 (pm = static pre-forks every worker): set workers, or lower BLASTIT_VIEW_MAX_WORKERS / BLASTIT_MAX_WORKERS` |
| `FastCgiWorkersOverBudget { location, workers, budget }` *(pool leaf, gated on `MB.12`)* | `route "app" (index 0): fastcgi workers 32 exceeds this deployment's per-view worker budget 16 (BLASTIT_VIEW_MAX_WORKERS / BLASTIT_MAX_WORKERS); lower workers or raise the budget` |
| `FastCgiPoolSocketInvalid { location, path, rule }` *(implemented by `MB.3.5.2`)* | `route "app" (index 0): pool socket "<run>/fastcgi/0/fpm.sock": is not a socket` (the path is the `dev`/`check` run root's; production uses the stage-0 boot diagnostic instead, `stage0::fail_pool_parent`) |
| `FastCgiDeadDeclaration { location, key, why }` | `route "app" (index 0): spa = true never reaches the front controller` |
| `FastCgiListenDerived { location }` | `route "app" (index 0): fastcgi listen: unknown key; the pool socket is created by blastit` |
| *(removed)* `FastCgiListenInvalid`, `FastCgiListenParentInvalid` | **deleted.** Every rule they carried was a rule about a manifest-authored socket path (`must be a relative path`, `must not contain ..`, `must have a dedicated parent directory`, `is writable by the route uid / contains a symlink`, `is shared with another route`). With no path in the manifest there is no such input to reject, so no surviving rule needs either variant. The one `listen` refusal is `FastCgiListenDerived`: it carries **no** value (so it cannot smuggle a path), names the rejected key, and says who owns the socket. The one-line hint is kept because the key appears in the `0.2.1` drafts of the page and of this design, so the message saves a support question; `0.2.1` never shipped the key, so no migration path or alias is added. |

`rule`/`why` are **exactly** the phrases in the two `rule` columns below (the `entry` mapping
table and the derived-parent mapping table) — no other phrase exists, and neither table has a
phrase the other one shares. `FastCgiListenDerived` is produced by the strict parser (before any key is typed), so it
cannot carry a user value. `Manifest::check_fastcgi_workers` (the **pool leaf's**, gated on `MB.12`) evaluates its three arms in one
fixed order — explicit range (`FastCgiWorkersInvalid`), explicit budget
(`FastCgiWorkersOverBudget`), derived ceiling (`FastCgiWorkersCapExceeded`) — so
`workers = 65` under a 16 budget is always the range error, never the ceiling one; the
range arm duplicates `validate` deliberately (defence in depth: the boot check is total on
its own). `FastCgiWorkersOverBudget` is **not** a `Manifest::validate` error: `validate`
has no `Config` and must keep parsing a manifest on a machine with no deployment
(`docs_fences.rs`). It is a separate pure check,
`Manifest::check_fastcgi_workers(&self, budget: u32) -> Result<(), ManifestErrors>`, called
once at boot with `config.view_worker_cap()` by `stage0` (production **and** `dev`) and by
`check::execute`; the function interpolates both numbers, so the message never needs the
budget's source. `FastCgiPoolSocketInvalid` (`MB.3.5.2`'s) names the **derived** path, never a manifest
value, and is emitted by the `dev`/`check` probe path only (production reports a derived
parent that is not blastit's private directory as a stage-0 boot diagnostic with the same
`./blastit.toml:<line>: route "app" (index 0): …` shape, not as a `ManifestErrors`
variant, because it is an environment fault rather than manifest syntax).
`FastCgiDeadDeclaration` fires **only** when `fastcgi.is_some()` and
`static_policy.is_some_and(|p| p.spa)`; the 0.2.0 `StaticDeadDeclaration` keeps its exact
existing `why` phrases (`spa = true never reaches the app` / `the manifest declares no
app`) for `static_policy.is_some() && fastcgi.is_none() && (spa || app.is_none())`, so no
protected string in `critical_static_manifest.rs` changes. Existence, readability and the
derived parent are probed by explicit calls, never by `validate` (so `docs_fences.rs`
needs no socket and no run directory):

```rust
pub trait SocketProbe {
    /// Creates (when absent) and pins the **derived** pool parent of `R3`:
    /// `<run_root>/fastcgi/<route_index>`, two `mkdirat(0o700)` steps from `run_root`,
    /// opened `O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC` after `openat2(RESOLVE_BENEATH|
    /// RESOLVE_NO_SYMLINKS)`, required `0700` and owned by the invoking uid. `DevProbe`
    /// returns the opened `parentfd` (the caller owns it) and **must** run before the pool
    /// starts — `php-fpm` cannot bind into a directory that does not exist;
    /// `ProductionProbe` returns `None` (stage 0 runs `B1`/`B2` on its runtime dirfd).
    fn parent(&self, run_root: &Path, route_index: usize)
        -> Result<Option<OwnedFd>, SocketProbeError>;
    /// The derived leaf inside a pinned parent: `fstatat(parent, "fpm.sock",
    /// AT_SYMLINK_NOFOLLOW)` is `S_ISSOCK`, `st_uid == uid`, `(st_mode & 0o777) == 0o600`,
    /// and connectable as `uid`. The caller runs this **after** the pool has started
    /// (who starts it, and in what order, is `MB.3.9.3`).
    fn check_leaf(&self, parent: BorrowedFd<'_>, uid: u32) -> Result<(), SocketProbeError>;
    /// R2: `entry` resolved with openat2(RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS) from a
    /// `manifest_dirfd` opened `O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC`, final component
    /// not a symlink, regular file, readable as `uid`.
    fn check_entry(&self, manifest_dir: &Path, entry: &str, root: Option<&str>, uid: ProbeUid)
        -> Result<(), SocketProbeError>;
    /// `true` only for `DevProbe`; `check_fastcgi_sockets` refuses a `Some(fd)` from a
    /// probe that returns `false` (a `ManifestErrors` programmer error).
    fn provides_parent_fd(&self) -> bool { false }
}
/// The pure parent-side half of the probe child's reply: one byte → the variant it reports
/// (`O` is success and returns `None`; any byte outside the pinned set is `Io` with the
/// distinct message `probe child exited without a reply`). Living in `blastit-manifest`
/// (no `libc`) is what lets `critical_fastcgi_manifest` test the mapping.
pub fn from_reply_byte(byte: u8, uid: u32) -> Option<SocketProbeError>;
#[derive(Debug)]
pub enum SocketProbeError {
    Missing,
    NotSocket,
    NotRegularFile,
    NotDirectory,
    InsecureMode { mode: u32 },
    NotConnectable { uid: u32 },
    Unreadable { uid: u32 },
    OutsideRoot,
    Timeout,
    ForeignOwner { uid: u32 },
    Symlink,
    Io(std::io::Error),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeUid {
    /// The invoking uid (`dev`/`check`).
    Dev,
    /// Route `i`'s uid (production).
    Route(u32),
}
pub struct DevProbe { pub uid: u32 }
// `parent`: resolves <run_root> as the private dir the caller owns (0700, no symlink,
// fstat/lstat inode equality), mkdirat "fastcgi"/"<route_index>" 0700 when absent, then
// openat2 O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC (per-segment O_NOFOLLOW fallback on
// ENOSYS), verifies owner/mode, and returns Some(parentfd).
// `check_leaf`: fstatat(parentfd, "fpm.sock", AT_SYMLINK_NOFOLLOW) is S_ISSOCK,
// invoking-uid-owned, 0600, and connectable as `uid`.
// `ParentWritableOrSymlink` is gone with the manifest path it described: a foreign-owned
// derived parent is `ForeignOwner { uid }`, a group/world-writable or over-permissive one is
// `InsecureMode { mode }`, a planted symlink is `Symlink`, and a regular file where the
// directory belongs is `NotDirectory`.
pub struct ProductionProbe { pub init_uid: u32, pub scheduler_uid: u32, pub layout: UidLayout, pub routes: usize, pub seccomp: bool }
// `parent`/`check_leaf` return Ok(()) / Ok(None); B1/B2 run in `stage0` (`MB.3.6.3.1`)
// on the runtime dirfd, and the leaf is verified by `C1` on every connect.
impl Manifest {
    /// `ProbeUid::Dev` = the invoking uid; `ProbeUid::Route(i)` = route `i`'s uid.
    /// Validates **one** route's `entry`; call once per FastCGI route with that route's uid.
    pub fn check_fastcgi_entries(&self, manifest_dir: &Path, probe: &dyn SocketProbe,
                                  route_index: usize, uid: ProbeUid) -> Result<(), ManifestErrors>;
    /// Implemented by the **pool leaf, gated on `MB.12`** (the manifest crate gains it then).
    /// `R4`, three arms in this order: an explicit `workers` outside `1..=64` is
    /// `FastCgiWorkersInvalid`; an explicit value above `budget` is
    /// `FastCgiWorkersOverBudget`; and `resolved_workers(budget) > MAX_POOL_WORKERS` is
    /// `FastCgiWorkersCapExceeded` — so **omitting** `workers` is not automatically fine (it is
    /// refused when the deployment's own budget is above the pool ceiling).
    /// Pure: no probe, no I/O, no `Config` — the caller passes `view_worker_cap()`.
    pub fn check_fastcgi_workers(&self, budget: u32) -> Result<(), ManifestErrors>;
    /// Creates and pins every route's **derived** pool parent (`R3`) and returns it per
    /// route (`Vec::new()` for `ProductionProbe`, which returns `None`); the leaf check is
    /// `check_fastcgi_leaf`, run after the pool starts.
    pub fn check_fastcgi_sockets(&self, run_root: &Path, probe: &dyn SocketProbe)
        -> Result<Vec<(usize, OwnedFd)>, ManifestErrors>;
    /// Phase 2: `probe.check_leaf(parent, euid)` for one route, mapped to
    /// `FastCgiPoolSocketInvalid { location, path: <run_root>/fastcgi/<i>/fpm.sock, rule }`
    /// — the **one** mapper, so the derived path in the message comes from one place.
    pub fn check_fastcgi_leaf(&self, run_root: &Path, route_index: usize,
        parent: BorrowedFd<'_>, probe: &dyn SocketProbe) -> Result<(), ManifestErrors>;
    pub fn route_lines(&self) -> Vec<u32>;
}
```

**The production readability mechanism: a short-lived probe child.** `setfsuid`/`setfsgid`
cannot work here: `rustix 1.1.5` has neither, `init` holds `CAP_DAC_OVERRIDE`
(`blastit-sys/src/caps.rs`), which bypasses DAC independently of fsuid, and `setfsuid`
returns the previous id with no error, so "failed restore" is unwritable. `ProductionProbe::
check_entry` therefore forks a **probe child** from single-threaded stage 0 over a
`pipe2(O_CLOEXEC)`:

1. **Fork:** `crate::forking::fork(interpreter)` — the same `interpreter` flag `stage0`
   already passes to `fork_dbproxy` (`false` for the standalone binary, `true` when a wheel's
   interpreter is live). The parent closes the write end immediately after the fork.
2. **Child identity:** call **`blastit_sys::drop_privileges(uid)` and nothing else**. It
   clears ambient/inheritable/bounding, `setgroups(0, NULL)`, `setresgid(uid, uid, uid)`,
   `setresuid(uid, uid, uid)`, then `capset(0, 0, 0)` and verifies. **Inlining
   `setresgid`/`setresuid` is forbidden:** the kernel clears capabilities only when all uids
   cross 0, and a rootless `init` never was 0, so the inline form keeps `CAP_DAC_OVERRIDE`
   and reads a `0000` entry as `O`. (gid == uid; there is no separate route gid.)
3. **Child hardening:** `harden_child(Signal::KILL)`, `install_filter(self.seccomp,
   SeccompRole::View { egress: false })` (`ProductionProbe.seccomp` is `config.seccomp`, so
   `BLASTIT_SECCOMP=0` is honoured exactly as in `stage0`), `set_dumpable(false)`.
4. **Child check:** open `manifest_dirfd` (`O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC`);
   when `root` is given, `openat2(manifest_dirfd, root, O_PATH|O_DIRECTORY|O_NOFOLLOW|
   O_CLOEXEC, RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS)` and require
   `entry.strip_prefix(root)` (else `OutsideRoot`); then `openat2(manifest_dirfd, entry,
   O_PATH|O_NOFOLLOW|O_CLOEXEC, RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS)` — **`O_PATH`, never a
   readable `O_RDONLY` open**, so a route-owned FIFO cannot block the child — `fstat`
   `S_ISREG`, then `faccessat2(fd, "", R_OK, AT_EMPTY_PATH|AT_EACCESS)` via
   `unsafe { libc::syscall(libc::SYS_faccessat2, fd, c"".as_ptr(), libc::R_OK,
   libc::AT_EMPTY_PATH | libc::AT_EACCESS) }` with a `// SAFETY:` comment (`rustix`'s
   `accessat` rejects `EMPTY_PATH` with `EINVAL`; `libc` is already a `crates/blastit` dep).
5. **Reply:** exactly one byte — `O` ok, `M` missing, `U` unreadable, `N` not regular,
   `S` symlink, `R` outside root, `I` I/O, `T` timeout — then `_exit(0)`. Errno mapping: `ENOENT → M`,
   `EACCES`/`EPERM → U`, `ELOOP → S`, `EXDEV → R`, `EISDIR`/`ENXIO`/`ENODEV → N`, everything
   else → `I`.
6. **Parent:** `child::wait_readable(read_fd, Duration::from_secs(5))`; on timeout
   `kill(probe_pid, Signal::KILL)` + `waitpid` → `SocketProbeError::Timeout`. Otherwise
   `libc::waitpid(probe_pid, &mut status, 0)` (never `blastit_sys::reap()`, which is
   `WNOHANG` and reaps any child) and read exactly one byte. Any reply that is not exactly one byte from the pinned set
   (`O`, `U`, `N`, `S`, `R`, `M`, `I`, `T`) is boot-fatal `SocketProbeError::Io` with the
   distinct message `probe child exited without a reply` (`exit 1`); a **diagnostic** byte
   maps to its `SocketProbeError` variant (`U → Unreadable { uid }`, `N → NotRegularFile`,
   `S → Symlink`, `R → OutsideRoot`, `M → Missing`, `I → Io`, `T → Timeout`) and every
   non-`O` byte still aborts boot, **never** "readable".

`DevProbe` needs no child (it already runs as the invoking uid) and calls the same
`O_PATH`/`fstat`/`faccessat2` sequence inline. `ProductionProbe::parent`/`check_leaf` return
`Ok(None)`/`Ok(())`; `B1`/`B2` run in `stage0` (`MB.3.6.3.1`).

**`ENOSYS` policy.** `openat2` has the per-segment `O_NOFOLLOW` fallback (`B2`), but
`faccessat2` is **required**: on `ENOSYS` the probe refuses to boot with
`./blastit.toml:<line>: fastcgi entry "<entry>": faccessat2 is unavailable (Linux < 5.8);
blastit requires it for the route-uid readability check`. Hosts with kernels older than 5.8
(RHEL/CentOS 8 4.18, Debian 10 4.19, Ubuntu 18.04 4.15, Amazon Linux 2 4.14) are therefore
unsupported for `[route.fastcgi]`; there is no silent weaker check.

A `0000`-mode entry owned by another uid therefore yields `Unreadable { uid }` in
production, not a silent pass. Oracles: the unprivileged `MB.3.5.2` fake-probe unit, and the
root-only `e2e/rootless/run.py::check_fastcgi_entry_0000_refused_for_route_uid` (`MB.3.7`).
No caller-path `chown`/`chmod`/`connect` is introduced, and `blastit-manifest` still has no
`libc`/`rustix` edge (the child lives in `crates/blastit/src/probe.rs`).

| `SocketProbeError` (entry path) | `FastCgiEntryInvalid.rule` |
|---|---|
| `Missing` | `must exist` |
| `NotRegularFile` | `must be a regular file` |
| `Unreadable { uid }` | `must be readable by uid <uid>` |
| `Symlink` | `must not be a symlink` |
| `OutsideRoot` | `must be under static root "<root>"` |
| `Io(_)` | `must be stat-able` |
| `Timeout` | `must be stat-able (probe timed out)` |

The fake probe's `Timeout`→rule mapping is one oracle; the real 5 s path (`child::wait_readable` →
`kill(SIGKILL)` → `waitpid` → `Timeout`) is the one that can hang a boot, so it carries its own
`#[ignore] requires-root` test, `probe_child_that_never_replies_is_killed_at_five_seconds`
(`critical_fastcgi_manifest` cannot fork `blastit`; it lives in `MB.3.6.3.1`'s `crates/blastit`
tests and runs in `e2e/rootless` for the full boot).

`rule` is a `String` (the phrases interpolate `uid`/`root`), never `&'static str`. The
derived-parent path of `check` maps to its own variant and its own fixed phrases:

| `SocketProbeError` (derived-parent path) | `FastCgiPoolSocketInvalid.rule` |
|---|---|
| `Missing` | `does not exist` |
| `NotSocket` | `is not a socket` |
| `NotDirectory` | `is not the private directory blastit created` |
| `Symlink` | `is a symlink` |
| `ForeignOwner { uid }` | `is owned by uid <uid>` |
| `InsecureMode { mode }` | `is not 0700` (`mode` is the observed mask) |
| `NotConnectable { uid }` | `is not connectable by uid <uid>` |
| `Io(_)` | `must be stat-able` |
| `Timeout` | `must be stat-able (probe timed out)` |

`SocketProbe` and `check_fastcgi_sockets` stay **pure** in `crates/blastit-manifest/src/probe.rs`
(they call the injected probe and validate state; no syscalls). The two probe
implementations live in `crates/blastit/src/probe.rs` (`MB.3.5.2`), where `rustix` is already
a dependency: `blastit-manifest` gains **no** `libc`/`rustix` edge and no `Cargo.toml` change.
`check_fastcgi_sockets` returns `Vec::new()` for `ProductionProbe` (`check` → `Ok(None)`) and
one `(route_index, parentfd)` per FastCGI route for `DevProbe`; a probe that returns `Some(fd)` while `provides_parent_fd()` is `false` is a `ManifestErrors`
programmer error (checked in `check_fastcgi_sockets`, not by downcasting).
A test injects a fake `SocketProbe` returning `ForeignOwner { uid: other_uid }` / `NotSocket` /
`Symlink` / `NotDirectory` from `check`, and `NotRegularFile` / `Unreadable { uid }` /
`OutsideRoot` from `check_entry`, without needing root; the real multi-uid case (a route uid
that cannot traverse or rebind the derived parent) is in `e2e/rootless` (§7.2).

`ProbeUid` is authoritative for readability; `DevProbe.uid` is used only by `check`'s
connect-as-uid, and `check_entry` uses its `uid` argument. `DevProbe::parent` is the **one**
resolver for `dev`/`check`: `stage0::run_dev` takes its
`Some(parentfd)` as the descriptor it hands over (§4.8.1), and `check::execute` collects the
`Vec<(usize, OwnedFd)>` from `check_fastcgi_sockets` and passes it to `serve_and_run`, which
`clear_cloexec`s each right before spawning the child `dev` (`BLASTIT_CHECK_POOL_PARENT_FD`).
The child re-validates the received fd (`fstat` `S_ISDIR`, `fstatat` `S_ISSOCK`) but never
calls `openat2` again, so there is exactly one resolution per parent and no second handoff
source. Because the leaf must already exist when `check` runs (`R3`, `R20`), the **call site
now follows the pool's start**: `MB.3.9.3` owns that ordering in `dev` and `check`; this
document pins only what `check` proves (`R3`) and that its input is `<run_root>`, never a
manifest string.

**The environment map is derived, not a key.** Production writes the pool's
`env[…]` lines from the environment init itself holds (the sanitized stage-1
environment) minus every `BLASTIT_*` name and minus `HTTP_PROXY`/`http_proxy` (so an
ambient proxy variable cannot re-open `httpoxy`), rewriting any `host=/proc/self/cwd` in
proxy outputs to `host=<runtime>/views/<uid>` (because PHP's SAPI `chdir`s to
`dirname(SCRIPT_FILENAME)` on each request) and setting `env[HOME]=<runtime>/views/<uid>/tmp`
and `env[TMPDIR]=<runtime>/views/<uid>/tmp`, INI-escaped; `clear_env = yes` stays.
There is no `[route.fastcgi.env]`, no `BLASTIT_ROUTE`: the page's table has two keys,
and a third is a page change with its own DX node. `env` and any other key are `UnknownKey`
errors; `listen` has its own refusal (`FastCgiListenDerived`, above).

**Exact `docs/reference/manifest.md` reconciliation (`MB.3.5.2`).** Three surgical edits
keep `manifest.md`, `docs/frameworks/php.md` and `blastit-manifest` identical without
touching `MB.6`'s `[route.static]` prose:
1. Top-level `app` row and footnote (`manifest.md:13, 20-22`): `app` is required when any
   route declares neither `[route.static]` nor `[route.fastcgi]`; a manifest whose every
   route is `[route.static]` or `[route.fastcgi]` needs no `app` (setting `app` or
   `BLASTIT_APP` when **every route is `[route.static]` and none is `[route.fastcgi]`**
   remains a boot error — that is the predicate the 0.2.0 rule uses, not the narrower
   *static-only* of ADR-0008; on a manifest with any `[route.fastcgi]`, `app` is accepted
   and ignored for those routes).
2. Routes table (`manifest.md:77`) + new `## FastCGI` section: add the `[route.fastcgi]`
   row (`entry`, optional `workers` derived from `BLASTIT_VIEW_MAX_WORKERS` /
   `BLASTIT_MAX_WORKERS`; no socket key) and a concise section mirroring `php.md`'s 2-key
   table, `[route.static]` composition, and the `.php`-never-served rule.
3. Static dead-declaration & Validation at boot (`manifest.md:142-145, 324-325`): change
   "a manifest with no `app`" to "a static route with neither `app` nor `[route.fastcgi]`"
   (and state `spa = true` cannot combine with `[route.fastcgi]`).

### 4.2 The backend contract (`crates/blastit-backend`, `MB.9.1`)

One trait; nothing in it knows PHP, Python, static files or a server:

```rust
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Upstream: Send + Sync {
    /// Backend kind identifier ("static"/"wsgi"/"fastcgi"), for diagnostics and tests only.
    /// Never logged; the `request` event has no backend field.
    fn kind(&self) -> &'static str;
    /// Can this stage take one request now? No I/O; delegates to the shared `Arc<RouteState>`.
    fn capacity(&self) -> Capacity;
    /// Idempotent, rate-limited request to start the executor (init spawn).
    fn ensure_ready(&self) -> BoxFuture<'_, Result<(), UpstreamError>>;
    /// A lifecycle signal from the scheduler (`ExecutorReady`/`ExecutorExited`); updates `RouteState`.
    fn signal(&self, _signal: UpstreamSignal) {}
    /// Validate the head; state what this stage needs from the body.
    fn plan(&self, head: &UpstreamHead) -> Result<UpstreamPlan, UpstreamError>;
    /// Perform the exchange under the existing credits; return what was observed.
    /// `body` owns its receiver and `sink` is owned (boxed), so the scheduler can move
    /// both into a `'static` task; neither borrows the scheduler.
    fn exchange<'a>(
        &'a self,
        plan: UpstreamPlan,
        body: Box<dyn BodySource + Send>,
        sink: Box<dyn ResponseSink + Send>,
    ) -> BoxFuture<'a, Result<ExchangeOutcome, UpstreamError>>;
}
```

Object safety: every method takes `&self`, none is generic, none returns `impl Trait`,
the one async method returns the `BoxFuture` alias. `Arc<dyn Upstream>` compiles
(`MB.9.1` asserts `fn _obj(_: Arc<dyn Upstream>)` and that the returned future is
`Send`). **No `async_trait`, no new dependency, no fallback variant.** `Pin<Box<dyn
Future + Send + 'a>>` is stable on MSRV 1.88; there is no second path.

#### 4.2.1 Contract types

| Item | Exact shape | Notes |
|---|---|---|
| `RouteId` | `#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)] pub struct RouteId(u32)` | minted only by `Registry::build_route` / `BuildCtx` (`pub const fn index(self) -> usize`); adapters cannot forge another route's index |
| `Capacity` | `enum Capacity { Ready, Busy }` (`Debug, Clone, Copy, PartialEq, Eq`) | `Busy` means queue; the scheduler keeps the global slot/memory gate (`slots < max_workers`, `mem_high_watermark`) while `capacity()` delegates to the shared `Arc<RouteState>` (zero I/O); `ensure_ready` runs at most once per admission tick |
| `RouteState` | `pub struct RouteState { ready: AtomicBool, inflight: AtomicU32, worker_cap: u32 }` (`new(ready, worker_cap)`, `capacity(&self) -> Capacity`, `set_ready(bool)`, **`try_acquire(self: &Arc<Self>) -> Option<RoutePermit>`**) where `RoutePermit { state: Arc<RouteState> }` is `Send + 'static` and its `Drop` decrements `inflight` | **one** shared `Arc<RouteState>` per stage, created by the scheduler (`worker_cap = view_worker_cap` for WSGI, `policy.resolved_workers(cfg.view_worker_cap())` for FastCGI (**pool leaf's** resolution, gated on `MB.12`) — the same number `pm.max_children` gets, §4.8.1, `u32::MAX` for static; `ready = true` for static, `false` for WSGI views, and `false` for every FastCGI pool once `MB.3.9.4` makes `dev`/`check` `Managed` too — a pool is never `Ready` before its `Ctl::PoolReady`/leaf proof) and passed in `BuildCtx.state` so the scheduler and adapter read the exact same state. `try_acquire` takes `&Arc<Self>` (not `&self`) so a `'static` `tokio::spawn` task can hold the permit: the permit owns a clone of the `Arc`, never a borrow; `None` means `inflight >= worker_cap` and dispatch answers `Unavailable { retry_after: Some(1) }` (`503`) |
| `PinnedSocket` | `#[cfg(unix)] #[derive(Clone, Debug)]` (**not** `PartialEq`/`Eq`: it holds `Arc<OwnedFd>`/`Mutex`) `pub struct PinnedSocket { parent: Arc<OwnedFd>, name: CString, parent_dev: u64, parent_ino: u64, expected_uid: u32, peer_uid: u32, peer_gid: u32, enforce_socket_inode: bool, bound_inode: Arc<Mutex<Option<(u64, u64)>>> }` with `pub fn new(parent: Arc<OwnedFd>, name: CString, parent_dev: u64, parent_ino: u64, expected_uid: u32, peer_uid: u32, peer_gid: u32, enforce_socket_inode: bool) -> Result<Self, PinnedSocketError>` and accessors `parent_fd(&self) -> BorrowedFd<'_> { self.parent.as_fd() }`, `name(&self) -> &CStr`, `parent_inode(&self) -> (u64, u64)`, `expected_uid(&self) -> u32`, `peer(&self) -> (u32, u32)`, `enforce_inode(&self) -> bool`, `bound_inode(&self) -> Option<(u64, u64)>`, `set_bound_inode(&self, (u64, u64))` | server-agnostic `std` descriptor + validated leaf filename (no `'/'`, non-empty, not `"."`/`".."`); `new` is **pure** (name validation only — `PinnedSocketError::BadName` for empty/`..`/`/`-bearing names — so `blastit-backend` stays zero-I/O) and the caller passes the already-`fstat`ed `(parent_dev, parent_ino)`; `set_bound_inode`/`bound_inode` lock the `Mutex` and **map `PoisonError` to `PinnedSocketError::Poisoned`** (no `unwrap`); `new` takes the same `Arc<OwnedFd>` the caller keeps (no `try_unwrap`, no `dup`, no `unsafe`), so `SchedulerConfig.pinned_sockets: Vec<(usize, PinnedSocket)>` and init's per-route pool state share one descriptor; `MB.3.6.2`'s tests build one from a hand-rolled temporary directory (no `tempfile` crate) and a `UnixListener` via `new`; carries the `O_RDONLY\|O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC` `parentfd` that stage 0 pinned at boot, crossed the stage-0→stage-1 `execve` as an internal `--fastcgi-parent-fd=<route>:<n>` argv entry (§4.8.1), and reaches `SchedulerConfig.pinned_sockets` → `BuildCtx.socket` by fork. `expected_uid` is the **socket owner** (`scheduler_uid` in production, `invoking_uid` in `dev`/`check`); `peer_uid`/`peer_gid` are the **listening pool master's** credentials — the socket creator that `SO_PEERCRED` reports — `RouteSpec.uid` and its layout `route_gid` in production (the `php-fpm` master creates the socket as `route_uid:route_gid`), `invoking_uid`/`invoking_gid` in `dev`/`check`; setting `peer_uid = scheduler_uid` in production is a bug that `502`s every request. In production (`enforce_socket_inode = true`), `bound_inode` holds the exact `(sock_dev, sock_ino)` received in `Ctl::PoolReady`; in `dev`/`check` (`enforce_socket_inode = false`), `parentfd` + `expected_uid` stay pinned while allowing the pool to be restarted in place. **No implementation may ever `open`, `openat` or `connect` a `listen` path string, and no code path re-opens the parent segment after boot** |
| `UpstreamSignal` | `enum { ExecutorReady { socket_inode: Option<(u64, u64)> }, ExecutorExited }` | sent by the scheduler's `on_ctl` on **both** view `Ready`/`Exited` (`socket_inode: None`) and pool `PoolReady`/`PoolExited` (`socket_inode: Some((sock_dev, sock_ino))`); `FastCgiUpstream::signal` stores `socket_inode` before `self.state.set_ready(true)` and clears it on `ExecutorExited` |
| `UpstreamHead` | `#[derive(Debug, Clone, PartialEq, Eq)] struct { method: Vec<u8>, target: Vec<u8>, headers: Vec<(Vec<u8>, Vec<u8>)>, scheme: Tls, server_name: String, server_port: u16, remote_addr: String, peer_addr: String, body: BodyFraming, expect_continue: bool }` | built once in `begin_raw` from `ReqHeadView` (`scheme() -> Scheme` (the frame-owned **`blastit_frame::Scheme`** root re-export; inside `raw.rs` it is `crate::frame::Scheme`; `begin_raw` maps `Scheme::Http&#124;Https → Tls::Http&#124;Https`), `server_name`/`server_port` accessors added in `MB.9.2.1.1`; `body: BodyFraming` and `expect_continue: bool` derived in `begin_raw` from `Content-Length`/`Transfer-Encoding` and `Expect: 100-continue` before `Expect` is stripped); **not** `blastit_http::RequestHead`; `Tls { Http, Https }` is contract-owned, `BodyFraming` from `blastit-http`. Every `String` field is converted with `String::from_utf8_lossy` (never `from_utf8` unwrap). `tests/head_allocs.rs` documents the bounded copy (`MB.9.2.1.2`). WSGI re-encodes `REQ_HEAD` from these fields |
| `UpstreamPlan` | `{ head: UpstreamHead, body: BodyMode, expects_continue: bool }` | by value, so a `Declined` stage hands the head on |
| `BodyMode` | `enum { None, Buffered { max: u64 }, Stream { credit: u32 } }` | what the stage consumes, not the client's framing |
| `BodySource` | `trait: Send { fn next<'a>(&'a mut self) -> BoxFuture<'a, BodyEvent>; fn cancel_token(&self) -> CancelToken; }` (contract, object-safe) | the body seam: the scheduler's concrete `BodyStream` (tokio `mpsc::Receiver<BodyEvent>` + `Arc<CancelToken>`, in `blastit-scheduler`) implements it; the contract crate stays free of `tokio`/`std::sync::mpsc`, and `MB.9.1.1`'s `! grep -rn '…\|tokio' crates/blastit-backend/src` still passes |
| `CancelToken` | `#[derive(Clone, Debug)] pub struct CancelToken(Arc<AtomicBool>)` with `fn cancel(&self)` and `fn is_cancelled(&self) -> bool` | contract-defined (std only); the scheduler's `BodyStream` holds one and closes its channel on `cancel()`. `Credit` is **not** a contract type: upload/download credit lives in the scheduler's `Outbound`/body pump, and `BodySource::next` yields only after credit, so no credit value crosses the seam |
| `BodyEvent` | `enum { Chunk(Vec<u8>), End, Cancelled, Refused { status: u16 } }` | `Refused` is an ingress refusal found mid-stream |
| `ReplyHead` | `{ status: u16, reason: Vec<u8>, headers: Vec<(Vec<u8>, Vec<u8>)> }` | contract-owned; the sink validates/strips headers and maps it to `Frame::RespHead` |
| `ResponseSink` | `trait: Send { fn head<'a>(&'a mut self, ReplyHead) -> BoxFuture<'a, Result<(), UpstreamError>>; fn body<'a>(&'a mut self, Vec<u8>) -> BoxFuture<'a, Result<(), SinkError>>; fn end<'a>(&'a mut self) -> BoxFuture<'a, Result<(), SinkError>>; fn output(&mut self, OutputStream, String); fn head_sent(&self) -> bool; }` | `head` strips hop-by-hop/`Content-Length`/`X-Powered-By`/`Server`/`Proxy-*`, **refuses exactly four proxy-control names** (`X-Accel-Redirect`, `X-Sendfile`, `X-Lighttpd-Send-File`, `X-Rewrite-URL`, ASCII case-insensitive) with `UpstreamError::Fault { detail: "forbidden proxy-control response header" }` (`502` before any head is sent), and **drops every other `X-Accel-*` name** (`X-Accel-Buffering`, `-Expires`, `-Charset`, `-Limit-Rate`) without a fault, so a benign SSE directive cannot turn a `200` into a `502`; `body` awaits download credit; `output` receives lossy-UTF-8 lines (`String::from_utf8_lossy`) |
| `SinkError` | `enum { Cancelled, Closed }` | no status |
| `ExchangeOutcome` | `enum { Answered { status: u16, bytes_in: u64, bytes_out: u64, protocol_status: ProtocolStatus, dropped: u32, observations: Vec<Observation> }, Declined { next: Arc<dyn Upstream>, plan: UpstreamPlan } }` | `Declined` writes no head |
| `ProtocolStatus` | `enum { Complete, Failed(u8) }` | the upstream protocol's own completion status; static/WSGI always `Complete`; `Failed` maps to `UpstreamError::Fault`; never logged and absent from the request event |
| `UpstreamError` | `enum { Refused { status: u16, reason: Refusal }, Unavailable { retry_after: Option<u32> }, Fault { detail: &'static str }, Cancelled }` | `Refusal::status()` is authoritative (`Refused::new(reason)` sets `status = reason.status()`); `Unavailable` → **`503`** (with `Retry-After` when `Some`); `Fault` → **`502`** before head (`RespAbort` after head); `Cancelled` → no status. `detail` is a fixed phrase, never a response byte; hand-written `Display` |
| `Refusal` | `enum { MalformedPath, Hidden, Missing, SpaLost, EntryScript, MethodNotAllowed { allow: Option<&'static [u8]> }, DeclaredBody, TooLarge, Ingress, ParamsTooLarge, DuplicateParam, UnknownParam }` (`Debug, Clone, Copy, PartialEq, Eq`) | sole status authority `fn status(&self) -> u16`: 400, 404, 404, 404, 404, 405, 413, 413, 400, 400, **500** (programmer error), 500; one `fn allow(&self)` |
| `Observation` | `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] { target: ObsTarget, #[serde(flatten)] kind: ObservationKind }` | `ObservationKind` is **internally tagged** (`#[serde(tag = "kind", rename_all = "snake_case")]`), which is what makes `#[serde(flatten)]` legal (an externally tagged enum cannot be flattened); the wire line is `{"kind":"wire_request","target":{"scope":"routed","route":"app"},…}` and `Query` serialises as `{"kind":"query",…}`. `UpstreamError`/`Refusal`/`BodyMode` are **not** serde types (never in the learn/check JSON). Exact-line tests: `blastit-backend` `types::tests::wire_request_line_is_exact` and `critical_fastcgi_learn::http_obs_line_is_exact` |
| `ObsTarget` | `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] #[serde(tag = "scope", rename_all = "snake_case")] enum { Routed { route: String }, Unrouted { method: String, path: String } }` | byte-identical to `blastit_dbproxy::learn::ObsTarget`; a test parses a real `WireSqlObs` line with it |
| `ObservationKind` | `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] #[serde(tag = "kind", rename_all = "snake_case")] enum { WireRequest { method: String, path: String, status: u16, body_len: u64, content_type: Option<String>, #[serde(default = "default_true")] executor_ran: bool }, Query { proxy: String, sql: String }, Param { name: String, decision: ParamDecision }, StaticFile { path: String, status: u16, bytes: u64 }, Output { stream: OutputStream, line: String } }` | `Query` is **reserved**: no implementation may emit it in 0.2.1 (dbproxy's `WireSqlObs` is the SQL record); `executor_ran` is `true` for every `exchange` answer and `false` only for a pre-executor `plan` head `413` (so `missing_ingress` fires without counting the un-run front controller in `routes.exercised`); `Param` never carries a value; `content_type` and `line` are always `String::from_utf8_lossy`-converted (`U+FFFD` replacement, zero panics on arbitrary pool bytes) |
| `ParamDecision` | `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] enum { Emitted, Dropped }` | |
| `OutputStream` | `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] enum { Stdout, Stderr }` | |
| `SessionPort` | `trait: Send + Sync { fn spawn(&self, route: RouteId) -> Result<(), UpstreamError>; fn open(&self, route: RouteId) -> Result<(u32, Box<dyn ExchangeChannel>), UpstreamError>; }` | **WSGI-only** scheduler-side capability keyed by `RouteId` (never a raw `usize`); `open` returns the scheduler-allocated request id. FastCGI sessions are `Scheduler::session` (§4.2), not this trait |
| `ExchangeChannel` | `trait: Send { fn send<'a>(&'a mut self, bytes: Vec<u8>) -> BoxFuture<'a, Result<(), UpstreamError>>; fn recv<'a>(&'a mut self) -> BoxFuture<'a, Result<Vec<u8>, UpstreamError>>; fn close(&mut self); }` | opaque encoded frames (WSGI only) |
| `PoolPort` | `trait: Send + Sync { fn ensure_pool(&self, route: RouteId) -> Result<(), UpstreamError>; }` | scheduler-side capability **distinct from `SessionPort`**: `run_with` creates one `CtlPoolPort` (it owns an `mpsc::Sender<Ctl>` and the route's `Arc<RouteState>`) and passes it in `BuildCtx.pool`; `ensure_pool` sends `Ctl::Spawn { route }` once (idempotent/rate-limited by `RouteState.ready`), returns `Ok(())` when queued, and `Err(Unavailable { retry_after: Some(1) })` if `ctl.chan` is closed. `FastCgiUpstream::ensure_ready` calls it **only** when `!external_executor`; the first request then waits for `PoolReady` (bounded 5 s, §4.5.2) under the existing `request_timeout` |
| `Session` / `SessionMsg` | `pub(crate) struct Session { id: u64, done: mpsc::Receiver<SessionMsg> }`; `pub(crate) enum SessionMsg { Done { id: u64, result: Result<ExchangeOutcome, UpstreamError> } }`; `pub(crate) fn Scheduler::session(upstream: Arc<dyn Upstream>, plan: UpstreamPlan, body: Box<dyn BodySource + Send>, sink: Box<dyn ResponseSink + Send>, permit: RoutePermit) -> Session` | scheduler-side (not contract), **`pub(crate)`** so `private_interfaces` stays silent and `MB.3.6.2.2` tests behaviour only (never names the types); one `tokio::spawn`ed task per request owns all arguments (including the `RoutePermit`) and a `SessionDoneGuard`; the scheduler keeps `done` in `self.sessions`; `Done` is delivered exactly once (normal `finish` or panic `Drop`) on a bounded `mpsc::Sender<SessionMsg>` (capacity `1`) and is the only slot-release event |
| `BackendBuilder` | `trait: Send + Sync { fn kind(&self) -> &'static str; fn build(&self, spec: &RouteSpec<'_>, ctx: &BuildCtx<'_>, next: Option<Arc<dyn Upstream>>) -> Result<Arc<dyn Upstream>, BootError>; }` | `next` is the executor a static layer forwards to |
| `RouteSpec<'a>` | `{ name: &'a str, route: RouteId, uid: u32, line: u32, ingress: Option<&'a IngressPolicy>, static_policy: Option<&'a StaticPolicy>, static_forward: bool, executor: Option<&'a Executor> }` | manifest types only (`blastit_manifest::Executor`, re-exported at the crate root by `MB.3.5.1`; `FastCgiPolicy` likewise); built by `run_with` from `RouteInfo` plus `SchedulerConfig.route_lines[index]` (built by `stage1` from `Manifest::route_lines()`, routing order), so every `BootError` can print `./blastit.toml:<line>`; a short/missing `route_lines` entry is itself a `BootError` naming the route; `static_forward` says “a miss may reach an executor” |
| `BuildCtx<'a>` | `{ manifest_dir: &'a Path, route: RouteId, uid: u32, limits: BuildLimits, state: Arc<RouteState>, socket: Option<PinnedSocket>, session: Arc<dyn SessionPort>, pool: Option<Arc<dyn PoolPort>>, external_executor: bool }` where `BuildLimits { max_body_bytes: u64, static_max_bytes: u64, request_timeout: Duration }` | carries the route's shared `Arc<RouteState>` and (for FastCGI) the boot-pinned `PinnedSocket` from `SchedulerConfig.pinned_sockets`; `FastCgiBuilder` requires `ctx.socket.clone()` and `ctx.pool.clone()` by construction (`None` → `BootError`). `external_executor` is **contract-owned** (not `SpawnMode`, which is a scheduler type) and set by `run_with` from `cfg.pool_spawn == PoolSpawn::External`; it is `false` in every shipped mode after `MB.3.9.4`, so `ensure_ready` always reaches `PoolPort::ensure_pool` and blastit always starts the pool it owns |
| `Registry` | `fn new(Vec<Arc<dyn BackendBuilder>>) -> Result<Self, BootError>; fn build_route(&self, &RouteSpec, &BuildCtx) -> Result<Vec<Arc<dyn Upstream>>, BootError>` | looks up `executor.kind()` (single source of executor kinds), then wraps with `"static"` when `spec.static_policy.is_some()`; returns `[static?, executor?]` |
| `BootError` | `{ manifest: PathBuf, line: u32, route: String, kind: String, detail: String }` | `./blastit.toml:<line>: <detail>`; hand-written `Display` |

**Zero panics on wire/pool bytes.** Every conversion from request headers, WSGI frames or
FastCGI `STDOUT`/`STDERR` bytes into a Rust `String` uses `String::from_utf8_lossy`
(`char::REPLACEMENT_CHARACTER` `U+FFFD` for invalid UTF-8); no `from_utf8` unwrap or UTF-8
panic exists anywhere on the session path.

**Where cancellation and backpressure live.** Cancellation is the contract `CancelToken`
held by the scheduler's concrete `BodyStream`; `BodySource::next()` yields `Cancelled`, sink
calls return `SinkError::Cancelled`, and a `Drop` guard in each implementation sends what
its protocol offers and closes the transport. Backpressure is `ResponseSink::body` (awaits
download credit) and `BodySource::next` (yields only after upload credit). No implementation
buffers a whole response. `Cancelled` is the one error with no client-visible status.

**`app.output` ownership.** An implementation calls `sink.output(stream, line)`; the
sink records a bounded `Observation::Output` and forwards the line to **init** over the
events pipe, which emits the `app.output` event. Implementations write no events
themselves, and the scheduler is not the event writer (`ARCHITECTURE.md` §8.1).

**Injection, not selection.** Dispatch calls `plan` then `exchange` on the route's
handle and never `match`es on backend kind — no `cfg!`, no `match backend_kind`, no
globals/`lazy_static`/`OnceCell` lookup per request. Backend kind is consulted in two
non-dispatch places only: boot stage assembly (`Registry::build_route`) and process
lifecycle (`supervise`/`on_ctl` `Ctl::Spawn` and `Pool*` frames). The dispatch call site:

```rust
// scheduler.rs — one path for every backend, no `kind` consulted
let runtime = self.routes.get(route).ok_or_else(|| /* 404 */)?;
let stage = usize::from(request.stage);           // 0 or 1
let upstream = runtime.stages.get(stage).ok_or_else(|| /* Fault */)?.clone();
let plan = upstream.plan(&head)?;                 // Refused -> canned status; never 5xx for a bad head
// admission (slot, queue) and the body plan, exactly 0.2.0's rules
let permit = state.try_acquire().ok_or(UpstreamError::Unavailable { retry_after: Some(1) })?;
let sink = Box::new(self.sink_for(request.id));   // owns its senders; no borrow of `self`
let session = self.session(upstream, plan, body, sink, permit); // one 'static task
self.sessions.insert(session.id, session.done);   // the scheduler keeps the receiver
request.session = Some(session.id);               // completion arrives as SessionMsg::Done
```

The scheduler-side seam (in `scheduler.rs`, not the contract). **One shape**, bounded
channel, receiver owned by the scheduler:

```rust
pub(crate) struct Session { id: u64, done: mpsc::Receiver<SessionMsg> }   // JoinHandle held by `session()`
pub(crate) enum SessionMsg { Done { id: u64, result: Result<ExchangeOutcome, UpstreamError> } }
impl Scheduler {
    /// One `tokio::spawn`ed task per request. It owns `upstream`/`plan`/`body`/`sink`
    /// and the `RoutePermit`; a `SessionDoneGuard` is its only `Done` emitter.
    fn session(&self, upstream: Arc<dyn Upstream>, plan: UpstreamPlan,
               body: Box<dyn BodySource + Send>, sink: Box<dyn ResponseSink + Send>,
               permit: RoutePermit) -> Session;
}
```

Admission is `RouteState::try_acquire()` **before** `Scheduler::session` (non-blocking; the
0.2.0 queue/`mem_high_watermark` gate runs first, and a `None` is
`Unavailable { retry_after: Some(1) }` → `503`). The permit moves into the task and its
`Drop` decrements `inflight`; `SessionDoneGuard` holds `Option<mpsc::Sender<SessionMsg>>`
(capacity `1`), the normal path calls `guard.finish(result)` (sends once, then `take()`s the
sender), and `Drop` sends `Done { id, result: Err(UpstreamError::Fault { detail: "session
task panicked" }) }` only if the sender is still there — so exactly one `Done` is delivered,
the slot is released on that message, and a panic never unwinds `run_with`. `SessionPort` is
**WSGI-only** (view spawn/open); FastCGI pool starts go through `PoolPort` (§4.2.1), not
`SessionPort`. The scheduler's one `select!` polls `self.sessions`; `Done` is the only event
that releases the slot, emits the observation line and accounts bytes (no `.await` inside
the scheduler's borrow).

**Where the handle lives.** `RouteInfo` stays the manifest-derived descriptor the
protected suites construct and assert (`critical_rewrite.rs:612-678` reads
`static_policy`/`static_forward`), gains `executor: Option<Executor>`
(`None` = static-only, `Some(View)` = WSGI, `Some(Pool(FastCgiPolicy))` = FastCGI;
`RouteInfo::new` defaults to `executor: Some(Executor::View)`, the 0.2.0 plain-route
default), and keeps `static_forward` meaning **“a miss may reach an executor”**:
`policy.is_some() && !spa && (has_app || fastcgi.is_some())`. In
`routes_from_manifest` (`route.rs`), **`with_fastcgi(policy)` MUST run after
`with_static(policy, has_app)`**: `with_static` sets `static_forward =
policy.as_ref().is_some_and(|p| !p.spa && has_app)` and `executor =
if policy.is_none() || static_forward { Some(View) } else { None }`; then `with_fastcgi`
(when `Some(fcgi)`) sets `executor = Some(Pool(fcgi))` and updates
`static_forward = static_policy.as_ref().is_some_and(|p| !p.spa)`, so the page's app-less
PHP route forwards to its front controller while a static-only route with no app has
`fastcgi = None` and keeps `static_forward = false` and `executor = None`. The injected
handle lives beside the descriptor in the scheduler, still one non-optional stage chain
per route built once at boot:

```rust
struct RouteRuntime { info: RouteInfo, stages: Vec<Arc<dyn Upstream>> }   // 1 or 2
struct Scheduler { routes: Vec<RouteRuntime>, /* … */ }
```

`Request.stage: u8` (0 or 1) indexes `stages`; there is no `Option<Box<dyn …>>`, no
lookup by name, no rebuild. Adding `Arc<dyn Upstream>` to `RouteInfo` would break its
`Eq` derive and the protected value comparisons, which is why the handle is one level up.
`blastit_manifest::Executor` (`#[derive(Debug, Clone, PartialEq, Eq)]` — **not** `Copy`, because
`Pool` owns a `FastCgiPolicy` with `String` fields; `FastCgiPolicy` carries the same four
derives; `Executor::kind(&self) -> &'static str`, returning `"wsgi"`
for `View` and `"fastcgi"` for `Pool`; `pub const ALL_KINDS: &'static [&'static str] =
&["wsgi", "fastcgi"];`) is the single source of executor kinds (a new
backend in `MB.4`/`MB.5` adds one `Executor` variant, one `Upstream` impl, and one
`BackendBuilder`; `Registry::new` rejects duplicate or unknown builder kinds outside
`"static"` and `Executor::ALL_KINDS`). `Registry::build_route` builds `executor` first via
`executor.kind()` and then, whenever `spec.static_policy.is_some()`, builds `"static"` in
front with `next = executor.filter(|_| spec.static_forward)`:
- plain route (`app` or app-less `RouteInfo::new` in `World` / `critical_ingress_enforcement`): `[wsgi]`;
- static-only or `spa = true` route: `[static]` (`next = None`);
- `app` + `[route.static]` (`spa = false`): `[static, wsgi]`;
- `[route.static]` + `[route.fastcgi]` (`spa = false`, no `app`): `[static, fastcgi]`;
- `[route.fastcgi]` alone: `[fastcgi]`;
- `--learn` fallback slot: `[wsgi]`.

**One predicate, one spelling.** Every place in the codebase uses these exact definitions:
- `Manifest::is_all_static(&self) -> bool`:
  `self.app.is_none() && !self.routes.is_empty() && self.routes.iter().all(|r| r.static_policy().is_some() && r.fastcgi().is_none())` (0.2.0 meaning plus `&& r.fastcgi().is_none()`);
- `Manifest::can_boot_without_app(&self) -> bool`:
  `!self.routes.is_empty() && self.routes.iter().all(|r| r.fastcgi().is_some() || r.static_policy().is_some())` ("every route is static-only or has its own executor");
- per-route `executor`: `Some(Pool)` when `fastcgi.is_some()`, `Some(View)` when `fastcgi.is_none() && (static_policy.is_none() || static_forward)`, `None` otherwise;
- `check` external-executor gate: `config.app.is_none() && manifest.can_boot_without_app() && !manifest.is_all_static()` (§1.3, §4.10.3, §8).

`critical_ingress_enforcement`'s plain app-less route gets `Some(View)` (`[wsgi]`), and
`critical_static_cli`'s MIXED fixture (`api` plain + `site` static, no `app`) has
`can_boot_without_app() == false` so it stays on the pytest branch and preserves `"no
interpreter to run pytest on"`. No other spelling exists in the design.

`kind()` is `"static"`/`"wsgi"`/`"fastcgi"`; a missing builder in `Registry::build_route`
fails closed at boot: `./blastit.toml:<line>: no builder registered for backend
"fastcgi": this build registers static, wsgi` (line from `Manifest::route_lines()`,
directly unit-tested in `MB.9.1.1` with an incomplete `Registry`).
Builders run in the scheduler process **after** the role fork (so static roots are opened
by the scheduler role, while FastCGI `PinnedSocket`s in `SchedulerConfig.pinned_sockets`
wrap the `parentfd`s that stage 0 pinned before the `execve` and handed over as
`--fastcgi-parent-fd` entries — `FastCgiBuilder` and `FastCgiUpstream` **never** resolve the
socket or its parent by path), before any listener is served. `crates/blastit/src/roles.rs` (`scheduler`, forked by `stage1.rs`)
is the single composition-root call site: it calls
`blastit_scheduler::run_with(blastit_scheduler::builtin_registry()?, …)` and writes any
`RunError::Boot` (`eprintln!("{err}")`, captured by init's stderr pipe as `app.output` in
production and printed on fd 2 in `dev`) before returning `1`.

**`run_with`, `RunError` and the transitional registry (`MB.9.2.1`–`MB.9.3`).**
`pub async fn run_with<C: CtlChannel + Send + 'static>(registry: Registry, cfg: SchedulerConfig, routes:
Vec<RouteInfo>, table: RoutingTable, http: UnixStream, ctl: C, emit: Emit) ->
Result<(), RunError>` lives in
`crates/blastit-scheduler/src/scheduler.rs`; `pub enum RunError { Boot(BootError),
Scheduler(SchedulerError) }` and `SchedulerError::Boot(BootError)` live in
`crates/blastit-scheduler/src/error.rs` (both files owned by `MB.9.2.1.2`). `SchedulerError::Boot`
is declared `#[error("{0}")] Boot(BootError)` with **no** `#[source]`, and `BootError`
implements `std::error::Error` (in `MB.9.1.1`) so the `thiserror` derive is satisfied. `run`
keeps its 0.2.0 signature and is written out (it is `async`, so no `.map_err` on the future):

```rust
pub async fn run<C: CtlChannel + Send + 'static>(/* 0.2.0 args */) -> Result<(), SchedulerError> {
    run_with(builtin_registry().map_err(SchedulerError::Boot)?, /* … */)
        .await
        .map_err(RunError::into_scheduler)
}
```

where `into_scheduler` maps `Boot(e) => SchedulerError::Boot(e)` and `Scheduler(e) => e`.
`builtin_registry() -> Result<Registry, BootError>` is **defined in `scheduler.rs`** (it calls
the fallible `Registry::new` over a fixed builder list; no `expect`/`unwrap`) and only
**re-exported** from `lib.rs`, so `run` (also in `scheduler.rs`) can call it. `MB.9.2.2`
registers `"static"`, `MB.9.3` adds `"wsgi"`, and `MB.3.6.2.1` adds `"fastcgi"` — each
appends to the list **in `scheduler.rs`**, which all three leaves own — and
`MB.3.6.2.2` asserts `builtin_registry()` registers `"static"`, `"wsgi"`, `"fastcgi"`.

**Transitional rule (until `MB.9.3`).** Before `MB.9.3` nothing is registered for a plain
route, so `run_with` builds a stage chain **only** for a route whose `executor.kind()` has a
registered builder; a route with no builder gets `stages: Vec::new()` and dispatch serves it
through the 0.2.0 in-place path (`RouteRuntime.stages.is_empty()` is a shape check, not a
`kind` match). `MB.9.3` deletes that fallback: after it, a missing builder is the `BootError`
above and `stages` is never empty. `MB.9.4`'s conformance table asserts `IMPLEMENTATIONS`
covers every builtin kind, so the fallback cannot survive a missing registration after
`MB.9.3`. `MB.9.2.1`/`MB.9.2.2` therefore keep `cargo test -p blastit` green with no
fallback backend.

**Crates by seam, dependencies inward.**

| Crate | Depends on | Contains | Must not |
|---|---|---|---|
| `blastit-backend` (`MB.9.1.1`/`.2`) | `blastit-http` (`BodyFraming`), `blastit-manifest` types (`Executor`, `IngressPolicy`, `StaticPolicy`, `FastCgiPolicy`), `serde`; std `Display`/`OwnedFd` | trait, `RouteId`, `RouteState`, `PinnedSocket`, types, `Registry`, `conformance` | I/O syscalls, tokio socket types, `unsafe`, any implementation, a language or server name |
| `blastit-fastcgi` (`MB.3.4.1`/`.2`) | nothing in production; `[dev-dependencies] blastit-backend` (`MB.3.6.1.2`) for its conformance driver | records, name-value codec, param builder, CGI head parser | sockets, `unsafe`, `tokio` |
| `blastit-scheduler` | `blastit-backend`, `blastit-fastcgi` (pure), `blastit-ingress`, `blastit-frame`, `blastit-manifest` | `StaticUpstream`, `WsgiUpstream`, `FastCgiUpstream`, dispatch, `SessionPort` | a socket type from `blastit-fastcgi`, `blastit` |
| `blastit-view` | `blastit-frame` | the Python side (unchanged) | the contract |
| `blastit` | everything | composition root (`roles::scheduler`), spawn, `dev`/`check`/`--learn` | a per-backend branch in dispatch |

Every leaf that creates a workspace crate or edits a `Cargo.toml` owns `Cargo.lock` in one
strictly serialized dependency chain (`MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` →
`MB.3.6.1.2` → `MB.3.8`, §8): its *first* cargo command runs offline **without** `--locked`
to update `Cargo.lock`, and every subsequent command and `make check` runs **with**
`--locked`.

### 4.3 The static port (`MB.9.2`)

`StaticUpstream` wraps `static_asset.rs` (resolution, `AnswerKind`) and
`static_response.rs` (`StaticResponse`, body pump); `rewrite.rs` stays the pure
precedence module and is called by the impl, not by dispatch.

| Today | After |
|---|---|
| `RouteInfo.static_policy`/`static_forward` read by `scheduler.rs:833,1417,1599,2297` | the builder reads them once; dispatch reads `stages` |
| `begin_raw` → `static_head` → `head_segments` | `StaticUpstream::plan` runs `head_segments` (`MalformedPath` `400`, `Hidden` `404`) plus the `.php`/`.phar` `EntryScript` `404` gate (when `fastcgi` is on the route) and returns `BodyMode::None` — refusing malformed/dotfile/entry paths before admission without rejecting `POST`/bodies on paths that may `Forward` (`Missing`) or `404` (`SpaLost`/`Hidden`), matching `rewrite::decide` (`rewrite.rs:79-131`) and `ADR-0007` decision 5 (`POST /index.php` with a body is `404`, `POST /../x` is `400`, `POST /assets/app.css` is `405`, `GET /assets/app.css` with a body is `413`, `POST /users` on `[static, executor]` forwards) |
| `dispatch_static` resolves + `decide` | `exchange` resolves + `rewrite::decide`; `Serve` writes through the sink; `Reject` is `UpstreamError::refused(reason)` (`Refusal::status()` authoritative); `Forward` is `Declined { next, plan: next.plan(&plan.head)? }` |
| `Scheduler.slots`, `expire_requests`, `client_cancel`, the session registry | **stay in `scheduler.rs`**: the adapter owns `StaticRoot`, `resolve`, `respond` and the `StaticFile`/`Observation`; the scheduler owns admission, the slot and the timeout, and releases a slot **only** on `SessionMsg::Done` (a parked `pread` keeps its slot, exactly 0.2.0). `Request.pumping`, `Scheduler.static_in` and `release_static_slot` are deleted: the session task awaits its own `spawn_blocking` read, so there is no second owner |
| `AnswerKind`, `STATIC_CHUNK_BYTES`, `STATIC_MAX_FILE_BYTES`, `STATIC_ALLOW`, `declares_body`, `http_date` | unchanged |

**Preserved, byte-identical:** refusal statuses (`400` for traversal/absolute/backslash/
control/invalid UTF-8/over-long; `404` for symlink/special/oversize/dotfile/unreadable/
`SpaLost`/miss-without-app), `no-store` on every scheduler-written refusal,
`immutable`/`no-cache` keyed on the answering file, `ETag`/`Last-Modified`/`304`/`HEAD`,
directory-index vs SPA precedence, `405` + exactly `Allow: GET, HEAD`, `413` for a
declared body, `405`/`413` decided from the head before a byte is read, and `Forward`
releasing the slot and re-entering the ordinary queue. The protected suites
`critical_static_assets.rs`, `critical_static_resolve.rs`, `critical_rewrite.rs` and
`critical_ingress_enforcement.rs` pass **unmodified**; that is the port's acceptance.
`critical_rewrite.rs` reads `RouteInfo.static_policy`/`static_forward`, so those fields
stay on the descriptor while every dispatch branch on them is deleted. A protected
contract change is its own node.

**`Forward`.** `StaticUpstream` is built with `next: Option<Arc<dyn Upstream>>`; on a
miss that may forward it returns `Declined { next, plan: next.plan(&plan.head)? }`. The
scheduler verifies identity, releases the head-phase slot, sets `request.stage = 1` and
the new plan, and re-enters `admit_ready` — exactly 0.2.0's `forward_static` order. A
static-only route builds no `next`, so a miss is `404`.

### 4.4 The WSGI port (`MB.9.3`)

`WsgiUpstream` is a thin adapter over the existing view path, in `blastit-scheduler`:

| Method | Behaviour |
|---|---|
| `capacity` | `self.state.capacity()` (`ready && inflight < view_worker_cap`) on the `Arc<RouteState>` passed in `BuildCtx.state` |
| `ensure_ready` | `SessionPort::spawn(self.route)` (the existing view `Ctl::Spawn`); backoff via `tokio::time`, tests use `pause()` |
| `plan` | route `IngressPolicy` → `Buffered { max: BLASTIT_MAX_BODY_BYTES }` / `Stream { credit }` / `None`; `413`/`400` as `plan_body` today |
| `exchange` | `SessionPort::open(self.route)` → `(request_id, channel)`; write the encoded `REQ_HEAD`, relay `REQ_BODY`/`REQ_END`/`CANCEL`/`WINDOW`, map `RESP_HEAD`/`RESP_BODY`/`RESP_END`/`RESP_END_COST` to the sink, lossy-UTF-8 `APP_OUTPUT` lines to `sink.output`, return `WireRequest` |
| `Declined` | never |

`SessionPort`/`ExchangeChannel` are the **WSGI** scheduler capability in the contract (the
pool-start capability is `PoolPort`, §4.2.1). A
`ViewHub` (built from the channels `run_with` creates **before** the registry) **wraps the
existing view state** — holding the per-route `Arc<RouteState>` passed into `BuildCtx`,
the atomic request-id counter and the `dispatch` map — and implements `SessionPort` keyed
by `RouteId`: `open` allocates the id, registers the channel, and the scheduler's
`view_in` arm routes each encoded frame to it. `Scheduler` holds `Arc<ViewHub>`, so there
is no bootstrap cycle and `BuildCtx` carries an owned `Arc<dyn SessionPort>`.
`FastCgiUpstream` uses `PinnedSocket` for data I/O, `PoolPort::ensure_pool(self.route)` for
pool start (when `pool_spawn == PoolSpawn::Managed`; never `SessionPort`), and
`self.state.capacity()` on its `BuildCtx.state`. Preserved exactly:
status, hop-by-hop stripping (plus the shared proxy-control header denial), `Content-Length`/
chunking, body credit, `app.output`, `RESP_END_COST`, the lazy spawn, the queue and the
slot. `critical_ingress_enforcement.rs` and `critical_rewrite.rs` pass unmodified;
`conformance_wsgi` asserts the learn record is identical.

**The session task (all backends).**

| Concern | Pinned |
|---|---|
| Owner | one `tokio` task per admitted request; it owns a `RoutePermit` (from `RouteState::try_acquire()`), a `SessionDoneGuard`, a cloned `http_out: Outbound`, the owned `Box<dyn ResponseSink + Send>`, the `Box<dyn BodySource + Send>` and `Arc<dyn Upstream>` |
| Completion & panic guard | normal exit calls `guard.finish(result)` sending `SessionMsg::Done { id, result }` on the bounded `mpsc::Sender<SessionMsg>` (capacity `1`); if the task unwinds on a panic or drops without `finish`, `Drop for SessionDoneGuard` sends `SessionMsg::Done { id, result: Err(UpstreamError::Fault { detail: "session task panicked" }) }` **exactly once**, so **every** exit path sends `Done` and a panicking task can **never** strand a global `BLASTIT_MAX_WORKERS` slot. The scheduler keeps the matching `mpsc::Receiver` in `self.sessions` (never drops it in dispatch) |
| Credit | the scheduler owns an `Arc<Credit>` per request; `Frame::Window` calls `grant`, `ResponseSink::body` awaits it |
| Body | `Request` holds the `mpsc::Sender<BodyEvent>`; `REQ_BODY`/`REQ_END`/`CANCEL` are forwarded; a full channel is backpressure, never a drop |
| Cancel/timeout/drain | set the `CancelToken` and `try_send(BodyEvent::Cancelled)`; the task closes and reports `Done`; the scheduler never aborts the task (a parked read keeps its slot until `Done`) |
| Events | `Done` carries observations; the scheduler writes `http_obs.jsonl`; `sink.output` forwards lossy-UTF-8 stderr to init |

### 4.5 The FastCGI session

#### 4.5.1 Pure codec vs scheduler client

| Piece | Crate | Contract |
|---|---|---|
| Header encode/decode, type constants, padding, total decoder | `blastit-fastcgi::record` | truncated/over-long is `Err`, never a panic or over-read |
| Name-value encode/decode, 1-byte/<128 and 4-byte forms, terminator | `blastit-fastcgi::param` | a pair never straddles records; a truncated/straddling pair is `RecordError::Truncated` (no separate `ParamError`) |
| Param map → bytes, allowlist, canonicalisation, caps, refusals | `blastit-fastcgi::lockdown` | pure; every §4.7 `lockdown` row is a `Result<_, LockdownError>` |
| CGI `STDOUT` head parse (`Status:`, headers, the four forbidden proxy-control names, default `200`, caps) | `blastit-fastcgi::cgi` (`MB.3.4.1`) | pure state machine; a bad head, a header **name** that is not an RFC 7230 `tchar` token (empty, NUL/CTL, space, `:`, obs-fold), or one of `X-Accel-Redirect`/`X-Sendfile`/`X-Lighttpd-Send-File`/`X-Rewrite-URL` (compared on the token name, case-insensitively) is `Err(CgiError::BadHead &#124; CgiError::NonTokenName &#124; CgiError::ForbiddenProxyHeader)`, never a partial head; any other `X-Accel-*` name is dropped, not an error |
| `END_REQUEST` decode; `appStatus`/`reserved[3]` ignored | `blastit-fastcgi::record` | `protocol_status != Complete` is `Err(RecordError::ProtocolStatus)` |
| `PinnedSocket` connect, write, body pump, read pump, credit, abort, accounting, backoff | `blastit-scheduler::upstream::fastcgi` (`MB.3.6.2.1`) | sockets and time only; takes `PinnedSocket` by construction and never resolves the socket by path |

**Codec-local errors (no `blastit-backend` edge).** `blastit-fastcgi` is zero-dependency in
production, so it never names `Refusal`/`UpstreamError`. It defines

```rust
pub enum LockdownError { Malformed, UnknownParam, ParamsTooLarge, DuplicateParam }
pub enum CgiError { BadHead, NonTokenName, ForbiddenProxyHeader }
pub enum RecordError { Truncated, ProtocolStatus }
```

and `MB.3.6.2.1` maps them at the adapter boundary: `LockdownError::UnknownParam =>
Refusal::UnknownParam` (`500`), `ParamsTooLarge | DuplicateParam => Refusal::ParamsTooLarge`
(`400`), `Malformed => Refusal::MalformedPath` (`400`), and `CgiError::* |
RecordError::* => UpstreamError::Fault { detail: … }` (`502`).

**`FastCgiPaths` lives in `blastit-fastcgi::lockdown`** (std types only, no I/O):
`pub struct FastCgiPaths { pub script_filename: PathBuf, pub document_root: PathBuf,
pub script_name: String }`. `FastCgiBuilder` (`MB.3.6.2.1`) builds it once from
`manifest_dir` + validated relative `root`/`entry` segments after the R2
`openat2(RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS)` check (**never** `std::fs::canonicalize()`,
matching `ADR-0007` decision 3) and passes `&FastCgiPaths` to `lockdown`, which performs no
filesystem access.

#### 4.5.2 Records, ids and lifetimes

| Item | Pinned behaviour | Source |
|---|---|---|
| Connection | one per request, `AF_UNIX` `SOCK_STREAM`, `SOCK_CLOEXEC`; `connect()` exclusively to `/proc/self/fd/<PinnedSocket.parent>/<PinnedSocket.name>` (`FastCgiUpstream` holds `PinnedSocket` from `BuildCtx.socket`; no path string, no name and no mode ever comes from the manifest) | nginx/Apache default (`fastcgi_keep_conn off`) |
| Scheduler socket | in `crates/blastit-sys/src/seccomp.rs`: (1) `SeccompRole::sockets()` allows `Scheduler` (`!matches!(self, Self::Http)`) so `socket(args[0] == AF_UNIX)` is permitted while `AF_INET`/`AF_INET6`/`AF_NETLINK` stay `EPERM` (classic BPF filters `args[0]` only, not `SOCK_STREAM`/`SOCK_CLOEXEC` or `connect` paths — the `0600` socket in pinned `PinnedSocket.parent` is the path wall); (2) `build()` adds an `if matches!(role, SeccompRole::Scheduler)` block denying `SYS_bind` and `SYS_listen` (`EPERM`), keeping `bind`/`listen` allowed for `SeccompRole::View`; (3) unit tests assert `Scheduler` (`AF_UNIX` allowed, `AF_INET`/`bind`/`listen` `EPERM`) and `View { egress: false }` (`AF_UNIX` `bind`/`listen` allowed). Recorded in `ADR-0008` decision 7, `ARCHITECTURE.md` §7.3 and `static-and-spa` §5.1 by `MB.3.8` | C-B1, S5 |
| Socket identity | exact single sequence in §4.8.3: stage 0 (init in production, `stage0::run_dev` in `dev`/`check`) creates and resolves the **derived** per-route pair directory `<run>/fastcgi/<route_index>` (`R3`) from its own run-directory descriptor at boot before any route process exists, opens `parentfd` (`O_RDONLY\|O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC`), and hands it across the stage-0→stage-1 `execve` as the internal argv entry `--fastcgi-parent-fd=<route>:<n>` (§4.8.1) — never an env var, never a path re-open — from which `SchedulerConfig.pinned_sockets` is built and `roles::scheduler` receives it inside `PinnedSocket`. Before each `connect()`, `FastCgiUpstream` `fstat`s `PinnedSocket.parent` (`(st_dev, st_ino) == (parent_dev, parent_ino)`) and `fstatat`s `(PinnedSocket.parent, PinnedSocket.name, AT_SYMLINK_NOFOLLOW)` (`S_ISSOCK`, `st_uid == expected_uid`, `(st_mode & 0o777) == 0o600` in production, and `(st_dev, st_ino) == bound_inode` when `enforce_socket_inode` is set), connects via `/proc/self/fd/<fd>/<name>`, and checks `SO_PEERCRED`: the accepted set is **exactly** `{route_uid:route_gid}` in production (`peer_uid = RouteSpec.uid`, `peer_gid = route_gid` — the **listening** `php-fpm` master, i.e. the socket creator, since `SO_PEERCRED` reports the creator and not the accepting worker; `expected_uid` is the socket owner `scheduler_uid`) and `{invoking_uid:invoking_gid}` in `dev`/`check` (`peer_uid = expected_uid = invoking_uid`); `scheduler_uid` is **not** accepted in production, there is no gVisor tolerance (gVisor reports the listener's creds, which is already `route_uid`), and any other uid/gid → `Fault` (`502`). Only `php-fpm` itself binds the path in its generated pool file's `listen=` line, and that line is **always** the derived path of `R3` — no manifest byte reaches it (safe because `B1` proves `<runtime>` is blastit's own non-route-writable directory with no route-writable ancestor, and `B2` creates and pins `<runtime>/fastcgi/<route_index>` itself; a route uid cannot rename or plant in either, so the pre-`MB.3.9.2` "route owns `manifest_dir`" pivot class ceases to exist); Blastit itself performs **no caller-path-resolved** `chown`/`chmod`/`openat`/`connect` — every operation is on `parentfd` with `AT_SYMLINK_NOFOLLOW`, or the one mode change `chmod("/proc/self/fd/<sockfd>")` on an `O_PATH` fd whose `(st_dev, st_ino)` was just verified (`S5`), which is descriptor-derived and never resolves a manifest path | S-2 |
| Pool readiness | init sends `Ctl::PoolReady { route, sock_dev, sock_ino }` over the private `ctl.chan` socketpair strictly after the socket is `0600` scheduler-owned, `parentfd` is restored to `init_uid:init_gid 0511`, and `(sock_dev, sock_ino)` is re-verified post-freeze; until then `RouteState` (`capacity()`) is `Busy`, so no request ever sees an unfrozen parent or unverified socket | S1 |
| Ownership window | the single `*at(parentfd, …)` sequence of §4.8.3 hands `parentfd` to `(route_uid, route_gid)` `0755` only for the bind, verifies and hands the socket to `scheduler_uid`, and restores `parentfd` to `(init_uid, init_gid)` `0511` on both success and every failure path; another route's uid is never the owner, and no application request runs before `PoolReady` | S1 |
| `BEGIN_REQUEST` | exactly one, `role = RESPONDER (1)`, `flags = 0`, 8-byte body, `reserved[5] = 0` | spec §5.1; research §C |
| `requestId` | `1` on every request; `keepConn = 0` and one request per connection, so the counter never advances. If reuse is ever added: strictly increasing `1..=65535`, wrap to `1` (never `0`), close at exhaustion | research §E, §L |
| `PARAMS` | **one** record when encoded ≤ 65 535 plus a zero-length terminator; a set that would exceed is `Refused { 400, ParamsTooLarge }` before `connect`. One pair ≤ 65 531 including its length prefixes. (The “split between pairs” branch is deleted: unreachable under the total cap and untestable.) | spec §3.4; php-fpm's per-record parser |
| `CONTENT_LENGTH` | known before `PARAMS`: `exchange` drains the `BodySource` to `End` (bounded by `plan.body.max`), then sends `BEGIN_REQUEST`/`PARAMS` with the exact length, then replays the chunks as `STDIN`. `raw` is buffered for this backend only; per-request cap `BLASTIT_MAX_BODY_BYTES`, process aggregate bounded by `BLASTIT_MAX_WORKERS × BLASTIT_MAX_BODY_BYTES` (per route `workers × BLASTIT_MAX_BODY_BYTES`) plus the existing pre-admission half-body queue cap (`BLASTIT_MEM_HIGH_WATERMARK` governs view/pool spawning, not scheduler heap bytes) | I18 |
| `STDIN` | chunks under the upload credit; always one zero-length terminator | spec §5.3 |
| `plan` body mode | path/extension gate runs **first** (`.php`/`.phar` final segment on a FastCGI-only route → `Refused { 404, EntryScript }`); then `None` when the head declares no body; `Buffered { max: BLASTIT_MAX_BODY_BYTES }` when a body arrives on a route with `[route.ingress]` (`json`, `form` or `raw`); a body with no ingress is `Refused { 413, DeclaredBody }`; param count/size preflight runs before connect (`ParamsTooLarge` `400`); `expects_continue = head.expect_continue` | I18, T5 |
| `STDOUT` | stream; first `\r\n\r\n` splits head/body; `Status:` three digits `200..=599` or default `200` (`100..=199` or out-of-range → `Fault` / `502`, never forwarded); `X-Powered-By`, `Server`, `Connection`, `Keep-Alive`, `Transfer-Encoding`, `Upgrade`, `TE`, `Trailer`, `Proxy-*` and **`Content-Length`** always stripped; **exactly four proxy-control names — `X-Accel-Redirect`, `X-Sendfile`, `X-Lighttpd-Send-File`, `X-Rewrite-URL` (ASCII case-insensitive) — are refused as `Fault` (`502` before any head is sent)** in both `blastit-fastcgi::cgi` and `ResponseSink::head` so a pool behind nginx/Apache can never trigger host file sendfile outside `open_basedir`; every other `X-Accel-*` name (`X-Accel-Buffering`, `-Expires`, `-Charset`, `-Limit-Rate`) is **dropped**, never a fault and never forwarded, so a benign `X-Accel-Buffering: no` keeps the response `200` | CGI; SEC MAJOR-6, MINOR-19 |
| Response head caps | ≤ 64 KiB total, ≤ 100 headers, ≤ 8 KiB/value; any control byte `0x00..=0x08`, `0x0a..=0x1f`, `0x7f` (including NUL/CR/LF) in a value, **a header name that is not an RFC 7230 `tchar` token (empty, NUL/CTL, space, `:`, obs-fold)**, a missing colon, obs-fold, duplicate `Content-Length`/`Transfer-Encoding` → `Fault` (`502`) before any head is sent; the four proxy-control names are compared on the token name, case-insensitively; `WireRequest.content_type` is `String::from_utf8_lossy`-converted | M-6, MINOR-35 |
| `STDERR` | each `\n`-terminated line → `String::from_utf8_lossy` (`U+FFFD` for non-UTF-8) → `sink.output(Stderr, line)` → one `Observation::Output` and one `app.output` event through the scheduler's inherited events sink; ≤ 2048 bytes/line **and hard 2048-byte partial-line accumulator** (if 2048 bytes accumulate without `\n`, emit the 2048-byte prefix once, set `discard_to_newline`, increment `dropped`, and discard subsequent bytes in `O(1)` until `\n`/EOF; at clean `END_REQUEST` EOF emit a non-empty non-discarding tail, while on error/cancel/panic drop any partial accumulator without emitting), ≤ 60 lines/request, excess counted in `dropped` | I16 |
| `END_REQUEST` | `appStatus` ignored (hard-coded 0), `reserved[3]` ignored (residue), `protocol_status` non-`Complete` → `Fault` | research §G |
| Truncated stream | EOF/error before `END_REQUEST`: no head → `Fault` (`502`); head sent → `RespAbort { reason: "fastcgi: stream ended before END_REQUEST" }`, no status | M-4 |
| Dead worker | `connect` `ENOENT`/`ECONNREFUSED` before the head flips `RouteState.set_ready(false)`, triggers `ensure_ready` with backoff, and returns `Unavailable { retry_after: Some(1) }` (**`503`** + `Retry-After: 1`, matching `R20`). A lazy pool's first request waits for `PoolReady` (bounded 5 s) and then returns that same `503` + `Retry-After: 1`, never a spurious `502` | M-11 |
| Cancel / timeout | client `CANCEL` or `request_timeout` (`504` before head, `RespAbort` after head) writes `ABORT_REQUEST` (`requestId = 1`) best-effort, closes the socket, returns `Cancelled`; `SessionDoneGuard` guarantees `Done` is sent once; `pm.max_children` and `request_terminate_timeout` bound worker lifetime | M-3 |
| `GET_VALUES` | never sent | research §I |

### 4.6 The param contract

#### 4.6.1 Complete param set

| Param | Source | Class | Rule |
|---|---|---|---|
| `SCRIPT_FILENAME` | `FastCgiPaths.script_filename` (absolute, `openat2`-checked at boot) | **pinned** | never a request byte |
| `SCRIPT_NAME` | `FastCgiPaths.script_name` (`/index.php`) | pinned | |
| `PATH_INFO` | one decoded request path (empty for `/`) | derived | data only; never affects `SCRIPT_FILENAME` |
| `DOCUMENT_ROOT` | `FastCgiPaths.document_root` (`root`, else the entry's parent) | **pinned** | client cannot select a `.user.ini` tree |
| `REQUEST_URI` | request target verbatim (path + `?` + query) | derived | no decode |
| `QUERY_STRING` | bytes after the first `?`, verbatim | derived | **data only**, never argv/INI |
| `REQUEST_METHOD` | head method | derived | required by php-fpm |
| `SERVER_PROTOCOL` | `HTTP/1.1` | pinned | php-fpm never parses it |
| `CONTENT_TYPE` | head (after ingress transcode) | derived | omitted when absent |
| `CONTENT_LENGTH` | exact `STDIN` byte count | derived | |
| `GATEWAY_INTERFACE` | `CGI/1.1` | pinned | |
| `SERVER_SOFTWARE` | `blastit/<version>` | pinned | no build hash |
| `SERVER_NAME` | `head.server_name` | derived | |
| `SERVER_PORT` | `head.server_port` | derived | from the frame, never a header |
| `REMOTE_ADDR` | `head.remote_addr` | derived | the one resolver (`ARCHITECTURE.md` §2.3): peer, or a trusted proxy's `X-Forwarded-For`; a direct client's header is ignored |
| `REMOTE_PORT` | port of `head.peer_addr` | derived | |
| `REQUEST_SCHEME` / `HTTPS` | `head.scheme` | derived | `https` only from TLS or a trusted `X-Forwarded-Proto` |
| `FCGI_ROLE`, `PATH_TRANSLATED`, `REDIRECT_STATUS` | **not emitted** | dropped | php-fpm injects the first; the others are not controls |
| `PHP_VALUE`, `PHP_ADMIN_VALUE`, `auto_prepend_file`, `allow_url_include`, `open_basedir`, `disable_functions` | **not emitted** | dropped | reachable only through `PHP_*_VALUE` |
| `HTTP_PROXY` | **not emitted** | dropped | §4.6.2 |
| `HTTP_*` | folded header name, `-`→`_`, `HTTP_` prefix | derived | names in `HTTP_` only; `Proxy`, `Proxy-Connection` and hop-by-hop names dropped before derivation |

The non-`HTTP_` set is a closed allowlist with no caller-named-param API; a name outside
it is `Refusal::UnknownParam` (**500**, a programmer error, never a client status).
`SERVER_ADDR` is deliberately not emitted: the scheduler has no listening socket and a
header-derived value would be a spoof surface; `SERVER_NAME`/`SERVER_PORT` are what
Symfony/Laravel/Slim use.

#### 4.6.2 Canonicalisation and budgets

| Rule | Value |
|---|---|
| Name alphabet | `A-Z`, `0-9`, `_`; case-folded to upper before the check |
| Unrepresentable header name | a header whose folded name is empty, starts with a digit/`_`, or contains any other byte (`X.Foo`, `-x`, non-ASCII) is **dropped** (`Param { decision: Dropped }`), never a client error; CR/LF/NUL in a header is refused by the front end (`400`) before this point |
| Value bytes | any except NUL/CR/LF; no decode; no INI parse; ≤ 8192. A value **> 8192 bytes** is **dropped** (`Param { decision: Dropped }`), the request proceeds (`200`): it is neither a `400` nor `ParamsTooLarge` (the 128-count / 65 535-total rule below still applies to the remaining set), so a 9 KiB `Cookie` cannot turn a page load into a refusal and cannot silently vanish without a record |
| Header collision | `X-Foo`, `X_Foo` and a second `X-Foo` all fold to `HTTP_X_FOO`: **first wins, later ones dropped**, recorded as `Param { decision: Dropped }`; the request proceeds (`200`). `DuplicateParam` exists only for a builder-programming error (500) |
| Count / total | ≤ 128 params (so ≤ 110 `HTTP_*` headers with a body, ≤ 112 without); encoded ≤ 65 535; one pair ≤ 65 531; over → `Refused { 400, ParamsTooLarge }` before `connect` |
| One decode | path decoded **exactly once** for `PATH_INFO`; `REQUEST_URI`/`QUERY_STRING` never decoded; header names never decoded |
| `httpoxy` | `HTTP_PROXY` is not in the allowlist; `PROXY` is dropped before the prefix; a final name exactly `HTTP_PROXY` is dropped; `HTTP_PROXY`/`http_proxy` are also stripped from generated `env[…]` in production. `Proxy: …` yields no param; a header named `HTTP_PROXY` yields `HTTP_HTTP_PROXY` (data) |
| Re-serialisation | the builder is the only producer; `exchange` re-runs the same deterministic pure function from `plan.head` (no `Any` token), and `conformance` asserts the bytes equal those `plan` validated |

#### 4.6.3 Composition with `[route.static]`

When both blocks are declared, `entry` must be beneath `root` and `DOCUMENT_ROOT` is
`root`; otherwise `DOCUMENT_ROOT` is the entry's parent. `SCRIPT_NAME` is `entry`
relative to that directory. No regex splits the path into script + path-info.

### 4.7 The binary refusal matrix

**R8 single-owner rule:** Every row has **exactly one** authoritative Rust test
(`Owner file::fn`) that asserts its binary outcome; `E` names the supporting
`e2e/php2shell.py` probe (`E1`–`E17`, `E19`–`E25`: 24 client-reachable probes; row 18 is
an internal builder-API `500` guard with no client HTTP path and is marked `—`, excluded
from `PROBES`).

| # | E | Attack (`php2shell` #) | Bytes where | Blastit handling | Outcome | Owner `file::fn` | P |
|---|---|---|---|---|---|---|---|
| 1 | E1 | `SCRIPT_FILENAME` override (1) | path `/app/elsewhere/evil`; header `SCRIPT_FILENAME` | path → `PATH_INFO`/`REQUEST_URI`; param pinned | `200` (`script_filename` == entry) | `critical_fastcgi_lockdown::script_filename_is_pinned_ignoring_path_and_header` | P1 |
| 2 | E2 | `SCRIPT_FILENAME` traversal (2, 5) | `../`, `/%2e%2e/`, `..` after script | static head gate → `400`; param pinned | `400` | `conformance_static::malformed_path` (dispatch-level 400; `critical_static_resolve` asserts `StaticOutcome`, not HTTP codes) | P1 |
| 3 | E3 | `SCRIPT_FILENAME?query` (3) | `?` + entry-like segment in path | pinned value has no `?`; query separate | `200` (`script_filename` has no `?`) | `critical_fastcgi_lockdown::script_filename_never_absorbs_query_string` | P1 |
| 4 | E4 | `PATH_INFO` folded (4) | `/uploads/shell.jpg/x.php` | `.php` under root → `EntryScript` `404` | `404` | `critical_fastcgi_dispatch::php_path_under_root_is_404_entry_script` | P2 |
| 5 | E5 | `PHP_VALUE` (6, 7, 9, 10, 11) | header/query/path naming `PHP_VALUE` | not in allowlist; `-d` is query data | `200` (`allow_url_include` == `"0"`) | `critical_fastcgi_lockdown::php_value_never_emitted_from_any_input` | P3 |
| 6 | E6 | `PHP_ADMIN_VALUE` (8) | header/query/path naming `PHP_ADMIN_*` | not in allowlist | `200` (`allow_url_include` == `"0"`) | `critical_fastcgi_lockdown::php_admin_value_never_emitted_from_any_input` | P3 |
| 7 | E7 | `auto_prepend_file` chain (10) | query + body `<?php …` | no `PHP_VALUE`; pool pins `php_admin_value` | `200` (body is data, canary absent) | `critical_fastcgi_lockdown::auto_prepend_directives_never_emitted` | P4 |
| 8 | E8 | CVE-2012-1823 (12) | `?-d+allow_url_include=1…` | no `php-cgi`/argv; query verbatim | `200` (`allow_url_include` == `"0"`) | `critical_fastcgi_lockdown::query_string_dash_d_stays_verbatim_query_data` | P5 |
| 9 | E9 | CVE-2024-4577 (13) | `?%ADd+allow_url_include%3d1` | one decode for `PATH_INFO`; query verbatim | `200` (`allow_url_include` == `"0"`) | `critical_fastcgi_lockdown::soft_hyphen_probe_stays_verbatim_query_data` | P6 |
| 10 | E10 | CVE-2024-8926 (13) | best-fit/encoding query variants | same control | `200` (`allow_url_include` == `"0"`) | `critical_fastcgi_lockdown::best_fit_encoding_probes_never_become_params` | P6 |
| 11 | E11 | `REDIRECT_STATUS` / CVE-2024-8927 (15) | header `Redirect-Status: 200` / query; `cgi.force_redirect` collision | bare `REDIRECT_STATUS` never emitted; `cgi.force_redirect` untouched | `200` (`"REDIRECT_STATUS"` ∉ `server_keys`) | `critical_fastcgi_lockdown::redirect_status_is_never_emitted` | P9 |
| 12 | E12 | `DOCUMENT_ROOT` spoof (16) | header `Document-Root` / query | `DOCUMENT_ROOT` pinned; `.user.ini` disabled | `200` (`document_root` == pinned) | `critical_fastcgi_lockdown::document_root_is_pinned_ignoring_headers` | P8 |
| 13 | E13 | `Proxy:` → `HTTP_PROXY` (14) | header `Proxy: http://evil` | `Proxy` dropped; `HTTP_PROXY` banned | `200` (`http_proxy` == `false`) | `critical_fastcgi_lockdown::proxy_header_never_produces_http_proxy` | P7 |
| 14 | E14 | duplicate param (17) | `X-Foo: 1` + `X_Foo: 2` / two `X-Foo` | first wins, later `Param { Dropped }` | `200` (first value wins) | `critical_fastcgi_lockdown::colliding_http_headers_keep_first_and_record_drop` | P10 |
| 15 | E15 | NUL in a value (18) | `%00` in path / raw NUL in header | http refuses NUL (`400`) | `400` | `critical_head_refusals::nul_byte_in_header_or_target_is_400` | P10 |
| 16 | E16 | over-long header name | `65 KiB` header name (`> 64 KiB` limit) | http head limit | `431` | `critical_head_refusals::header_name_over_max_header_bytes_is_431` | P10 |
| 17 | E17 | over-budget params (20) | `> 65 535` param bytes, or `> 128` params total — i.e. `> 110` `HTTP_*` headers with a body (18 fixed params incl. `CONTENT_TYPE`/`CONTENT_LENGTH`) and `> 112` without | `Refused { 400, ParamsTooLarge }` before connect | `400` | `critical_fastcgi_lockdown::over_budget_param_count_or_bytes_is_params_too_large` (asserts `LockdownError::ParamsTooLarge`); `conformance_fastcgi::params_too_large` is the 400 oracle | P10 |
| 18 | — | unknown bare param (19) | internal map check with bare `"FOO_BAR"` (no client HTTP path) | closed non-`HTTP_` allowlist → `LockdownError::UnknownParam` | `500` at the adapter (`Err(Refusal::UnknownParam)`) | `critical_fastcgi_lockdown::unknown_bare_param_is_refused_500` (asserts `LockdownError::UnknownParam`) | — |
| 19 | E19 | `.php` source disclosure | `GET /index.php` (and `POST /index.php`) | `Refusal::EntryScript` (`404`) before body gate | `404` (`no-store`, zero source bytes) | `conformance_static::entry_script_refused_404` | P11 |
| 20 | E20 | `.phar`/polyglot (4) | `/shell.jpg/x.php`, `/x.phar` | `.php`/`.phar` gate + `limit_extensions=.php` | `404` | `critical_fastcgi_dispatch::phar_and_polyglot_suffix_are_404` | P2 |
| 21 | E21 | header injection into param | `Cookie: a\r\nPHP_VALUE: …` | http refuses bare CR/LF/injection (`400`) | `400` | `critical_head_refusals::crlf_header_injection_is_400` | P10 |
| 22 | E22 | request smuggling | `CL`+`TE`, duplicate `Content-Length` | http refuses before scheduler (`400`) | `400` | `critical_head_refusals::cl_plus_te_and_duplicate_cl_are_400` | P12 |
| 23 | E23 | `..` with no `[route.static]` | `/../../etc/passwd` on FastCGI-only route | `PATH_INFO` data; `SCRIPT_FILENAME` pinned | `200` (`script_filename` == entry) | `critical_fastcgi_lockdown::dotdot_without_static_keeps_script_filename_pinned` | P1 |
| 24 | E24 | malformed / proxy-control CGI head | no colon/CRLF/1000 headers/`1xx`/non-token header name/`X-Accel-Redirect`/`X-Sendfile` | `Fault` before head → `502` | `502` | `critical_fastcgi_dispatch::malformed_or_proxy_control_cgi_head_is_502` | P12 |
| 25 | E25 | slow-loris / request timeout | declared `Content-Length`, stalled body/peer past `request_timeout` | timeout → `504` before head, `ABORT_REQUEST` sent, close, slot released | `504` + `ABORT_REQUEST` on peer | `critical_fastcgi_dispatch::request_timeout_answers_504_sends_abort_and_releases_slot` | P10 |

Rows 1, 3, 11, 12, 13, 14, 23 also assert via the e2e `/__inspect` fixture (§7.3,
including `server_keys` = `array_keys($_SERVER)`). “`200`” rows assert a non-empty
discriminating key-value subset from `/__inspect` (enforced by `e2e/php2shell.py
--self-test`), so a harness that forwarded the attacker's param fails.

### 4.8 Pool workers

#### 4.8.1 Local and CI: the pool blastit owns

The grammar has **no socket key** (`R1`). The page's Local/CI steps are `MB.3.9.1`'s, and
who starts, supervises and stops the pool in `dev`/`check` — discovery order, cleanup and
the failure contract — is **`MB.3.9.3`**'s amendment (it depends on `MB.12`'s runtime
provider). This section pins only what both nodes build on: the **derivation** of the
socket, the **provenance** of its parent, the **resolution** of `workers`, and the
descriptor handoff that carries the parent from stage 0 to the scheduler.

**The derivation.** In `dev`/`check`, stage 0 (`stage0::run_dev`) resolves the run root it
already owns — `Dev::adopt`'s root (`dev_learn.rs:180`; `0700`, caller-owned, `O_NOFOLLOW`
on the leaf) for plain `dev`, or the adopted `<CheckRoot>/dev` for `check` — and uses the
probe in **two phases**, because the parent must exist before the pool and the leaf can only be checked
after it:

- **phase 1, `SocketProbe::parent(run_root, route_index)`, before the pool starts** — the
  step that makes `php-fpm`'s bind target reachable:
  1. re-prove the run root is the private directory the caller owns (`0700`, no symlink,
     `fstat`/`lstat` inode equality, absolute `/proc/self/fd` readlink) through **one**
     predicate: `MB.3.5.2` moves `verify_private_dir` into `crates/blastit/src/probe.rs` as
     `pub(crate) fn verify_private_dir` and `Dev::adopt` calls it (no duplicate), so the socket's
     neighbourhood is blastit's own state, never the repository;
  2. `mkdirat(run_rootfd, "fastcgi", 0o700)` and `mkdirat(fastcgifd, "<route_index>",
     0o700)`, treating `EEXIST` as success, then
     `openat2(run_rootfd, "fastcgi/<route_index>",
     O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC, RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS)`
     (per-segment `O_NOFOLLOW` fallback on `ENOSYS`), requiring
     `st_uid == invoking_uid`, `(st_mode & 0o777) == 0o700`, `!S_ISLNK` — a planted
     symlink, a foreign owner or a regular file at either derived segment is boot-fatal
     (`FastCgiPoolSocketInvalid`) — and returning the pinned `parentfd`;
- **phase 2, `SocketProbe::check_leaf(parentfd, invoking_uid)`, after the pool starts** —
  `fstatat(parentfd, "fpm.sock", AT_SYMLINK_NOFOLLOW)` must be `S_ISSOCK`,
  `st_uid == invoking_uid`, `(st_mode & 0o777) == 0o600`, and connectable as the invoking
  uid; the failure is boot-fatal
  `./blastit.toml:<line>: route "app" (index 0): pool socket "<run>/fastcgi/0/fpm.sock":
  <rule>`.

`Manifest::check_fastcgi_sockets(run_root, probe)` is phase 1 for every FastCGI route (it
returns the parent fds `check` hands to the child `dev`); **who calls phase 2 and when is
`MB.3.9.3`'s** ordering. Nothing in either phase reads a manifest string: `run_root` comes
from blastit, `<route_index>` from `RouteInfo` order, `fpm.sock` is the constant in `R3`.

**`workers`, resolved once — by the pool leaf.** `resolved(cap) = workers.unwrap_or(cap)`
with `cap = view_worker_cap()` is one formula, resolved **by the leaf that owns the pool
path** (`MB.3.6.3.1`/`MB.3.6.3.2`), used by init when it renders `pm.max_children` and
`RLIMIT_NPROC` and by `run_with` when it builds the route's `Arc<RouteState>`
(`worker_cap = resolved`). It is **not** hosted in `blastit-manifest` and **not**
`MB.3.5.1`'s: the manifest crate stores only `workers: Option<u32>`, and the whole pool
path is frozen pending `MB.12`'s embed-vs-pool decision (see R4 and §8.1 item 4), where
`workers` may lose its meaning entirely. `Manifest::check_fastcgi_workers(cap)` runs at boot
(`stage0`, production and `dev`, and `check::execute`) and refuses an explicit value above
`cap` **before** either consumer runs, so the two call sites cannot disagree and
`unwrap_or` is the whole rule — there is no `min`. `SchedulerConfig` carries the same
`max_workers`/`view_max_workers` from the same stage-1 document, so the scheduler derives
the identical `cap`; one shared pure helper — wherever the pool leaf puts it — is called by
both, never duplicated, which is what makes a missing `.unwrap_or` impossible. The
`model.rs` comment that says "the derivation is the scheduler's" is `MB.3.5.1`'s one-line
follow-up; the design's rule is the sentence above.

**The `parentfd` handoff — one mechanism, production and `dev`/`check` alike.** Stage 0 is
the only process that creates and resolves the parent, and stage 0 does **not** fork stage
1: it `execve`s it, so an `O_CLOEXEC` descriptor is closed at the barrier
(`stage0::exec_stage1_dev`
and `exec_stage1` pass descriptors only through argv — `--config-fd`, `--dbproxy-*-fd`,
`--dev-obs-fd`). The design reuses that exact pattern; it never uses `SCM_RIGHTS` (the
`ctl.chan` socketpair does not exist yet at this `execve` — stage 1 creates it) and never
re-opens the parent by path:

1. Stage 0 opens each `parentfd` `O_CLOEXEC` (so an early failure leaks nothing), pushes one
   internal argv entry `--fastcgi-parent-fd=<route_index>:<n>` per FastCGI route into
   `exec_stage1`/`exec_stage1_dev`, adds `<n>` to the `keep` list, and `set_cloexec(fd,
   false)`s exactly that list (the existing loop, `stage0.rs:1244-1251` and `:1697-1705`).
   It is a user-invisible internal re-exec argument, not a `blastit dev` flag: `cli::parse`
   accepts it only together with `--stage=1` (repeatable, `--fastcgi-parent-fd=<route>:<n>`),
   stores it in `cli::Args::fastcgi_parent_fds`, and `cli::Options` maps it into
   `stage1::Options`; it never appears in `--help` and stage 0 rejects it. `MB.3.6.3.1` owns
   `crates/blastit/src/cli.rs` for this arm (§8, §8.2).
2. Stage 1's `Options` gains `fastcgi_parent_fds: Vec<(usize, RawFd)>`. **Before wrapping any
   descriptor** it validates the list — unique `route_index`, unique fd number, `fd >= 3`,
   `fd` distinct from `config_fd` and every dbproxy fd, and `fstat(fd)` is `S_ISDIR` — and a
   violation is a boot error naming the route/manifest line with **no** descriptor wrapped
   (so a duplicated `(route, fd)` can never be double-owned and double-closed). Only then it
   wraps each entry in `OwnedFd`, re-sets `FD_CLOEXEC` on each **before any fork**, and
   builds one `Arc<OwnedFd>` per route. `PinnedSocket::new(Arc::clone(&parent), …)` consumes
   a clone of that same `Arc` (no `try_unwrap`, no `dup`, no `unsafe`), so
   `SchedulerConfig.pinned_sockets: Vec<(usize, PinnedSocket)>` and the pool supervisor's
   per-route state hold the **same** descriptor.
3. `FastCgiBuilder` requires exactly one descriptor per `[route.fastcgi]` route. A missing
   entry — or one step 2 already rejected — is a boot error naming the manifest line
   (`./blastit.toml:<line>: route "app" (index 0): no pinned pool parent descriptor`),
   never a path fallback.
4. **Ownership for the whole lifetime.** `init` (stage 1) keeps its own `Arc<OwnedFd>` per
   FastCGI route in the pool supervisor's per-route state for the process lifetime: `S1`–`S7`
   and `U1` run on it, and it is **not** dropped after `fork_scheduler`; it closes only when
   `init` exits (`RETIRE` + respawn reuses the same descriptor). The scheduler receives its
   own copy by fork (`fork_scheduler`'s `ChildIo.keep` extended by
   `SchedulerConfig::pinned_socket_fds()`), and `http`/`dbproxy`/`view`/`pool` children drop
   theirs (`ChildIo::prepare`'s `close_fds_except`). **The view/zygote path's `ChildIo.keep`
   must omit `pinned_socket_fds()`** (the monitor/zygote parent keeps its own copy; the view
   gets only its `view.chan`), so an app-controlled view can never `connect` to the pool and
   bypass the param wall. Neither process ever re-opens the parent segment.

**`check`'s external-executor branch** already resolved and validated that parent in its
preflight (`check_fastcgi_sockets` with `DevProbe`, which returns the opened `OwnedFd`), so
`check::serve_and_run` hands the same inode to the child `dev` instead of resolving twice: it
keeps `FD_CLOEXEC` on the descriptor until immediately before the child spawn (nothing else
inherits it), calls `clear_cloexec` right before spawning the child `dev` through
`Host::stage1()` (`program` + `prefix` + `dev --port 8080`, the `ctl_python` pattern,
`check.rs:207-211`), and passes
`BLASTIT_CHECK_POOL_PARENT_FD=<route_index>:<fd>` (`check::POOL_PARENT_FD`, repeatable).
`Config::for_dev()` reads it from `std::env` at the top of `stage0::run_dev` — **before**
`blastit_env::build_from_process`/`prepare_env` strips every `BLASTIT_*` name — stores it in
`Config.check_pool_parent_fds`, and stage 0 immediately re-sets `FD_CLOEXEC` on each so
`fork_dbproxy_dev` cannot inherit it; step 1 then clears it again for the stage-1 `execve`.
**Neither `check_pool_parent_fds` nor `check_sql_obs_fd` is a field of the stage-1
configuration document**: `Config::to_json` (`config.rs:1532`) is a hand-written allowlist
of scalar fields that inserts neither, and `Config` has no `Serialize` derive, so the
omission is structural, not an accident of a derive. The argv entry is the only `parentfd`
handoff and dbproxy is the only `WireSqlObs` writer; `critical_fastcgi_learn::
config_document_carries_no_check_fd` asserts the serialized document has no `check_*` or
`obs_fd` key.
When the variable is absent (plain `blastit dev`), stage 0 resolves the parent itself via
`DevProbe::parent` (which returns the `OwnedFd`); when it is present but an entry is not a
directory or names a route the manifest does not have, `dev` exits `1` naming the route —
never a path connect.

**Negative rule.** After boot no code path calls `open`/`openat`/`openat2` on the parent
segment or on the derived leaf: the only socket operation is
`connect("/proc/self/fd/<parentfd>/<name>")` (`C1`), and
`FastCgiBuilder`/`FastCgiUpstream` never re-resolve a directory. The descriptor that
crossed the barrier is the only way the scheduler can name the parent.

`PinnedSocket` is built with `expected_uid = invoking_uid`, `peer_uid = invoking_uid`,
`peer_gid = invoking_gid` (single-UID simulation: the pool runs as the caller) and
`enforce_socket_inode = false`, so **`dev`/`check` also connect exclusively via
`/proc/self/fd/<parentfd>/<sock_name>`**. `MB.3.9.3` owns whether `dev` keeps that flag at
`false` (today: a hand-restarted `php-fpm`) or flips it to `true` once blastit itself owns
the `dev` pool's lifetime; this amendment does not pre-empt it.

Until `MB.3.9.4`, `stage1` sets `pool_spawn = PoolSpawn::External` for `dev` and for
`check`'s child `dev` (owner of `stage1.rs`: `MB.3.6.3.1`), so the `Managed` default never
sends `Ctl::Spawn` for a pool nobody would start in that window.

`SchedulerConfig` defines `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum PoolSpawn {
Managed, External }` on `pub pool_spawn: PoolSpawn` (default `Managed`). **`External` is the
transitional, pre-`MB.3.9.4` value**: it existed because a human started `php-fpm`, and that
premise is exactly what this amendment removes. After `MB.3.9.4` every mode — production,
`dev` and `check`'s child `dev` — is `Managed`, so `external_executor` is always `false` and
`ensure_ready` always reaches `PoolPort::ensure_pool`; whether the now-unused variant is
deleted is `MB.3.9.4`'s call. `PoolSpawn` still scopes to `Pool` routes only, so WSGI views
keep lazy-spawning via `dev`'s zygote. The name is `PoolSpawn`, not `SpawnMode`, because `crate::config::SpawnMode`
(`Lazy`/`Eager`) already exists. `run_with` copies it into the
contract-owned `BuildCtx.external_executor`; `FastCgiBuilder` stores that flag and
`FastCgiUpstream::ensure_ready` returns `Ok(())` immediately when it is `true` (never
`PoolPort::ensure_pool`) — a mode no shipped configuration may use for a pool after
`MB.3.9.4` — while in `Managed` mode it calls `ctx.pool.ensure_pool(route)`
(one `Ctl::Spawn`) and the first request waits for `PoolReady` (bounded 5 s) under
`request_timeout`; `Pool` routes construct
`RouteState::new(false, policy.resolved_workers(cfg.view_worker_cap()))` after `MB.3.9.4`
(the `External` value was the pre-amendment `true`): `Busy` until the verified
`Ctl::PoolReady { sock_dev, sock_ino }` arrives — in `dev` the equivalent proof is §4.8.1
phase 2 (`MB.3.9.3`) — and then `Ready`; `capacity()` reads only `RouteState` atomics, zero I/O.

#### 4.8.2 Production: one pool per route, as the route's uid

| Question | Decision |
|---|---|
| Who spawns | init, on first request (or at boot with `prefork = true`), from `RouteInfo.executor = Some(Pool(…))` — a typed plan, not a backend string. A new `Ctl::Spawn` handler `supervise::spawn_pool` forks and `execve`s on **init's own `Arc<OwnedFd>`** from the per-route pool state (§4.8.1 step 4; never a re-open); it does not use the Python zygote. The fork goes through **`crate::forking::fork(true)`** — the GIL is held for the whole `Supervisor::run(py)` loop, so `PyOS_BeforeFork`/`PyOS_AfterFork_{Parent,Child}` are the matching halves — exactly as `zygote::fork_view` does, never a raw `libc::fork`; the child then runs `ChildIo::prepare` (after `PyOS_AfterFork_Child` reset the interpreter's locks), `drop_privileges`, `harden_child`, `install_filter`, `execve`, and `child_exit` on failure |
| Command | `php-fpm -F -y <runtime>/views/<uid>/php-fpm.conf` with empty `envp`, forked through the full `child::ChildIo { stdout: Some(pipe), stderr: Some(pipe), keep: vec![], limits: Rlimits { nproc: Some(<resolved> + 2), nofile: Some(max(1024, <resolved>*4 + 32)), core_zero: true }, working_dir: Some(<runtime>/views/<uid>) }` literal so `close_fds_except` leaves **only** fd 0/1/2 (`parentfd`, `ctl.chan`, the events pipes and any `BLASTIT_CHECK_*` observation fd are never inherited), then `setpgid(0, 0)` in the child **and** `setpgid(child_pid, child_pid)` in the parent immediately after the fork (ignoring `EACCES`, which means the child already did it), so `pgid` is known before any `kill(-pgid)`, `drop_privileges(route_uid)` (`route_uid:route_gid`, empty supplementary groups), `harden_child(Signal::KILL)`, `SeccompRole::View { egress }` (`AF_UNIX` `socket`/`bind`/`listen` allowed, no INET unless `egress`), `set_dumpable(false)`, `RLIMIT_NPROC = <resolved> + 2`, `RLIMIT_NOFILE = max(1024, <resolved>*4 + 32)`, `RLIMIT_CORE = 0`, `umask(0o077)`, `chdir(<runtime>/views/<uid>)`. Init signals the whole **process group** (`kill(-pgid, SIGTERM)` then `kill(-pgid, SIGKILL)` on drain; immediate `kill(-pgid, SIGKILL)` on `RETIRE`, crash cleanup or failed spawn). A compromised worker that called `setsid()` to leave `-pgid` is capped by `route_uid`'s `RLIMIT_NPROC = <resolved> + 2` and cannot unlink/rebind/chmod inside `0511` `parentfd` **outside the bind window**. The bind window is the real opening: `S2` makes `parentfd` `route_uid:route_gid 0755`, so a surviving escapee (same uid) can `unlink` + `bind` the leaf before `S4`. That is closed by the **new-pgid membership check** (§4.8.3 `S4`/`S6`): a socket is published only if its inode is held by a task whose `/proc/<pid>/stat` `pgrp` equals the new `master_pid`; an escapee's socket is not, so `S4`/`S6` fail and `U1` runs. Residual after that: an escapee that can inject an fd into, or `ptrace`, a new-pgid task (same-uid, beyond this model) — same-route persistence/DoS only, never cross-route access. Proven by `critical_fastcgi_pool::pool_ready_inode_is_held_by_a_task_in_the_new_pgid` and `critical_fastcgi_pool::setsid_escapee_is_never_published` (`MB.3.6.3.2`) + `e2e/rootless/run.py::check_fastcgi_setsid_escapee_contained` (`MB.3.7`) |
| New role? | **No.** Same uid/gid as the route's view (`UID_BASE+1000+i`), same supervisor, same `process.*`/`app.output` pipes. ADR-0001's five roles and ADR-0007's "no second supervisor" hold; the only role-level change is the scheduler's `socket(AF_UNIX)` with `SYS_bind`/`SYS_listen` `EPERM` (decision 7) |
| Pool file | `<runtime>/views/<uid>/php-fpm.conf`, `0600` route uid (rendered by `supervise::render_pool_conf`, `MB.3.6.3.2`), alongside private temp dir `<runtime>/views/<uid>/tmp` (`0700` route uid): `[global] daemonize=no`, `pid=<dir>/fpm.pid`, `error_log=/dev/stderr`, `[www] listen=/run/blastit/fastcgi/<route_index>/fpm.sock` (**always** the derived path of `R3`; no manifest byte reaches this line), `listen.mode=0600`, `clear_env=yes`, `pm=static`, `pm.max_children=<resolved>`, `pm.max_requests=<pool_max_requests>` = `max(BLASTIT_VIEW_MAX_REQUESTS, 500)` (`supervise::DEFAULT_POOL_MAX_REQUESTS`, defined in `supervise.rs` and owned by `MB.3.6.3.2`; for a pool `0` does **not** mean "never", unlike the view cap, so a future param-wall regression cannot persist for the process lifetime — one worker recycles every `pool_max_requests` requests), `security.limit_extensions=.php`, `catch_workers_output=yes`, `chdir=<manifest_dir>`, `request_terminate_timeout=<BLASTIT_REQUEST_TIMEOUT + 5s>`, `php_admin_value[allow_url_include]=0`, `php_admin_value[auto_prepend_file]=`, `php_admin_value[user_ini.filename]=`, `php_admin_value[cgi.fix_pathinfo]=0`, `php_admin_value[open_basedir]=<manifest_dir>:<runtime>/views/<uid>/tmp`, `env[HOME]=<runtime>/views/<uid>/tmp` (the private `TMPDIR` root, so `$HOME/.cache`/`.config` writes stay inside `open_basedir`; the private dir itself is deliberately **not** added to `open_basedir`, so the app can neither read nor rewrite its own `php-fpm.conf`), `env[TMPDIR]=<runtime>/views/<uid>/tmp`, `env[...]` = sanitized stage-1 environment (minus every `BLASTIT_*` and minus `HTTP_PROXY`/`http_proxy`, with proxy `host=/proc/self/cwd` rewritten to `host=<runtime>/views/<uid>` so `libpq`/`PDO` finds `.s.PGSQL.<port>` after PHP's SAPI `chdir`s to `dirname(SCRIPT_FILENAME)`), INI-escaped. **No** `user`/`group`, **no** `listen.owner`/`listen.group`. `listen.mode=0600` and the §4.8.3 `S5`/`S6` owner discipline are one invariant stated twice (php-fpm's own umask, then init's verified `0600`), and a manifest cannot influence either |
| `workers` vs `pm.max_children` | equal, both the **resolved** value (`policy.resolved_workers(view_worker_cap())`, resolved by the **pool leaf**, gated on `MB.12`), never `Option`-dependent; `RLIMIT_NPROC` and `RLIMIT_NOFILE` are sized from the same number, so one boot-time value governs the pool, the admission permit and both rlimits. The resolved value is also `RouteState.worker_cap` (sessions in flight, including cancelled ones until `Done`), which is why the pool can never be saturated by admitted requests: per-route admission stops at the same number. No boot-time **sum** against `BLASTIT_MAX_WORKERS`: each session takes one global slot, so the runtime budget is the real bound, exactly as `N` WSGI views each get `view_worker_cap`. `RLIMIT_NPROC = <resolved> + 2` is deliberately tighter than a view's `view_worker_cap + 16` (`zygote.rs:52` `VIEW_PROCESS_MARGIN`): a `pm = static` pool is a master plus exactly `<resolved>` children (one spare for the fork-to-`execve` window), while the CPython view needs 16 for its interpreter threads. `pm.max_requests` (per worker) is `max(BLASTIT_VIEW_MAX_REQUESTS, 500)`, so the pool always recycles even when the view counter is `0`; `request_terminate_timeout` bounds hung workers |
| Socket lifecycle | governed exclusively by the normative syscall table in **§4.8.3** (zero variants) |
| Restart / rebind | on master exit or `Ctl::Retire`, init runs **Unwind U1** (`kill(-pgid, SIGKILL)`, `remove_entry(parentfd, sock_name)`, keep `parentfd` `init_uid:init_gid 0511`, emit `Ctl::PoolExited`) and, for crash restart after backoff (not `RETIRE`), runs steps `S0`–`S7` of §4.8.3 from the top |
| Cold start / readiness | `capacity` stays `Busy` until `Ctl::PoolReady { route, sock_dev, sock_ino }` arrives; `ensure_ready` waits up to 5 s for it, then `503` + `Retry-After: 1`. The first request therefore waits or gets a `503`, never a spurious `502`. At boot, init validates `php-fpm -t -y` for each FastCGI route in a forked `route_uid` child before opening any bind window, so a missing `php-fpm` binary or invalid pool config is an immediate **boot error** naming the route |
| `dev` | no uid change, one identity; who spawns/restarts the pool is `MB.3.9.3`, and the socket is the derived `<run_root>/fastcgi/<route_index>/fpm.sock`; `DevProbe` pins `parentfd` into `PinnedSocket` (`enforce_socket_inode = false`, §4.8.1) at boot and hands it over via the §4.8.1 handoff |
| `--learn` | the pool's stderr is attributed to the route by init's pipe label, exactly as a view's |

**`render_pool_conf` — the one public renderer.** It is a pure function of std types (no
`Parts`, no private module type), so the integration test can call it:

```rust
#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct PoolConf<'a> {
    pub manifest_dir: &'a Path,
    pub runtime_views: &'a Path,
    pub uid: u32,
    pub gid: u32,
    pub workers: u32,
    pub max_requests: u32,
    pub request_timeout: Duration,
    /// The derived pool directory's parent (`<runtime>`) and the route index: the renderer
    /// builds `listen=<runtime>/fastcgi/<route_index>/fpm.sock` itself. The fixed suffix is
    /// spelled in four places (`R3`, `B2`, `render_pool_conf`, `check_fastcgi_leaf`), and **no**
    /// caller ever supplies a path — each takes `(runtime|run_root, route_index)`, so drift is a
    /// literal typo in tests, not a value a manifest can reach.
    pub runtime: &'a Path,
    pub route_index: u32,
    pub entry: &'a str,
    pub env: &'a BTreeMap<String, String>,
}
#[doc(hidden)]
pub fn render_pool_conf(spec: &PoolConf<'_>) -> String;
```

Every interpolated value goes through one `write_ini_value` helper that quotes/escapes it
(php-fpm value syntax) — for **keys as well as values** — and refuses either containing
`\n`/`\r`/NUL, so `manifest_dir` in `chdir=`/`open_basedir=` and an `env[NAME]` key cannot inject a
second directive (S-5). It lives in `supervise.rs`
(`MB.3.6.3.2`), which is `pub mod` only so
`blastit::supervise::{render_pool_conf, PoolConf}` are reachable; `Parts`, `Supervisor` and
`Supervisor::new` stay `pub(crate)`. `MB.3.6.3.2`'s acceptance runs
`cargo clippy -p blastit --all-targets --locked -- -D warnings`, so the leaf that adds the
function is the leaf that proves it lint-clean.

#### 4.8.3 Normative pool socket lifecycle, syscall invariants and unwind matrix

`php-fpm` (compiled without `libsystemd`, as in `php:8.4-fpm-alpine`) always `bind(2)`s the
path in its generated pool file's `listen=` line itself (`fpm_sockets.c`), and that line is
**always** the derived `R3` path — `<runtime>/fastcgi/<route_index>/fpm.sock`, a value no
manifest byte can reach. Blastit never calls a
**caller-path-resolved** `chown`, `chmod`, `unlink`, `open` or `connect`: every operation is
on `parentfd` (`*at` with `AT_SYMLINK_NOFOLLOW`), or the one descriptor-derived mode change
`chmod("/proc/self/fd/<sockfd>")` on an `O_PATH` fd whose inode was just verified (`S5`). Safety holds by combining **non-route-writable
ancestors from `/` to `parentfd`** (so `php-fpm`'s own path walk cannot be redirected by any
route uid) with **fd/inode-only operations on `parentfd`** and **post-freeze `(st_dev, st_ino)`
verification**. Because `<runtime>` is blastit's own non-route-writable directory and
`<runtime>/fastcgi/<route_index>` is created by init, the ancestor rule is *satisfied by
construction* rather than checked against an operator-chosen path — that is the whole point
of deriving the socket, and it is why `B2` no longer has to police `manifest_dir`:

| Step | Exact syscall(s) & flags | Caller | Invariant preserved / race closed | Failure action & residual state on disk |
|---|---|---|---|---|
| **B1** Anchor | `fstat` walk from `/` to `<runtime>`; `runtimefd = open(<runtime>, O_RDONLY\|O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC)`; then `fstat(runtimefd)` and `lstat(<runtime>)` must agree on `(st_dev, st_ino)`, `S_ISDIR`, `!S_ISLNK`, and `readlink("/proc/self/fd/<runtimefd>")` must be an absolute path (never a `(deleted)` suffix) | `init` (boot, pre-fork) | `<runtime>` is the tree blastit already creates: root-owned (`0`) `0755`, **not** `0700` — `blastit-sys::sock::make_private_dir`'s `create_parents` mkdirs every missing parent `0755` and the shipped `/run/blastit` must stay `0755` so a `route_uid` can path-walk into its own `views/<uid>` (`sock.rs:452-454,497-519`). B1 therefore requires, in this order: no segment from `/` to `<runtime>` is writable by any route uid/gid (`ARCHITECTURE.md` §5.3's sticky-bit rule); `<runtime>` is **traversable** (`o+x`) by every route uid, because `php-fpm` `bind(2)`s an absolute path beneath it; and `<runtime>` is not itself a route-owned directory. In `dev`/`check` the same predicate runs against `Dev::adopt`'s `0700` caller-owned root (`dev_learn.rs:180`), where owner and user coincide | Boot error naming the offending path, `exit 1`; nothing created |
| **B2** Create & normalise `parentfd` | `mkdirat(runtimefd, "fastcgi", 0o711)` (`EEXIST` ok) → `fastcgifd = openat(runtimefd, "fastcgi", O_RDONLY\|O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC)` → `fchownat(fastcgifd, c"", init_uid, init_gid, AT_EMPTY_PATH)` + `fchmodat(fastcgifd, c".", 0o711, 0)` → `mkdirat(fastcgifd, "<route_index>", 0o700)` (`EEXIST` ok) → `parentfd = openat2(fastcgifd, "<route_index>", O_RDONLY\|O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC, RESOLVE_BENEATH\|RESOLVE_NO_SYMLINKS\|RESOLVE_NO_MAGICLINKS)` (per-segment `openat(…, O_RDONLY\|O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC)` fallback on `ENOSYS`) → **normalise, then verify**: `fchownat(parentfd, c"", init_uid, init_gid, AT_EMPTY_PATH)` + `fchmodat(parentfd, c".", 0o700, 0)`, then `fstat(parentfd)` requires `S_ISDIR`, `!S_ISLNK`, `st_uid == init_uid`, `(st_mode & 0o777) == 0o700`, and `(st_dev, st_ino)` distinct from every other route's `parentfd` | `init` (boot, pre-fork) | The path is a pure function of the route index (`R3`): **no manifest segment exists**, so there is no symlink/magic-link input to traverse and no `manifest_dir`-ownership question. The `fastcgi` container is `init_uid:init_gid 0711`: **traverse-only for everyone else** (so `php-fpm`'s absolute path walk reaches `<i>`), writable by nobody but init (so no route can plant or rename an entry there), and not listable. Normalising before verifying is what makes the cycle **re-entrant**: a second boot over a preserved `<runtime>` finds `<i>` frozen at `0511` by `B3`/`S6` and must be able to bring it back to `0700` (init owns it; `CAP_CHOWN`/`CAP_FOWNER` as in `B3`) instead of failing boot forever. Opened `O_RDONLY\|O_DIRECTORY` (never `O_PATH`, where `fchmod` is `EBADF`) | Boot diagnostic naming the route and the derived path, `exit 1`; zero processes, zero sockets, nothing usable left |
| **B3** Boot freeze & pin | `fchownat(parentfd, c"", init_uid, init_gid, AT_EMPTY_PATH)` → `fchmodat(parentfd, c".", 0o511, 0)`; **type-agnostic stale cleanup**: `fstatat(parentfd, sock_name, AT_SYMLINK_NOFOLLOW)` (`ENOENT` ok; any removable entry — socket, regular file, symlink, **empty directory** — is removed by `remove_entry(parentfd, sock_name)`, below, whose failure semantics are per-route and never `exit 1`); hand `parentfd` to stage 1 as the internal argv entry `--fastcgi-parent-fd=<route>:<n>` (§4.8.1) and share one `Arc<OwnedFd>` per route between `SchedulerConfig.pinned_sockets` and init's per-route pool state (kept for the process lifetime), closed in `http`/`dbproxy`/`view`/`pool` | `init` (boot, pre-fork) | Any `EPERM`/error on `fchownat`/`fchmodat` exits `1` naming `CAP_CHOWN`/`CAP_FOWNER` (never a silent runtime `503`). Locks `parentfd` `init_uid:init_gid 0511` (owner `r-x`, everyone else `--x`: traversable by the scheduler's `/proc/self/fd` lookup, not listable, not writable) and empties it before any untrusted process exists. The descriptor crosses the stage-0→stage-1 `execve` exactly once, as an argv number with `FD_CLOEXEC` cleared for that `execve` and re-set in stage 1 before any fork, so no later `execve` (view, `php-fpm -t`, `php-fpm -F`) inherits it | A `fchownat`/`fchmodat` `EPERM` is a boot error naming `CAP_CHOWN`/`CAP_FOWNER`, `exit 1`. A **leaf that cannot be freed** is **not**: `remove_entry`'s `ENOTEMPTY`/occupied-rename-slot outcome skips this route's pool (a boot **warning** naming the route and the entry; the route stays `Busy` → `503` + `Retry-After: 1`) and `B3` continues with the other routes — one compromised worker never bricks the boot (§4.8.3, "A leaf that cannot be freed never fails the boot") |
| **S0** Config test | Render `<runtime>/views/<uid>/php-fpm.conf` (`0600`) + `tmp/` (`0700`); fork a `route_uid` child running `php-fpm -t -y <conf>` (creates no socket/pidfile) through the §4.8.2 `ChildIo` literal (`keep: vec![]`, only stdio) — `close_fds_except` leaves only fd 0/1/2, so **`ctl.chan`, `parentfd`, the events pipes and every `BLASTIT_CHECK_*` observation fd are closed before the `execve`** — then `drop_privileges(route_uid)`, `harden_child(Signal::KILL)`, `SeccompRole::View { egress }`, `set_dumpable(false)`, exactly S3's sequence | `init` (boot & pre-spawn) | Validates the `php-fpm` binary and pool directives **before** opening the bind window (`parentfd` stays `0511`); the app-controlled binary never holds init's end of `ctl.chan`, so it cannot forge `PoolSpawned`/`PoolReady`/`PoolExited` | Boot error (`exit 1`) at startup, or spawn failure (`parentfd` untouched `init_uid:init_gid 0511`, `Busy` → `503`). Asserted by `critical_fastcgi_pool::config_test_and_pool_child_exec_with_only_stdio`: the fake `php-fpm` helper's first calls are `fcntl(3, F_GETFD)` and `read(3, …)`, both of which must return `EBADF`, and it scans `/proc/self/fd` for any target equal to the pinned parent path or matching `socket:[<ino>]`; the exact `{0,1,2}` set is **not** asserted because a wrapper's own script fd and the scanning child's directory handle make it unobservable |
| **S1** Pre-clean | If a previous pool existed: `kill(-old_pgid, SIGKILL)` then `waitpid(-old_pgid, WNOHANG)` until `ECHILD` (bounded 100 ms; not `ECHILD` → **Unwind U1**, the window never opens). Then `fstat(parentfd)` asserts `(st_dev, st_ino) == (parent_dev, parent_ino)`; `remove_entry(parentfd, sock_name)` (`ENOENT` ok) | `init` (`spawn_pool`) | Confirms pinned directory inode and clears any stale entry before granting write permission; no live task of the old pgid can race the bind window | **Unwind U1** |
| **S2** Bind window | `fchownat(parentfd, c"", route_uid, route_gid, AT_EMPTY_PATH)` → `fchmodat(parentfd, c".", 0o755, 0)` | `init` (`spawn_pool`) | Opens only after `S1` reached `ECHILD`. Grants `wx` on `parentfd` solely to this route's `route_uid:route_gid` (never `o+w`, never another route) while `RouteState.ready == false` (`Busy`), so scheduler never calls `connect()` during `S2`–`S6`. A surviving `setsid()` escapee is the **same uid**, so DAC cannot exclude it from this window: the exclusion is `S4`/`S6`'s pgid-membership check | **Unwind U1** |
| **S3** Spawn & poll | Fork via `crate::forking::fork(true)` (the GIL is held in `Supervisor::run(py)`; same pattern as `zygote::fork_view`), then the child runs the §4.8.2 `ChildIo` literal (`keep: vec![]`; only fd 0/1/2 survive; the pool needs only its own listening socket, so `ctl.chan` and `parentfd` are closed by `close_fds_except`), `setpgid(0,0)` in the child **and** `setpgid(child_pid, child_pid)` in the parent right after the fork (`EACCES` = already set, ignored), `RLIMIT_NPROC=<resolved>+2`, drop caps/uid/gid, `SeccompRole::View { egress }`, `execve` `php-fpm -F -y`; emit `Ctl::PoolSpawned { route, pid }`; non-blocking event-loop poll of `fstatat(parentfd, sock_name, AT_SYMLINK_NOFOLLOW)` + `waitpid(pid, WNOHANG)` up to 5 s | `init` (`spawn_pool`) | Master + workers isolated in `-pgid` with zero caps/secrets and no `ctl.chan`/`parentfd`; the parent-side `setpgid` closes the fork→`setpgid` window in which `kill(-pgid)` would be `ESRCH`; non-blocking poll never stalls PID 1 reaping or other routes | Early exit or 5 s timeout → **Unwind U1** |
| **S4** Verify by inode | `st1 = fstatat(parentfd, sock_name, AT_SYMLINK_NOFOLLOW)`: require `S_ISSOCK(st1.st_mode)`, `st1.st_uid == route_uid`, `st1.st_gid == route_gid`, `st1.st_nlink == 1`; **and** require `(st1.st_dev, st1.st_ino)` to appear as `socket:[<ino>]` in the fd table of at least one task whose `/proc/<pid>/stat` `pgrp` (field 5, parsed after the final `)`) equals `master_pid` (the new pgid; bounded `/proc` scan, no per-request cost); record `(sock_dev, sock_ino) = (st1.st_dev, st1.st_ino)` | `init` (`spawn_pool`) | Rejects planted symlinks, regular files, FIFOs, devices, foreign-uid entries and hardlinked sockets (`st_nlink > 1`) without following symlinks, **and rejects an escapee's rebind**: a socket held only by a `setsid()` process outside the new pgid is never recorded, so it can never be published | **Unwind U1** |
| **S5** Chown/chmod `0600` | `fchownat(parentfd, sock_name, scheduler_uid, scheduler_gid, AT_SYMLINK_NOFOLLOW)` → `fchmodat(parentfd, sock_name, 0o600, AT_SYMLINK_NOFOLLOW)` (on `EOPNOTSUPP`: `sockfd = openat(parentfd, sock_name, O_PATH\|O_NOFOLLOW\|O_CLOEXEC)`, `fstat(sockfd)` must match `(sock_dev, sock_ino)` + `S_ISSOCK`, `chmod("/proc/self/fd/<sockfd>", 0o600)`, `close(sockfd)`) | `init` (`spawn_pool`) | Closes the `S4`→`S5` symlink-swap TOCTOU race while `route_uid` still writes `parentfd`; makes the socket `scheduler_uid:scheduler_gid 0600` so neither `route_uid` nor any other route uid can `connect()` | **Unwind U1** |
| **S6** Re-freeze & re-verify | `fchownat(parentfd, c"", init_uid, init_gid, AT_EMPTY_PATH)` → `fchmodat(parentfd, c".", 0o511, 0)`; `st2 = fstatat(parentfd, sock_name, AT_SYMLINK_NOFOLLOW)` must match `(sock_dev, sock_ino)`, `S_ISSOCK`, `st_uid == scheduler_uid`, `st_gid == scheduler_gid`, `st_nlink == 1`, `(st_mode & 0o777) == 0o600`; **and** re-run `S4`'s pgid-membership check on `(sock_dev, sock_ino)` after the freeze | `init` (`spawn_pool`) | Restoring **both** owner and mode revokes `route_uid`'s `chmod`/`unlink`/`rename`/`creat` rights permanently; post-freeze `st2` closes the `S5`→`S6` replacement race where `route_uid` swapped the socket one instant before the freeze; the re-run membership check closes a swap by an escapee in the same window | **Unwind U1** |
| **S7** Publish | Send `Ctl::PoolReady { route, sock_dev, sock_ino }` (`0x46`) over `ctl.chan`; `scheduler::on_ctl` checks `executor == Pool` and active `PoolSpawned`, stores `(sock_dev, sock_ino)` in `PinnedSocket`, then sets `RouteState.set_ready(true)` | `init` → `scheduler` | Unspoofable: `ctl.chan` is an unnamed `socketpair` held **only** by `init` and `scheduler` — `S0` and `S3` fork through `child::ChildIo`, whose `close_fds_except` leaves only stdio, so no `route_uid` process (including the app-controlled `php-fpm` binary) ever holds an end — and cannot arrive early (emitted only at `S7`, gated on `PoolSpawned`) | Channel close → container teardown (`ARCHITECTURE.md` §8) |
| **U1** Unwind | If forked: `kill(-pgid, SIGKILL)` + reap (`SIGTERM`→grace→`SIGKILL` on `DRAIN`); **then freeze first** — `fchownat(parentfd, c"", init_uid, init_gid, AT_EMPTY_PATH)` + `fchmodat(parentfd, c".", 0o511, 0)`; **then** `remove_entry(parentfd, sock_name)` (`ENOENT` ok; in `0511` the owner's `w` is gone, so the unlink/rename succeeds **only** because init holds `CAP_DAC_OVERRIDE`, the capability the boot freeze already requires); emit `Ctl::PoolExited` (never `PoolReady`) | `init` | Leaves **nothing usable**: `init_uid:init_gid 0511` directory whose leaf name is free (an inert `stale.<pid>` from a rename-aside may remain, and that is fine — nothing connects to it), zero connectable socket, `RouteState` `Busy` (`503` + `Retry-After: 1`). Freeze-before-unlink is deliberate: unlink-then-freeze leaves a `0755 route_uid` window in which a `setsid()` escapee could `bind` again. A failed group kill (`ESRCH`/`EPERM`) **never skips** the rest; an escapee keeps at most an unlinked socket inode and cannot rebind in `0511` | If `fchownat`/`fchmodat` itself fails on unwind, init exits non-zero (fail-fast container abort) |
| **C1** Connect | `fstat(parent)` == `(parent_dev, parent_ino)`; `fstatat(parent, name, AT_SYMLINK_NOFOLLOW)` == `S_ISSOCK`, `expected_uid`, `0600` (prod), and `bound_inode` (`(sock_dev, sock_ino)` when `enforce_socket_inode`); `connect("/proc/self/fd/<parent>/<name>")`; verify `SO_PEERCRED` against the exact accepted set — production `{route_uid:route_gid}` (`peer_uid = RouteSpec.uid`, `peer_gid = route_gid`; the listening `php-fpm` master is the socket creator, and `SO_PEERCRED` reports the creator), `dev`/`check` `{invoking_uid:invoking_gid}`; `scheduler_uid` is rejected in production and there is no gVisor tolerance | `scheduler` (`FastCgiUpstream`) | Proves directory inode, socket inode, DAC owner/mode and peer credentials before sending `BEGIN_REQUEST`; `parent` is the descriptor stage 0 handed over as `--fastcgi-parent-fd` (§4.8.1) and is never re-opened; `SeccompRole::Scheduler` forbids `AF_INET*` and `bind`/`listen` | Pre-connect `ENOENT`/`ECONNREFUSED` → `Unavailable` (`503` + respawn); inode/owner/`SO_PEERCRED` mismatch → `Fault` (`502`); foreign-uid peer covered by `critical_fastcgi_dispatch::foreign_uid_peer_is_fault_502` (`MB.3.6.2.2`) |

**The three modes, and who must traverse what.** Every process that reaches
`<runtime>/fastcgi/<i>/fpm.sock` does so by a different right, and one mode cannot serve all
three:

| Path | Steady owner:mode | Why |
|---|---|---|
| `<runtime>` (`/run/blastit`) | `0:0 0755` (the shipped tree) | `php-fpm`'s **absolute** path walk needs `x` from every ancestor of `fpm.sock`, including `<runtime>`; `blastit-sys::sock::make_private_dir`'s `create_parents` already mkdirs it `0755` and keeps it out of any route's hands (`sock.rs:452-454,497-519`). It is not writable by any route uid, so the walk cannot be redirected — that is the invariant, not `0700` |
| `<runtime>/fastcgi` | `init_uid:init_gid 0711` | traversable by `php-fpm` (needed) and by the scheduler (not needed — it holds `<i>` directly), writable by **nobody but init** (so no route can plant `fastcgi2` or rename `<i>`), not listable |
| `<runtime>/fastcgi/<i>` | `init_uid:init_gid 0511` | `x` for the scheduler's `/proc/self/fd/<parentfd>/fpm.sock` lookup, no `r` (no `readdir`: route count/layout is not public), no `w` outside the bind window. Created `0700`, `0755 route_uid:route_gid` only during `S2`–`S6`, restored `0511` by `S6`/`U1` |

`dev`/`check` run a **single-uid variant** with four named deltas from the production sequence,
because every identity step is a no-op for a pool that already runs as the caller (and some would
fail: `drop_privileges(euid)` calls `setgroups(0, NULL)`, which an unprivileged uid cannot do —
`privs.rs:418-438`):

| Step | Production | `dev`/`check` variant |
|---|---|---|
| `S0` / `S3` | fork a `route_uid` child, `drop_privileges(route_uid)`, then execute | fork as the invoking uid and **skip `drop_privileges`**; `harden_child`, `install_filter`, `set_dumpable(false)` still run |
| `S2` / `S5` / `S6` | chown the parent to `route_uid`, chown the socket to the scheduler, freeze `init 0511` | **skipped**: the pool binds as the uid that already owns `<run_root>/fastcgi/<i>` (`0700`), the socket is `0600` that uid because the generated pool file still sets `listen.mode=0600` (§4.8.2), and nothing is frozen |
| `S1`, `S4`, `S7` | as written | as written (`S4`'s pgid check still runs) |
| `U1` | kill, **freeze `0511`**, remove the leaf | kill, **no freeze**, `remove_entry` — it runs as the **owner** with `w` on the directory |

`DevProbe`'s predicate is therefore exactly `0700` + invoking-uid owner; a `0511` freeze in
`dev` would both fail that predicate and make the retire path `EACCES`. The `0511` steady mode,
the `0711` container, `S2`/`S5`/`S6` and the `drop_privileges` call are **production-only**.

`remove_entry(parentfd, name)` is the one removal helper used by `B3`, `S1` and `U1` (owned by
`MB.3.6.3.1`, with the unprivileged unit `planted_entry_at_the_leaf_is_removed`: socket,
symlink, regular file, empty directory, **non-empty directory**, `ENOENT`): it tries
`unlinkat(parentfd, name, 0)`; on `EISDIR` retries with `AT_REMOVEDIR`; on `ENOTEMPTY` — an
escapee that planted a directory *with a child* inside it during the `S2` window — it does
**not** recurse: it atomically renames the entry aside
(`renameat(parentfd, name, parentfd, "stale.<pid>")`, a single `renameat` on `parentfd`, no
traversal of the stale tree), trying `stale.<pid>.1` … `stale.<pid>.7` when the destination is
occupied, which frees the leaf name for the fresh bind and leaves an inert entry nothing can
connect to; `ENOENT` is success.

**A leaf that cannot be freed never fails the boot.** If all eight rename slots are occupied the
leaf name is still taken, so that **route's pool cannot bind** — and the outcome is exactly the
existing one for a pool that cannot start: `U1` runs, `Ctl::PoolExited` is emitted, the route
stays `Busy` (`503` + `Retry-After: 1`), and a boot **warning** names the route and the entry.
`B3` continues with the other routes and the process serves. One compromised worker must never be
able to brick every other route on a preserved `<runtime>` — that is the whole rule, and its oracle
is `eight_planted_stale_entries_leave_only_that_route_503` (a temp `parentfd` with eight
`stale.*` entries plus a `fpm.sock`: `remove_entry` frees nothing, the **only** outcome is a
warning, that route's `PoolExited`/`Busy`/`503` + `Retry-After: 1`, and every other route still
boots and serves; no root needed); empty `stale.*` entries are removed
opportunistically (`AT_REMOVEDIR`, `ENOTEMPTY` ignored) and a non-empty one is left inert with no
ceiling that can turn into an `exit 1`. It is what makes a hostile leftover at the leaf a *cleaned* boot
rather than a wedged one: without it, `unlinkat(…, 0)` on a planted directory returns
`EISDIR`, `B3`'s old "else error" exits `1`, and — with a preserved `<runtime>` — every
route fails boot behind one route's compromised worker.

**Dev/check ordering (this document) vs who starts the pool (`MB.3.9.3`).** The parent must
exist before `php-fpm` binds and the leaf can only be checked after it has bound, so the probe
is two-phased (`parent` then `check_leaf`, §4.8.1) and `dev`/`check` run phase 1 before the
pool and phase 2 after it. `check`'s own preflight needs no pool: it runs phase 1, creates
`<CheckRoot>/dev/fastcgi/<i>` and passes those directory fds to its child `dev`
(`BLASTIT_CHECK_POOL_PARENT_FD`, unchanged) — one resolution, no circular handoff. **Who
starts the pool in `dev` and in `check`'s child, and where phase 2 is invoked, is
`MB.3.9.3`'s** amendment.

**What this amendment moved, and what it did not.** Only the **provenance** of `parentfd`
changed: `B1`/`B2` now anchor on blastit's runtime directory and create the per-route
directory themselves, instead of resolving a manifest-relative parent from `manifest_dirfd`.
`S0`–`S7`, `U1` and `C1` are unchanged — same freeze, same bind window, same pgid-membership
check, same `0600` chown to the scheduler, same `PoolReady` inode, same connect-time
verification — because they were already written against `parentfd` and a leaf name, never
against a manifest path. That is why deleting the key costs no socket invariant, and why the
`fastcgi` container is the only genuinely new directory the amendment introduces.

**Integration-test seam (`MB.3.6.3.2`).** `Supervisor::run` returns `!`, `on_ctl`/`spawn_pool`
are private and `Parts` holds private types (`SignalFd`/`SignalMask`), so the protected pool
test needs an explicit seam: `supervise.rs` carries an inline `#[doc(hidden)] pub mod pool_test`
exposing

```rust
pub struct PoolReadyInfo { pub sock_dev: u64, pub sock_ino: u64 }
pub struct PoolHarness { /* private */ }
impl PoolHarness {
    pub fn new(manifest_dir: &Path, route: usize, workers: u32) -> std::io::Result<Self>;
    pub fn spawn(&mut self) -> std::io::Result<PoolReadyInfo>;   // S1, S2, S4-S7
    pub fn retire(&mut self) -> std::io::Result<()>;         // U1
    pub fn assert_ready(&self) -> (u64, u64);                // recorded (sock_dev, sock_ino)
}
```

It runs the **unprivileged subset** (`S1`, `S2` as `euid`, `S4`–`S7`, `U1`) against a
hand-rolled temporary directory (no `tempfile` crate) with a `UnixListener` fake pool and a
forked fake child in a new pgid — no `php-fpm`, no root. The root-only `chown`/freeze path
stays in `e2e/rootless`. `crates/blastit/src/lib.rs` stays owned by `MB.3.5.2`
(`pub mod supervise;` only); `MB.3.6.3.2` adds the inline module inside `supervise.rs` and
edits no other file for it.

**Pool ctl messages** (all in `blastit-frame/src/{ctl,frame}.rs`, **`MB.3.6.1.1`**):
`Ctl::Spawn { route }` (`0x40`) starts a pool when the route's executor is `Pool`;
`Ctl::PoolSpawned { route, pid }` (`TypeCode = 0x45`, **never** carries an `SCM_RIGHTS`
`view.chan` fd, unlike `Ctl::Spawned = 0x41` which keeps its fd),
`Ctl::PoolReady { route: u32, sock_dev: u64, sock_ino: u64 }` (`0x46`) strictly at step
`S7` after the freeze, `Ctl::PoolExited { route, pid, status }` (`0x47`), and the existing
`Ctl::Retire { route, pid, reason }` (`0x42`) for request-count/idle retire. The
scheduler's `on_ctl` accepts the pool messages only for a route whose executor is `Pool`
(and accepts `PoolReady` only while a `PoolSpawned` pid is active) and delivers
`signal(ExecutorReady { socket_inode: Some((sock_dev, sock_ino)) })` /
`signal(ExecutorExited)`; a pool spawn error/exit clears `bound_inode`, calls
`RouteState.set_ready(false)` and starts the same backoff as a view.

### 4.9 Static coordination

| Path | Route has `[route.static]` | Route has no `[route.static]` |
|---|---|---|
| a file under `root` that is not `.php`/`.phar` | shipped responder (`200`/`304`/`405`/`413`) | no static stage: the path reaches the front controller |
| a `.php`/`.phar` path under `root` (entry included) | `Refusal::EntryScript` → `404`, final, never forwarded | the FastCGI stage returns `404` for a final segment `.php`/`.phar` |
| any other path | miss → `Declined` → front controller | front controller directly |
| `spa = true` + fastcgi | boot-fatal (R5) | — |
| dotfile / `..` | 0.2.0 refusals unchanged | path data only |

The entry is never served: the static stage’s resolver wrapper returns `Refused { 404,
EntryScript }` for a decoded path whose final segment ends in `.php`/`.phar`
(case-insensitive) **only when the route declares `[route.fastcgi]`**, before the
`Content-Type` table and before method/body checks; the FastCGI-only route refuses the
same path in `plan` rather than passing it to the worker. This is an explicit exception to
ADR-0007 decision 4, recorded in ADR-0008 decision 5: a miss forwards to the **route's
executor** (its `[route.fastcgi]` front controller when declared, otherwise the manifest's
`app`), never unconditionally to "the manifest's `app`". A plain `[route.static]` route is
untouched; `critical_static_*` stay green.

### 4.10 `--learn` and `check` attribution

#### 4.10.1 One file, two line shapes, one write path

`http_obs.jsonl` carries two JSON shapes, and `check` learns which is which without a
schema bump:

| Fact | Line | Produced by |
|---|---|---|
| an executor (WSGI/FastCGI) answered or was refused with a body proof | `Observation { "kind": "wire_request", "target": { "scope": "routed", "route": … }, "method", "path" (no query), "status", "body_len", "content_type", "executor_ran" }` | the session’s `Done` (`executor_ran: true`), or the scheduler’s pre-executor head `413` (`executor_ran: false`) |
| a static file was answered | `Observation { "kind": "static_file", … "path", "status", "bytes" }` | `Observation::StaticFile` |
| the Python gate’s legacy line (WSGI) | `HttpObsLine` (unchanged) | the existing view path until the Python-owning node retires it |

A line with a top-level `"kind"` is an `Observation`; otherwise it is an `HttpObsLine`.
Only `wire_request` and `static_file` facts are written to this file (`Param`/`Output`
observations stay in memory/events); `requests` counts both. **`HttpObsLine` is not
changed** (its struct literals are in a protected suite); the reader in `check` normalises
both shapes into one internal `WireRecord`, and `CheckObsV1` gains a private
`upstream: Vec<Observation>` field plus `new_with_upstream(schema_version: u64, http: Vec<HttpObsLine>, upstream: Vec<Observation>, sql: Vec<WireSqlObs>)` and `upstream()`;
`CheckObsV1::new(...)` keeps its signature and zero upstream records, so the protected
constructors compile. `sample` continues to come from the ingress gate, never from an
upstream. The file is `0600` in the `0700` runtime dir, and `WireRequest.path` is the
routed path with the query removed (sentinel test). **Both** `NoObservations` and the
`if observations.http().is_empty()` early-return guards in `audit_v1::unexercised_routes`
(`crates/blastit/src/audit_v1.rs:950`) and `audit_v1::unrequested_static_roots`
(`audit_v1.rs:989`) are updated to check the **combined** set
(`observations.http().is_empty() && observations.upstream().is_empty()`), so three
`static_file` lines with zero `HttpObsLine`s do not trigger `NoObservations` and **do** run
`unexercised_routes` to emit `unexercised_route`.

The scheduler writes one `wire_request` line (`body_len > 0`, `executor_ran: false`) for a
request it refuses at the head with a body proof (`413` `DeclaredBody` on a route with no
`[route.ingress]`), matching `docs/ci.md`'s existing WSGI `upload` rule so `missing_ingress`
fires while `routes.exercised` stays `0` unless a request actually ran the front controller
(`executor_ran: true`, matching `docs/frameworks/php.md:127`). One route has one producer:
a WSGI route keeps its Python `HttpObsLine` (the scheduler writes no duplicate) until a
Python-owning follow-up retires it; static and FastCGI routes write the scheduler shapes.
`HttpObsLine` lines are treated as `wire_request` (`executor_ran: true`) facts for the
route's executor.

#### 4.10.2 `--learn`

The writer is observation-driven, not language-driven:

1. a route declared no `[route.static]`, every observed request for it is a `404` whose
   path exists as a regular file under exactly one of `public/`, `static/`, `dist/` (or
   the declared `root`), and no such path ends in `.php` → propose
   `[route.static] root = "<dir>"`;
2. unexercised routes and unrouted requests keep 0.2.0's proposals;
3. SQL keeps dbproxy's `WireSqlObs` and `ObsTarget`.

`Observation::Param` records names and `Emitted`/`Dropped` decisions (never values) so a
framework's missing-header complaint has a diagnostic. No PHP-specific line.

#### 4.10.3 `check`

| Situation | Output | Exit |
|---|---|---|
| `manifest.is_all_static()` (checked first) | unchanged (0.2.0 rule), no finding (`--learn` exits `2` `nothing to learn: every route is static`) | `0` |
| `config.app.is_none() && manifest.can_boot_without_app()`, not all-static, with `-- <suite>` | `check::serve_and_run` (`MB.3.6.4.2`) creates `<CheckRoot>/dev` (`0700`), `<CheckRoot>/http_obs.jsonl` (`0600`) and `socketpair` `(obs_read, obs_write)`, builds `prepared` against `<CheckRoot>/dev/views/0`, spawns `Host::stage1()`'s `Reexec` (`program` + `prefix`: `/proc/self/exe` for a standalone binary, `<python> -I -m blastit` for a wheel) followed by `dev --port 8080` with `BLASTIT_CHECK_DEV_ROOT=<CheckRoot>/dev` + `BLASTIT_CHECK_HTTP_OBS=<CheckRoot>/http_obs.jsonl` + `BLASTIT_CHECK_SQL_OBS_FD=<obs_write_fd>` + `BLASTIT_CHECK_POOL_PARENT_FD=<route_index>:<fd>` (`clear_cloexec` immediately before the spawn; stage 0 restores `FD_CLOEXEC`, adopts `<CheckRoot>/dev`, binds all declared proxies at `slot_offset = 0`, and closes `obs_write_fd` immediately after `fork_dbproxy_dev`; stdout → stderr under `--json`), waits for `127.0.0.1:8080`, runs `<suite>` with `BLASTIT_URL=http://127.0.0.1:8080`, cwd = manifest dir, `env_clear` + `prepared`; terminates child `dev`, joins `ObsReader` (`WireSqlObs`) + reads `http_obs.jsonl` into `CheckObsV1::new_with_upstream` and **starts no dbproxy**; a busy port or spawn failure is `2`, any non-zero suite exit is **`3`** (`docs/ci.md`, `check.rs::SUITE_FAILURE`) | `0` clean / `1` findings / `3` suite failed |
| `config.app.is_none() && manifest.can_boot_without_app()`, not all-static, **no** `-- <suite>` | `route "<first_fastcgi_route>" runs an external executor: blastit check -- <suite>` | `2` |
| everything else (`config.app.is_some()`, or `config.app.is_none() && !manifest.can_boot_without_app()`) | the existing pytest path (`--` forwarded verbatim to `pytest`), keeping `docs/ci.md` (`blastit check -- -k products`) and `critical_static_cli` app-less MIXED (`"no interpreter to run pytest on"`) unchanged | unchanged |
| a route with an executor (`executor.is_some()`) and zero `wire_request` lines with `executor_ran == true` | `unexercised_route`, `severity: error`, `fix: {action: "add_test"}` | `1` |
| a static-only / `spa = true` route (`executor.is_none()`) with no `static_file` line | warning on stderr (0.2.0 rule), no finding | `0` |
| body route with no `[route.ingress]` | `missing_ingress`, `fix: {action: "declare_ingress", codecs: […], allow_raw: …}` | `1` |

`--json` is the existing document; no new shape:

```json
{"schema_version":1,"ok":false,"requests":3,"statements":0,"routes":{"total":1,"exercised":0},"proxies":{},"findings":[{"kind":"unexercised_route","severity":"error","route":"app","fix":{"action":"add_test"}}]}
```

`requests` counts every line (assets and pre-executor `413` lines included); `exercised`
counts routes with an executor that have ≥ 1 `Routed` `wire_request` line with
`executor_ran == true`. Current `audit_v1` counts every line into `requests_per_route` and
skips any `route.static_policy().is_some()`, so it changes in two places: (1) replace
`if observations.http().is_empty()` in `unexercised_routes` and `unrequested_static_roots`
with `if observations.http().is_empty() && observations.upstream().is_empty()`; (2) compute
`executor_requests_per_route` (`executor_ran == true`) for `unexercised_routes` and
`routes.exercised`, reporting any route with `executor.is_some()` and zero executor runs
while skipping `executor.is_none()`. The protected waiver test in
`critical_static_cli.rs:699-739` uses a `spa = true` static route (`executor: None`), so
its waiver is preserved unchanged, while the page's `[route.static]` + `[route.fastcgi]`
three-asset fence reads `requests: 3, exercised: 0` and emits `unexercised_route`. There
is no `Php`/`FastCgi` finding kind or branch.

### 4.11 Conformance (the anti-drift gate)

`crates/blastit-backend/src/conformance.rs` backs `pub fn cases() -> &'static [Case]`
with `static CASES: std::sync::LazyLock<Vec<Case>>` (std on MSRV 1.88, so `Observation`
values with `String` fields require no duplicate borrowed enum):

```rust
pub struct Case {
    pub id: &'static str,
    pub class: Class,                  // Refusal | BodyCap | Observation | ErrorMapping
    pub capability: Capability,        // exact bits this case requires
    pub fixture: RouteFixture,         // the manifest + ingress shape the driver builds
    pub inject: Inject,                // how the fixture drives the case
    pub head: HeadSpec,                // method/target/headers, expect_continue
    pub expect: Expect,
}
pub struct Expect {
    pub outcome: OutcomeKind,          // Answered | Refused | Declined | Unavailable | Fault | Cancelled
    pub status: Option<u16>,
    pub refusal: Option<Refusal>,      // the exact contract `Refusal` variant (`Copy + Eq`)
    pub head_written: bool,
    pub body_mode: Option<BodyMode>,
    pub expects_continue: Option<bool>, // `Some(b)` asserts `UpstreamPlan.expects_continue == b`
    pub dropped: Option<u32>,          // `Some(n)` asserts `ExchangeOutcome::Answered.dropped == n`
    pub next_stage: Option<u8>,
    pub observations: Vec<Observation>,
}
pub enum Inject {                      // data, never a per-implementation branch
    Request,
    Head(&'static [(&'static [u8], &'static [u8])]),
    Body(&'static [u8]),
    Peer(PeerScript),                  // a scripted peer response (status line, chunks, EOF)
    CancelAt(CancelAt),
}
pub enum RouteFixture {
    FrontController { ingress: Option<IngressPolicy> },
    StaticOnly,
    Plain { ingress: Option<IngressPolicy> },
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capability(u32);
impl Capability {
    pub const DECLINES: Self = Self(1 << 0);
    pub const STATIC_FILES: Self = Self(1 << 1);
    pub const PARAMS: Self = Self(1 << 2);
    pub const STREAMS_BODY: Self = Self(1 << 3);
    pub const EXECUTOR: Self = Self(1 << 4);
    pub const ALL: Self = Self((1 << 5) - 1);
    pub const fn union(self, other: Self) -> Self { Self(self.0 | other.0) }
    pub const fn contains(self, other: Self) -> bool { self.0 & other.0 == other.0 }
}
impl std::ops::BitOr for Capability {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self { self.union(rhs) }
}
pub struct HeadSpec {
    pub method: &'static [u8],                              // e.g. b"GET"
    pub target: &'static [u8],                              // e.g. b"/users/42?x=1"
    pub headers: &'static [(&'static [u8], &'static [u8])], // name, value
    pub expect_continue: bool,
}
pub enum OutcomeKind { Answered, Refused, Declined, Unavailable, Fault, Cancelled }
pub struct PeerScript {                                    // a scripted FastCGI peer
    pub status_line: &'static [u8],                        // e.g. b"Status: 200 OK\r\n"
    pub headers: &'static [&'static [u8]],                 // raw header lines incl. CRLF
    pub chunks: &'static [&'static [u8]],                  // STDOUT chunks after the head
    pub eof: bool,                                         // close without END_REQUEST when false
}
pub enum CancelAt { BeforeHead, MidBody, AfterHead }
```

`Capability` is hand-rolled (no `bitflags` dependency): `union`, `contains`, `BitOr`, `ALL`,
all `const`. `HeadSpec`/`OutcomeKind`/`PeerScript`/`CancelAt` are the five types the four
drivers share; nothing else in `Case`/`Expect` is implementation-specific.

`blastit_backend::conformance` exports **one** shared
implementation registry beside `cases()`:

```rust
pub const IMPLEMENTATIONS: &[(&str, Capability)] = &[
    ("static", Capability::DECLINES.union(Capability::STATIC_FILES)),
    ("wsgi", Capability::STREAMS_BODY.union(Capability::EXECUTOR)),
    ("fastcgi-codec", Capability::PARAMS),
    ("fastcgi-session", Capability::PARAMS.union(Capability::EXECUTOR)),
    ("fake", Capability::ALL),
];
pub fn implementation(kind: &str) -> Option<Capability>;
```

**Anti-under-declaration & missing-binary gates:**
1. `conformance_table.rs` (`MB.9.4`) asserts the **exact whole `IMPLEMENTATIONS` slice** and the **exact `cases()` id set** (a deleted case cannot shrink every driver consistently)
   (`assert_eq!(IMPLEMENTATIONS, &[("static", DECLINES | STATIC_FILES), ("wsgi",
   STREAMS_BODY | EXECUTOR), ("fastcgi-codec", PARAMS), ("fastcgi-session", PARAMS |
   EXECUTOR), ("fake", ALL)])`), so dropping `EXECUTOR` from `"wsgi"` or `PARAMS` from
   `"fastcgi-session"` fails `conformance_table.rs` immediately instead of shrinking the
   driver's expected count.
2. Each driver passes **only** its `kind` string and asserts
   `ran == cases().iter().filter(|c| implementation(kind).unwrap().contains(c.capability)).count()`
   with **zero** `Skipped` results among those cases.
3. `conformance_table.rs` also asserts per-class `Expect` completeness (`Refusal` ⇒
   `refusal.is_some() && status == refusal.map(Refusal::status) && !head_written`;
   `BodyCap` ⇒ `body_mode.is_some() || refusal.is_some()`; `Observation` ⇒
   `!observations.is_empty() || dropped.is_some()`; `ErrorMapping` ⇒
   `(matches!(outcome, Unavailable) && status == Some(503)) || (matches!(outcome, Fault)
   && ((!head_written && status == Some(502)) || (head_written && status.is_none()))) ||
   (matches!(outcome, Refused) && status == refusal.map(Refusal::status)) ||
   (matches!(outcome, Cancelled | Declined) && status.is_none())`), plus every case
   covered by ≥ 1 non-`"fake"` implementation and every `Class` non-empty.
4. `Makefile`'s `rs-test-conformance` target invokes **each driver binary explicitly by
   `--test <binary>` name** as it lands (`MB.9.4` adds `-p blastit-backend --test
   conformance_table` and `-p blastit-scheduler --test conformance_static --test
   conformance_wsgi`; `MB.3.6.1.2` owns `Makefile` and appends `-p blastit-fastcgi --test
   conformance`; `MB.3.6.2.2` owns `Makefile`, appends `-p blastit-scheduler --test
   conformance_fastcgi`, and asserts in `conformance_fastcgi.rs` that `Makefile`'s
   `rs-test-conformance` line names all five binaries), so deleting or renaming **any** of
   the five binaries fails `make rs-test-conformance` and `make check` with `error: no test
   target named …`.

`RouteFixture`'s `Option<IngressPolicy>` distinguishes `None` (`declared_body` /
`body_none_refuses_bytes` → `413`), `Some(Json)` (`buffered_max` / `expect_continue` →
`Buffered`), and `Some(Raw { allow_raw: true })` (`stream_credit` → `Stream` on
`STREAMS_BODY` upstreams) without any driver matching `case.id` (enforced by `! grep -rn
'case\.id'`). Two fakes that differ only in `kind()` must produce identical accounting.

| Class | Cases (ids) | Asserts |
|---|---|---|
| `refusal` | `method_not_allowed`, `declared_body`, `malformed_path`, `dotfile`, `entry_script`, `params_too_large` | `status == refusal.status()` + `Refusal` + no head written. `duplicate_param`/`unknown_param`/`PHP_VALUE` are internal builder-API `500` guards in `critical_fastcgi_lockdown.rs` |
| `body_cap` | `body_none_refuses_bytes`, `buffered_max`, `stream_credit`, `expect_continue` | `BodyMode`, `expects_continue`, and `413` |
| `observation` | `wire_request_shape`, `param_has_no_value`, `output_line_cap`, `static_file_path`, `observation_budget` | the serialised record and `dropped` caps |
| `error_mapping` | `refused_status`, `unavailable_retry_after` (`503`), `fault_before_head` (`502`), `forbidden_response_header` (`X-Accel-Redirect`/`X-Sendfile`/`X-Lighttpd-Send-File`/`X-Rewrite-URL` → `502`), `benign_x_accel_header_dropped` (`X-Accel-Buffering: no` → `200`, header absent), `fault_after_head` (`RespAbort`), `cancelled_no_status`, `decline_identity` | the contract’s one mapping |

Named binaries in `make rs-test-conformance`: `conformance_table` (`blastit-backend`,
`MB.9.4`), `conformance_static` (`blastit-scheduler`, `MB.9.2.2`), `conformance_wsgi`
(`blastit-scheduler`, `MB.9.3`), `conformance` (`blastit-fastcgi`, `MB.3.6.1.2`), and
`conformance_fastcgi` (`blastit-scheduler`, `MB.3.6.2.2`). Scheduler tests use the fake
double, never a pool.

---

## 5. Security & CVE Analysis

### 5.1 The boundary

- **The scheduler is the only FastCGI client**, using the one recorded seccomp addition
  (`socket(AF_UNIX, …)` allowed, `AF_INET`/`AF_INET6`/`AF_NETLINK` and `SYS_bind`/`SYS_listen`
  `EPERM`) and connecting exclusively via `PinnedSocket`
  (`/proc/self/fd/<PinnedSocket.parent>/<PinnedSocket.name>` — never a path string),
  verifying `(parent_dev, parent_ino)`, `(sock_dev, sock_ino)` (from `Ctl::PoolReady` in
  production), `S_ISSOCK`, `expected_uid`, `0600` and best-effort `SO_PEERCRED`.
- **Params are computed, never named.** The pure builder is the only producer; the
  scheduler re-runs it in `exchange`; the conformance table covers it.
- **The pool receives a fixed param stream**, one script path and a body under credit.
  `security.limit_extensions = .php`, the pinned `SCRIPT_FILENAME`, the `openat2`-checked
  entry and `cgi.fix_pathinfo = 0` close the truncation class.
- **The pool runs as the route's uid**, on a socket `0600` scheduler-owned inside a derived
  per-route `parentfd` (`O_RDONLY|O_DIRECTORY`) that init **creates and pins itself** at boot
  before any route process exists — `<runtime>/fastcgi/<route_index>`, a pure function of
  the route index with no manifest segment in it (`R3`) — with `B1`'s `ARCHITECTURE.md` §5.3
  sticky-bit rule above `<runtime>`, a `fastcgi` container `init_uid:init_gid 0711`, and a per-route
  directory `init_uid` `0700`→`0511` (§4.8.3). The
  socket's path, name, parent, owner and mode are therefore **not steerable by the app's
  configuration at all**: the pre-`MB.3.9.2` classes (two routes pointing at one pool, a
  world-writable parent, a route-owned directory renamed under the pool) cannot be
  expressed, rather than being policed. The parent is restored to `init_uid:init_gid 0511`
  (both owner
  and mode, plus post-freeze socket inode re-verification) on both bind completion and every
  error path; a
  compromised pool gains only what that route's view already had and cannot reach or pivot
  another pool.
- **Response and stderr bytes are untrusted**: head caps, `Status: 200..=599`, CTL
  refusal, stripped framing headers, **the four proxy-control names (`X-Accel-Redirect`,
  `X-Sendfile`, `X-Lighttpd-Send-File`, `X-Rewrite-URL`) refused as `Fault` `502` in
  `ResponseSink` and `cgi`, every other `X-Accel-*` name dropped without a fault**,
  `String::from_utf8_lossy` on every wire-to-`String` conversion, hard `2048`-byte
  partial-line STDERR cap, `SessionDoneGuard` `Drop` preventing any panic from stranding a
  global slot, a short stream aborts, and no header value is logged.
- **No secrets, forged SQL observations, or ambient proxy vars**: `check_sql_obs_fd` is
  closed in `stage0` right after `fork_dbproxy_dev` before `exec_stage1_dev` so no view/pool
  inherits it; the `env[…]` map is the sanitized environment (`BLASTIT_*` and
  `HTTP_PROXY`/`http_proxy` removed, private per-route `HOME`/`TMPDIR` and
  `host=<runtime>/views/<uid>`), INI-escaped, `0600`, never logged.
- **Layered backstops**: the pool pins `allow_url_include=0`, `auto_prepend_file` empty,
  `user_ini.filename` empty, `open_basedir=<manifest_dir>:<runtime>/views/<uid>/tmp`,
  `cgi.fix_pathinfo=0` via `php_admin_value`, so a `.user.ini` planted under `public/`
  cannot re-enable the chain while the param stream is the harness's fixed set (a raw
  `PHP_VALUE` param is stopped by the allowlist wall, §5.2).

### 5.2 CVE defence table

| Technique | Blastit control | `e2e/php2shell.py` row (`MB.3.7`) | Expected status / evidence |
|---|---|---|---|
| `SCRIPT_FILENAME` override/traversal/`?` | pinned; no request bytes | `P1_script_filename` | `200`; `/__inspect` names the entry; canary absent |
| `fix_pathinfo` truncation / polyglot | entry pinned; `.php`/`.phar` `404`; `limit_extensions`; `fix_pathinfo=0` | `P2_polyglot_lfi` | `404` |
| `PHP_VALUE`/`PHP_ADMIN_VALUE` | allowlist; builder only; `php_admin_value` backstops | `P3_php_value` | `200`; `allow_url_include=0` |
| `auto_prepend_file=php://input` chain | same + `pm.max_requests`/retire | `P4_prepend_chain` | `200`, body data; canary absent |
| CVE-2012-1823 | query is data, no php-cgi/argv | `P5_cve_2012_1823` | `200`; literal `query_string` |
| CVE-2024-4577 / CVE-2024-8926 | one decode for `PATH_INFO`; query verbatim | `P6_cve_2024_4577` | `200`; no ini change |
| `httpoxy` | `Proxy` dropped; final-name rule | `P7_httpoxy` | `200`; `getenv('HTTP_PROXY')` false |
| `DOCUMENT_ROOT`/`.user.ini` | pinned; `user_ini.filename` empty | `P8_document_root` | `200`; no prepend canary |
| CVE-2024-8927 (`cgi.force_redirect`/`REDIRECT_STATUS` collision) | `REDIRECT_STATUS` never emitted; `cgi.force_redirect` untouched | `P9_redirect_status` | `200`; `"REDIRECT_STATUS"` ∉ `server_keys` |
| NUL/over-long/duplicate/unknown | http limits, builder caps/map/allowlist | `P10_param_edges` | `400`/`431`/`200`/`200` per row (row 16 is `431`) |
| `.php` source disclosure | `EntryScript` `404` | `P11_php_source` | `404`, no source bytes |
| response/request smuggling | front-end `CL`+`TE`; CGI head caps + stripped `CL` | `P12_smuggling` | `400`/`502` |

Every row is traceable to `docs/research/php2shell.md` §B (`#1`–`#20`) and
`docs/research/php-fastcgi-wire.md` §§C–G. `MB.3.7` replays the page's fences in Docker
against an **unpatched** `php-fpm`; `P4b` is a **positive control**, not a probe: it runs the
same `-d`/`PHP_VALUE` chain against a hand-written pool that *does* accept `PHP_VALUE`
**and** has the `php_admin_value` pins. Research §D shows `PHP_VALUE` overrides
`php_admin_value`, so in `P4b` the chain **executes and its canary is present** — that is the
proof the chain is live and that only the harness's closed param stream (not
`php_admin_value`) stops it; `P4b` is therefore excluded from the "canary absent" assertion.
`P4` (the harness's own pool) keeps the canary absent, and the `.user.ini` half is closed by
the pins.

### 5.3 Why the param contract cannot drift

Three independent walls: (1) the pure crate has no API that takes a caller-named param;
(2) `lockdown` re-checks the map against the allowlist and budgets; (3) the conformance
table drives the same capability-filtered cases against every implementation, and
`MB.3.6` adds the FastCGI rows. A second producer fails (1)/(2) at test time; a status
mismatch fails (3).

---

## 6. Alternatives Considered

Every rejected option names the non-negotiable invariant that killed it:

1. **Top-level `[fastcgi.<name>]`** — killed by **DX single-rule manifest grammar
   (`docs/frameworks/php.md`)**: the accepted page writes `[route.fastcgi]`; a named block
   adds a second indirection for zero config reduction (`ADR-0008` decision 2).
2. **`[route.fastcgi].env`** — killed by **zero-secret-in-manifest & 2-key DX bar**:
   `blastit.toml` holds no inline environment literals and the pool derives `env[…]` from
   init's already-swapped stage-1 environment.
3. **Forward all params and filter `PHP_VALUE`** — killed by **worker-state isolation &
   no-parser-differential invariant**: `PHP_VALUE` persists across requests in `php-fpm`
   workers (`research/php2shell.md` §D); computing the closed allowlist from scratch is the
   only sound gate.
4. **Spawn `php-cgi`** — killed by **CVE-2012-1823 / CVE-2024-4577 `argv` elimination**:
   `php-fpm` has no per-request `argv`.
5. **Root `php-fpm` with `user = route_uid`** — killed by **least-privilege worker &
   rootless-capability invariant (`ADR-0001`)**: no worker master ever runs as `uid 0` or
   holds `CAP_SETUID`.
6. **Pre-binding the Unix listener in `init` via `LISTEN_FDS` (`systemd` socket
   activation)** — killed by **12-factor container portability**: upstream/Alpine `php-fpm`
   (`php:8.4-fpm-alpine`) omits `--with-fpm-systemd` and unconditionally `unlink` + `bind`s
   the path in its generated pool file in `fpm_sockets.c`, failing `EACCES` on a frozen
   non-writable directory; §4.8.3's pinned-`parentfd` bind-then-freeze works on every `php-fpm`
   build, and with a derived path (`R3`) the bind target is blastit's own directory.
7. **Freezing `parentfd` with `chmod 0511` while leaving `st_uid == route_uid`, or opening
   `parentfd` with `O_PATH`** — killed by **DAC immutability & Linux syscall semantics
   (`LESSONS.md` #34)**: the owner of a `0511` directory can `chmod(0755)` and replace the
   socket at will, and `fchmod` on `O_PATH` fails with `EBADF`; `S6` restores `init_uid:init_gid`
   **and** `0511` on an `O_RDONLY|O_DIRECTORY` fd.
8. **A per-route relay process as the FastCGI client** — rejected for 0.2.1: keeps the
   scheduler socket-free but adds a sixth process per route and a second parser; allowing
   `socket(AF_UNIX)` while denying `AF_INET*` and `SYS_bind`/`SYS_listen` on
   `SeccompRole::Scheduler` preserves **no network egress & no rogue listener in scheduler**.
9. **Keep static/WSGI as pre-dispatch branches** — killed by **uniform injected `Upstream`
   contract (owner requirement, `MB.9`)**: dispatch never branches on backend kind.
10. **`async_trait` / per-backend trait / `Box<dyn Any>` plan tokens** — killed by **minimal
    permissive dependency & panic-free determinism invariants**: `BoxFuture` is stable on
    MSRV 1.88 and pure re-derivation cannot fail a downcast.
11. **Keep-alive/multiplexed FastCGI or multi-record `PARAMS` split** — killed by **bounded
    worker-slot lifetime & zero dead-code invariants**: `FCGI_MPXS_CONNS = 0`, idle
    keep-alive pins a worker, and `> 65 535` params are `400` before `connect`.
12. **Stream `raw` bodies to FastCGI** — killed by **FastCGI protocol completeness**:
    `php://input` ignores `STDIN` beyond `CONTENT_LENGTH` (`research/php-fastcgi-wire.md`
    §H), so FastCGI buffers up to `BLASTIT_MAX_BODY_BYTES`.
13. **Keep a user-authored `listen` path (the 0.2.1 draft's third key)** — killed by
    **DX-of-one-knob & blastit-owns-the-socket**: blastit spawns and supervises the pool, so
    the socket is blastit's to name. A user path immediately creates three failure classes
    the harness would have to police — two routes aimed at one pool (one route's identity
    used by another), a socket mode the manifest could contradict (`listen.mode`), and a
    parent the manifest could place anywhere writable — plus an ancestor-safety walk over
    the repository (`manifest_dir` → parent) whose exploit surface was exactly the
    `storage/framework -> /sentinel` pivot. A knob the developer cannot act on correctly is
    a defect, not an option: the derived path is one line of code, needs no documentation,
    and removes the class instead of guarding it (`MB.3.9.2`).
14. **Keep `workers` as the pool's *own* separate budget** (a second admission number) —
    killed by **one budget invariant**: the scheduler's global `slots < max_workers` gate is
    the admission truth for every backend, and the per-route `RouteState` permit is the same
    `view_worker_cap` shape WSGI already uses. A second budget would let a manifest raise
    concurrency the scheduler will never grant, and a pool sized above its route permit
    would only idle. `workers` survives **only** as a downward override inside the one
    budget, which is why a value above it is a boot error rather than a clamp.

---

## 7. Validation & Critical Tests Plan

### 7.1 Tiers

| Tier | Where | Proves |
|---|---|---|
| unit (pure) | `blastit-fastcgi` in-module + `critical_fastcgi_lockdown.rs` (`MB.3.4.1`, `MB.3.4.2`), `critical_fastcgi_manifest.rs` (`check_entry` `ENOSYS` refusal + the **pure** `from_reply_byte` mapping `O/U/N/S/R/M/I/T`, which lives in `blastit-manifest::probe` so this crate can test it; the real forked timeout path is `MB.3.6.3.1`'s `requires-root` leg) | `#![forbid(unsafe_code)]`, zero production deps (`cargo tree -p blastit-fastcgi -e normal`), record round-trip, name-value forms, `lockdown` rows, `cgi` head parser (`Status: 200..=599`, CTL + proxy-control header refusal, caps), the page's exact param stream |
| unit (contract) | `blastit-backend` in-module + `tests/conformance_table.rs` (`MB.9.1.1`, `MB.9.1.2`, `MB.9.4`) | `#![forbid(unsafe_code)]`, zero I/O in its own source (`crates/blastit-backend/src`), object safety + `Send`, registry/line errors, `Refusal` mapping, exact `IMPLEMENTATIONS` slice, fake double |
| critical (protected, existing) | `critical_static_assets.rs`, `critical_static_resolve.rs`, `critical_rewrite.rs`, `critical_ingress_enforcement.rs`, `critical_static_manifest.rs`, `critical_static_cli.rs` | the ports and CLI branch order changed no protected behaviour — **unmodified** |
| critical (new) | `critical_fastcgi_lockdown.rs` (`MB.3.4.2`), `critical_fastcgi_manifest.rs` (`MB.3.5.2`), `blastit-http/tests/critical_head_refusals.rs` (`MB.3.6.1.1`, new file), `critical_fastcgi_dispatch.rs` (`MB.3.6.2.2`), `critical_fastcgi_parent_fd.rs` (`MB.3.6.3.1`), `critical_fastcgi_pool.rs` (`MB.3.6.3.3`), `critical_fastcgi_dev_pool.rs` (`MB.3.9.5`), `critical_fastcgi_learn.rs` (`MB.3.6.4.2`) | lockdown stream & builder guards; grammar/fatal messages; HTTP head refusals; session, credit, abort, `504` timeout, proxy-control `502`, non-UTF-8 & panic slot guard, post-`PoolReady` parent-rename sentinel test; parent-fd argv validation + `cli::parse` guard + the mixed-manifest view fd oracle (`/proc/<view_pid>/fd` holds neither the pinned parent inode nor init's `ctl.chan` inode); pool file, parent `openat2`/freeze/unwind; `dev` banner + `check`/`--learn` |
| conformance | five drivers (§4.11) | one table + exact `IMPLEMENTATIONS` slice + explicit `Makefile` `--test` gate for static, WSGI, FastCGI (codec + session) and fake |
| docs | `docs_fences.rs` (`FILES` bumped `7 → 8` adding `docs/frameworks/php.md` for the TOML fence in `MB.3.5.1`), `e2e/php2shell.py` (non-TOML `curl`/`check`/`--json` fences in `MB.3.7`; the whole `--json` document is asserted, and the Deployments `blastit --preset compose-traefik` fence is explicitly exempt) | the page parses and its commands replay |
| e2e | `e2e/php2shell.py`, `e2e/php/**`, `e2e/rootless/run.py` (`MB.3.7`) | the page's fences, every client CVE probe (`E1`–`E17`, `E19`–`E25`), production multi-uid pool isolation, the route-uid traversable-but-not-writable pool-directory leg and the post-`PoolReady` sentinel checks |

Scheduler tests use an in-process fake peer: a `UnixListener` bound in a `TempDir` opened as
a `PinnedSocket` (never a `socketpair`: the client `connect`s exclusively via
`/proc/self/fd/<parentfd>/<sock_name>`), a scripted thread that **asserts the received bytes
before replying**, and a fake `Upstream` double. `ensure_ready`/backoff is driven with
`tokio::time::pause()` (the dev-dependency already enables `test-util`). No Docker, no
`php-fpm`, no pool in the unit gate.

### 7.2 Requirement → enforcing module → suite

| Reqs | Enforcing | Suite | Positive / negative twins |
|---|---|---|---|
| `R1`–`R6` | `blastit-manifest` | `critical_fastcgi_manifest.rs` (`connect_and_trace_are_boot_fatal`, `workers_zero_and_sixty_five_are_boot_fatal`, `listen_is_refused_by_name`, `planted_symlink_in_the_run_dir_is_refused`, `group_or_world_writable_segment_is_refused`, `regular_file_at_a_segment_is_refused`, `probe_timeout_is_boot_fatal`; `blastit-sys` seccomp fn `scheduler_af_unix_allowed_bind_listen_denied` + `view_twin`) | *(+)* the page's block (`!manifest.is_all_static()` and `manifest.can_boot_without_app()`); defaults; entry under root; `prefork`; the page's shape **plus** `[route.ingress] body="json"`, `[route.db]`, `groups`, `prefork`, no `app` boots clean.<br/>*(−)* missing/absolute/`..`/`\`/NUL/`?` entry, non-`.php`, symlink entry, outside root, unreadable `entry` mode `0000` → boot error naming `entry`, `spa=true` (`FastCgiDeadDeclaration`), `workers=0`/`65`, `workers = 16` with `BLASTIT_VIEW_MAX_WORKERS=8` (`FastCgiWorkersOverBudget` names both numbers; `critical_fastcgi_learn::dev_exits_1_when_workers_exceed_budget` and `check_exits_2_when_workers_exceed_budget`, `MB.3.6.4.2`; the production leg is `e2e/rootless/run.py::check_fastcgi_production_workers_over_budget_exits_1`, `requires-root`), `workers = 8` at a budget of `8` accepted (`workers_at_budget_is_accepted`), **`workers` omitted under `BLASTIT_MAX_WORKERS=512` refused** (pure unit `absent_workers_under_a_512_budget_is_boot_fatal` — an oracle of the deferred budget resolution, owned by the **pool leaf, gated on `MB.12`**; `FastCgiWorkersCapExceeded` names `512` and `64` — and the boot leg `critical_fastcgi_learn::dev_exits_1_under_a_512_budget` in `MB.3.6.4.2`) and `workers = 64` under the same budget accepted, `CONNECT`/`TRACE`, a `listen = "run/fpm.sock"` line (`FastCgiListenDerived`, exact message, and the same block without it boots clean), `env` as an unknown key; `resolved_workers_table`: `None -> cap`, `Some(1) -> 1`, `Some(cap) -> cap` (never `None -> 0`) — pool leaf, gated on `MB.12`;  the static-only app-less route and `spa = true` route without `fastcgi` still get `StaticDeadDeclaration` with its exact 0.2.0 `why` strings |
| `R7`, `R8` (rows 1,3,5–14,17,18,23), `R19` params | `blastit-fastcgi::{record,param,lockdown}` | `critical_fastcgi_lockdown.rs` | *(+)* exact page param stream; `HTTP_*` derivation; 128/65 535 boundaries; `X-Foo`+`X_Foo` first-wins (`Param { Dropped }`); a header value **`> 8192` bytes is dropped** (`200`, `Param { Dropped }`, `over_long_header_value_is_dropped_not_refused`); split deleted (over-budget → 400).<br/>*(−)* every lockdown row by named test fn (§4.7), including row 11 (`REDIRECT_STATUS` absent from stream) and row 18 (`unknown_bare_param_is_refused_500`, internal builder guard); `PHP_*_VALUE`; NUL/CR-LF; `HTTP_PROXY`/`Proxy`; no value in `Param` |
| `R8` rows 15,16,21,22 | `blastit-http` head rules | new `critical_head_refusals.rs` | *(−)* NUL/CR-LF header (`400`), `65 KiB` header name (`> 64 KiB` → `431`), `CL`+`TE` and duplicate `CL` (`400`) by §4.7 named test fns |
| `R8` rows 2,4,19,20, `R12` | `StaticUpstream` + FastCGI path guard | `conformance_static` + `critical_fastcgi_dispatch` | *(−)* `.php`/`.phar` under root (`GET` and `POST` with body) → `404` (`EntryScript`, before `405`/`413`), `no-store`, zero source bytes, never forwarded |
| `R9`, `R10`, `R11`, `R19` caps, `R20`, `R8` rows 24,25 | `blastit-scheduler::upstream::fastcgi`, `cgi`, `ResponseSink` | `critical_fastcgi_dispatch.rs` | *(+)* record order, one connection, `Status: 200..=599`/default `200`, re-assembly of 1-byte/8184/65535-byte chunks, non-UTF-8 `Content-Type` & `STDERR` lossy conversion (zero panic), exact `CONTENT_LENGTH`, `requestId=1`, `prefork` spawn, cold-start waits for `PoolReady`; **post-build parent rename + symlink to sentinel `UnixListener` still serves `200` from `PinnedSocket` while sentinel records `0` `accept`s**.<br/>*(−)* no `END_REQUEST` (`502`/`RespAbort`), bad `protocol_status`, final `1xx` (`502`), non-HTAB CTL/DEL (`502`), the four proxy-control response headers (`X-Accel-Redirect`, `X-Sendfile`, `X-Lighttpd-Send-File`, `X-Rewrite-URL` → `502`) while a benign `X-Accel-Buffering: no` stays `200` with the header absent, missing colon/CRLF/1000 headers/64 KiB head (`502`), **a non-token CGI header name (`X-Accel-Redirect\x00: /x`, ` X-Accel-Redirect: /x`) → `502`** (`blastit-fastcgi` unit `cgi::tests::non_token_header_name_is_fault` + `critical_fastcgi_dispatch::malformed_or_proxy_control_cgi_head_is_502`), over-2048-byte unterminated `STDERR` capped in `O(1)` (`dropped` incremented), **60-lines/request `STDERR` cap**, **8 KiB CGI value cap**, **`BLASTIT_MAX_WORKERS × BLASTIT_MAX_BODY_BYTES` aggregate**, **`drain` and a parked static read keeps its slot**, `CL`/`TE` stripped, `appStatus`/`reserved` ignored, connect `ENOENT`/`ECONNREFUSED` → `503` + backoff, cancel → `ABORT_REQUEST`, **row 25 declared `Content-Length` stalled past `request_timeout` → `504` before head + `ABORT_REQUEST` observed on fake peer + close + slot released once**, **`external_mode_never_calls_ensure_pool`** (recording `PoolPort` fake with `Arc<AtomicUsize>` + panic-on-call; `external_executor = true`, one request → `200` from the scripted fake peer, counter `== 0`; the `false` twin → counter `== 1` and the request waits for `PoolReady`), **`lazy_pool_bind_timeout_is_503_retry_after_never_502`** (no `PoolReady`, advance `5 s + 1 ms`: `503`, `Retry-After: 1`, `UpstreamError::Unavailable { retry_after: Some(1) }`, no `502`, slot released; a later `PoolReady` admits the next request; the never-`PoolReady` `PoolPort` fake is named in §7.1), **`workers_cap_answers_503_for_the_second_concurrent_request`** (`workers = 2`, three concurrent held by a `tokio::sync::Barrier` in the fake `exchange`: two admitted, third `503` before connect, exactly two `connect`s observed, slot released, `try_acquire` yields exactly the route's resolved cap for `Pool` and then `None`) and **`capacity_is_the_resolved_budget_when_workers_are_omitted`** (same shape with `workers` omitted and `BLASTIT_VIEW_MAX_WORKERS=8`: eight permits, ninth `None`; `Capacity` is `Ready`/`Busy` and carries no number, so the oracle is the permit count), both owned by `MB.3.6.2.1`, **panicking fake `Upstream` → `SessionDoneGuard` `Drop` synthesises `Done(Fault)` once, releases global slot & `RoutePermit`, emits no partial `app.output`**, inode/symlink/other-uid socket → `Fault` (`502`), two fake pools (A cannot reach B) |
| `R11` pool file | `supervise::render_pool_conf` (pure) + `critical_fastcgi_pool.rs` | `critical_fastcgi_pool.rs` | *(+)* every directive, `pool_conf_is_fully_rendered` (every directive), `pm.max_children == policy.resolved_workers(view_worker_cap())` (the **pool leaf's** resolution, gated on `MB.12`) in both the omitted and the explicit case (the omitted case is the default `16`), and `pool_conf_uses_the_env_budget` (the renderer is pure and takes `workers: u32`, so this leg builds it from `Config::from_env` with `BLASTIT_VIEW_MAX_WORKERS=8` and asserts `pm.max_children=8`), `pool_rlimits_table` (`nproc`/`nofile` for resolved `2`, `8`, `64`, plus the formula-only `300` — at `resolved ≤ 64` the `max(1024, …)` branch is unreachable, so `nofile` is effectively the constant `1024`), and `fake_pool_child_has_the_resolved_rlimits` (the **unprivileged `PoolHarness`'s forked fake pool child**, which the harness forks through `ChildIo { limits: pool_rlimits(resolved), .. }`, reads `getrlimit(RLIMIT_NPROC).rlim_cur == <resolved>+2` and `RLIMIT_NOFILE` and reports on a pipe; `S3` itself is `requires-root`), `pm.max_requests == max(BLASTIT_VIEW_MAX_REQUESTS, 500)` (never `0`), no `listen.owner`, `env[]` has no `BLASTIT_*` and no `HTTP_PROXY`/`http_proxy`, `HOME == TMPDIR == <runtime>/views/<uid>/tmp` (both inside `open_basedir`), `host=<runtime>/views/<uid>` rewrite, INI escaping; `pool_parent_modes_are_0600_0511_0711` (the parent directory, the socket and the `fastcgi` container: exact masks after `S6`, and `getdents` on `<i>` from a foreign uid is `EACCES`); two routes never share a parent inode.<br/>*(−)* value with `"`/newline inert; a `pm.max_children` that disagrees with the resolved value fails; an explicit `workers = 32` under a budget of `16` never reaches `render_pool_conf` (boot error) |
| `R13`–`R15` | `blastit-backend` + scheduler | `conformance_*`, `conformance_table.rs` | *(+)* `#![forbid(unsafe_code)]` + zero I/O in `blastit-backend/src` (`R14`); exact `IMPLEMENTATIONS` slice equality; each driver's `ran` equals `implementation(kind)`'s matching cases with zero `Skipped`; `Makefile` `rs-test-conformance` names all five binaries; two `kind()`-only fakes identical; `decline_identity`; plan rebuild equality; missing builder with line.<br/>*(−)* dropping any capability bit fails `conformance_table.rs`; `Expect` class-completeness invariants enforced |
| `R16` | `StaticUpstream`, `WsgiUpstream` | existing protected suites + `conformance_static`/`conformance_wsgi` | unmodified protected suites (retained symbols `rewrite::decide`, `static_response::StaticBody`, `head_segments`, `resolve`), including **app-less plain route → `[wsgi]`** (`critical_ingress_enforcement`), `critical_static_cli` app-less `MIXED`, and `critical_rewrite` slot/forward/timeout cases |
| `R17`, `R18` | scheduler writer, `audit_v1`, `dev_learn`, `check::serve_and_run` | `critical_fastcgi_learn.rs` | *(+)* fast unprivileged `dev` boot on the page's static+fastcgi manifest (exact 3-line banner, `1 route`, no `BLASTIT_APP`); `check` with no `--` on the page's manifest exits `2` (`runs an external executor: blastit check -- <suite>`); `app` + `[route.static]` manifest and app-less `MIXED` manifest keep the pytest path (`MIXED`'s `api` route declares neither `[route.static]` nor `[route.fastcgi]`, so `can_boot_without_app()` is false and branch 3 keeps `"no interpreter to run pytest on"`); page's JSON (3 assets → `requests:3, exercised:0, unexercised_route`); one `/users/42` → `requests:1, exercised:1`; one SQL statement via `<CheckRoot>/dev/views/0` child dbproxy → `statements:1` (`check_sql_obs_fd` closed outside dbproxy); head `413` (`executor_ran: false`) emits `missing_ingress` while keeping `exercised: 0`; learn proposes `[route.static] root = "public"`; `Param` no value; legacy `HttpObsLine` without `kind` parses; `check -- <suite>` maps non-zero suite exit to `3`; `config_document_carries_no_check_fd` (the stage-1 document has no `check_*`/`obs_fd` key); `dev` with `BLASTIT_CHECK_POOL_PARENT_FD=0:/etc` or `=99:/run` exits `1` naming the route and never connects by path; **`dev_prints_resolved_pool_workers`** (`MB.3.9.5`; two legs — omitted `workers` with `BLASTIT_VIEW_MAX_WORKERS=8` prints `pool workers 8 (derived from BLASTIT_VIEW_MAX_WORKERS=8)`, explicit `workers = 2` prints `pool workers 2 (workers = 2, budget 8)`, and a third leg with `BLASTIT_VIEW_MAX_WORKERS` unset and `BLASTIT_MAX_WORKERS=8` prints `(derived from BLASTIT_MAX_WORKERS=8)`; once per FastCGI route, and stdout is still exactly the three banner lines); **`dev_starts_one_pool_per_route`** (replaces the retired `external_pool_never_spawns_php_fpm`, which asserted the opposite of the amendment: `dev` never spawning `php-fpm`) — `MB.3.9.5`'s protected suite asserts that a two-FastCGI-route manifest under `dev` yields **two** `php-fpm` masters, each in its own pgid and its own `<route_index>` directory, and that `SIGTERM` to `dev` leaves neither; the `/proc/<init_pid>/task/*/children` walk is transitive (a double-forked `php-fpm` is not a direct child of the main thread) and a **positive control** proves the scanner can see one (a deliberately renamed `/bin/sleep` child found by the same walk). Each suite carries its own `children()`/`fds()` helpers locally (precedent: `critical_dev_and_fail_closed.rs:482`), because `smoke.rs` is a separate test binary and cannot be imported.<br/>*(−)* sentinel `?token=`/header/cookie never in `http_obs.jsonl`; asset-only suite flags the route |
| `R2`, `R3`, `R20` production | manifest probe + boot + `supervise::spawn_pool` + `e2e/rootless/run.py` | `critical_fastcgi_pool.rs`, `critical_fastcgi_parent_fd.rs` + `e2e/rootless/run.py` (single-UID fixture: `fchownat`/`0511`/`0600` assertions are `#[ignore]`d as `requires-root` and the root-only legs run in `e2e/rootless`) | *(+)* init creates `<runtime>/fastcgi/0` (`0700`, `init_uid`), pins it, the pool binds the derived `fpm.sock`, the parent is frozen `init_uid:init_gid 0511`, the socket is handed to the scheduler `0600`, `PoolReady { route, sock_dev, sock_ino }` is published and the route serves; `render_pool_conf`'s `listen=` line equals the derived path and `pm.max_children` equals `resolved_workers`.<br/>*(−)* `dev` with a missing/foreign-uid/`0755` socket exits `1` (`MB.3.9.5`'s `leaf_missing_after_spawn_exits_1_in_dev`, which also proves the `check` twin: `critical_fastcgi_learn::check_child_leaf_failure_exits_2_naming_the_derived_socket` in `MB.3.6.4.2`); unreadable `entry` (`0000`) as route uid exits `1`; missing `CAP_CHOWN`/`CAP_FOWNER` at boot freeze exits `1` naming capability; route A's uid `connect` to pool B → `EACCES`; **after `PoolReady`, `route_uid` attempts `chmod(parent, 0755)`, `open(parent/x, O_CREAT)`, and `unlink(parent/fpm.sock)` → all fail `EPERM`/`EACCES`**; failed verify/bind runs Unwind U1 (`kill(-pgid, SIGKILL)`, `unlinkat`, restore `init_uid:init_gid 0511`, no `PoolReady`); **derived-parent refusal**, three unprivileged legs plus one root-only: `planted_symlink_in_the_run_dir_is_refused` (a symlink at `<run>/fastcgi` or `<run>/fastcgi/<i>` → `Symlink`), `group_or_world_writable_segment_is_refused` (a `0755`/`0777` derived segment → `InsecureMode { mode }`, exactly), `regular_file_at_a_segment_is_refused` (`NotDirectory`), and `check_fastcgi_b2_foreign_owner_segment_refused` in `e2e/rootless` for the root-only `ForeignOwner` leg; the **three `dev` legs** refuse through `FastCgiPoolSocketInvalid` before the pin finishes and leave the sentinel untouched, while the **production** legs (`check_fastcgi_second_boot_normalises_a_frozen_parent`, `check_fastcgi_b2_foreign_owner_segment_refused`) are stage-0 boot diagnostics through `stage0::fail_pool_parent` (`./blastit.toml:<line>: route … pool socket "<segment>": <rule>`, `exit 1`), **not** a `ManifestError`; `foreign_owner_segment_is_refused` is `#[ignore] requires-root` (a non-root fixture cannot create a foreign-owned directory) and its `B2` twin runs in `e2e/rootless`; **`eight_planted_stale_entries_leave_only_that_route_503`** (temp `parentfd` + eight `stale.*` entries: a boot **warning**, `PoolExited`, that route `Busy` → `503` + `Retry-After: 1`, and the other routes still boot and serve — never `exit 1`); **the route uid traverses but cannot write the pool directory** (`e2e/rootless/run.py::check_fastcgi_pool_dir_is_traversable_but_not_writable_for_the_route_uid`, exact three-way predicate: `stat`/`openat(O_PATH)` on `<runtime>/fastcgi/<i>/fpm.sock` **succeeds**; `readdir(<runtime>/fastcgi)` is `EACCES`; `O_CREAT`/`unlinkat`/`renameat`/`symlinkat` inside `<i>` and `mkdirat`/`renameat` in `fastcgi` fail `EPERM`/`EACCES`), a decoy listener pre-bound at `/sentinel/fpm.sock` records **`0` `accept`s**, and the live `GET /users/42` still answers `200` from the pinned `entry`; `RETIRE`/respawn binds a fresh `(sock_dev, sock_ino)` inside the same `parentfd` and answers `200`; **`e2e/rootless/run.py::check_fastcgi_second_boot_normalises_a_frozen_parent`** (`requires-root`) re-runs `B1`–`B3` over a preserved `<runtime>` whose `<i>` was left `0511` and asserts a normalised `0700` pin, `PoolReady` and `200`, and **`check_fastcgi_b2_foreign_owner_segment_refused`** refuses a foreign-owned `fastcgi`/`<i>` segment (the init-side assertion — `/proc/<init_pid>/fd` still resolves the pinned parent after `PoolReady` and after `RETIRE`/respawn — is `requires-root` and lives in `e2e/rootless`; the unprivileged `dev` addition checks `/proc/<init_pid>/fd` holds a directory fd whose `st_dev`/`st_ino` equal the fixture's `run/` inode); **`critical_fastcgi_parent_fd::view_child_holds_neither_parentfd_nor_ctl_chan`** (mixed `[route.fastcgi]` + WSGI manifest: the fixture binds a `UnixListener` at the derived `<run_root>/fastcgi/0/fpm.sock` in its temp dir, writes a `smoke.rs`-style `app`, and never starts `php-fpm`; preconditions asserted — WSGI response `200`, view pid found via `/proc/<init_pid>/task/<init_pid>/children`, fd set non-empty and containing `view.chan` + stdio; the predicate is **inode identity on both halves**: (a) the `(st_dev, st_ino)` of the pinned parent directory and (b) the inode read from init's `ctl.chan` entry before the request are both absent from the view's table, while the view's `view.chan` inode **is** present; no `#[ignore]`/skip), **`S0`'s `php-fpm -t` child and the `S3` pool master exec with only fd 0/1/2** (`critical_fastcgi_pool::config_test_and_pool_child_exec_with_only_stdio`: the fake `php-fpm` helper's first `fcntl(3, F_GETFD)`/`read(3, …)` returns `EBADF`, and no `/proc/self/fd` target equals the pinned parent path or a `socket:[<ino>]`; the exact `{0,1,2}` set is not asserted because a wrapper's own script fd and the scanner's directory handle make it unobservable); **`setsid()` escapee containment** (`critical_fastcgi_pool::pool_ready_inode_is_held_by_a_task_in_the_new_pgid` + `critical_fastcgi_pool::setsid_escapee_is_never_published` + `e2e/rootless/run.py::check_fastcgi_setsid_escapee_contained`): the fake pool records the inode it bound on a pipe and the test asserts `PoolReady.sock_ino` **equals that inode** (not merely "a different one") and that `/proc/<master_pid>/fd` contains `socket:[<ino>]`; then a fake worker `setsid()`s and keeps/rebinds a listener at `parentfd/fpm.sock` during the next `S2`, and the test asserts **no** `PoolReady` is emitted, `U1` runs (kill, freeze `0511`, unlink, `PoolExited`), `RouteState` stays `Busy` → `503` + `Retry-After: 1`, and the escapee's listener records **`0` `accept`s**; only after the harness kills the escapee does a clean respawn publish the new master's own inode |

No new critical test weakens an existing one; `tools/critical_guard.py` covers the
existing `critical_*.rs` files, none of which is edited by `MB.9` or `MB.3`.

### 7.3 e2e replay (`MB.3.7`) and the fixture contract

`e2e/php2shell.py` runs in an image with unpatched `php-fpm`, the shipped wheel and a
Symfony-style `public/index.php` as uid 1000, `HOME=/home/you`, `WORKDIR
/home/you/app`. It replays the page's Local/CI fences (`uv init`/`uv add` exempt as in
`static_assets.py`, an unknown `$ ` command is a failure) — the page's Local section is
`MB.3.9.1`'s, and with the `MB.3.9.2` grammar it contains no hand-written pool file and no
socket path, so the replay asserts what the page shows and what needs no dev-pool provider: `dev` answers and
the **generated** pool files are correct. "The pool `dev` started exists" is `MB.3.9.5`'s
protected assertion (`dev_starts_one_pool_per_route`) and only holds **after `MB.3.9.4`** flips
`dev` to `PoolSpawn::Managed`; before that, `stage1` sets `External` for `dev`/`check`
(§4.8.1) and `MB.3.7` runs before `.4`. It then renders **the generated** pool files (`dev`'s and production's) with
`render_pool_conf`, runs `php-fpm -tt -y` on them, and asserts `listen=` is
`<runtime>/fastcgi/<route_index>/fpm.sock`, `pm.max_children` is the resolved worker count,
`listen.mode=0600`, `clear_env=yes` and the `php_admin_value` backstops — so a drifted
directive fails, and parses `N` in `check: N requests, …`.

The fixture's front controller answers `GET`/`POST /__inspect` with exactly
`{"script_filename","script_name","path_info","document_root","request_uri",
"query_string","request_method","content_length","allow_url_include","http_proxy",
"user_ini","server_keys"}` (`server_keys` is `array_keys($_SERVER)` — names only, never
values — so row 11 asserts `"REDIRECT_STATUS" not in server_keys` directly in e2e alongside
`critical_fastcgi_lockdown.rs`); `PROBES` has one entry per client-reachable `E<n>` in §4.7
(**24** probes: `E1`–`E17` and `E19`–`E25`; row 18 is marked `—` in §4.7 because bare
non-`HTTP_` param names have no HTTP client path and are covered by
`critical_fastcgi_lockdown::unknown_bare_param_is_refused_500`), each carrying an expected
status and a discriminating `/__inspect` subset, and the canary files (`prepend.php`,
`evil.php`, `shell.jpg`) must be absent from any response **except `P4b`**, whose canary
must be **present** (it is the positive control of §5.2, excluded from the absent
assertion). `--self-test` parses §4.7's
table, asserts the 24 non-`—` rows match `PROBES` 1-to-1, parses the `#n` column to assert
the union is exactly `{1..20}`, and requires every `200`
probe to carry a **non-empty** discriminating key-value assertion; the `P` groups above are
the fixture functions that may serve several `E` rows (`P12` includes a raw-socket
malformed/proxy-control CGI-head responder for row 24). Two PHPUnit suites back the two CI
fences on the page: `uv run blastit check -- vendor/bin/phpunit` runs against
`e2e/php/suite/` (24 HTTP requests → `blastit check: 24 requests, 0 statements, 1/1
routes, clean`), and `uv run blastit check --json -- vendor/bin/phpunit` runs against
`e2e/php/assets/` (3 asset requests only → `requests:3, exercised:0, unexercised_route`).

---

## 8. Implementation Plan

**Plan ID ↔ design label mapping (exact 1-to-1, every leaf ≤ 7 files and ≤ 550 lines):**
`MB.3.4.1` (`3.4a`), `MB.3.4.2` (`3.4b`), `MB.3.5.1` + `MB.3.5.2` (`3.5` split),
`MB.9.1.1` + `MB.9.1.2` (`9.1` split), `MB.9.2.1.1` + `MB.9.2.1.2` + `MB.9.2.1.3` (`9.2a`
split), `MB.9.2.2` (`9.2b`), `MB.9.3`, `MB.9.4`, `MB.3.6.1.1` + `MB.3.6.1.2` (`3.6a1`
split), `MB.3.6.2.1` + `MB.3.6.2.2` (`3.6a2` split), `MB.3.6.3.1` + `MB.3.6.3.2` +
`MB.3.6.3.3` (`3.6b` split),
`MB.3.6.4.1` + `MB.3.6.4.2` (`3.6c` split), `MB.3.7`, `MB.3.8`.

**Order.** `MB.3.4.1` → `MB.3.4.2` → `MB.3.5.1` → (`MB.3.5.2` ∥ `MB.9.1.1` → `MB.9.1.2` →
`MB.9.2.1.1` → `MB.9.2.1.2` → `MB.9.2.1.3` → `MB.9.2.2` → `MB.9.3` → `MB.9.4`) →
`MB.3.6.1.1` → `MB.3.6.1.2` → `MB.3.6.2.1` → `MB.3.6.2.2` → `MB.3.6.3.1` →
`MB.3.6.3.2` → `MB.3.6.3.3` → `MB.3.6.4.1` → `MB.3.6.4.2` → `MB.3.7` → `MB.3.8`.
Note `MB.9.1.1` depends on `MB.3.5.1` so `blastit_manifest::Executor` exists before
`blastit-backend` compiles. Epic `MB.9` must complete **before `MB.4` (Node)** is
implemented.

**`Cargo.lock` and workspace-member ownership (one serialized chain, two-step `--locked`
rule).** Every leaf that creates a workspace member or edits a `Cargo.toml` owns
`Cargo.lock` in one strictly serialized `depends_on` chain (`MB.3.4.1` → `MB.9.1.1` →
`MB.9.2.1.1` → `MB.3.6.1.2` → `MB.3.8`): its **first** build command runs offline
**without `--locked`** (`CARGO_NET_OFFLINE=true cargo test -p <pkg>`) to write `Cargo.lock`
(`members = ["crates/*"]` needs no root `Cargo.toml` edit until `MB.3.8`'s version bump),
and every subsequent command and `make check` runs **with `--locked`**. No other leaf
touches any `Cargo.toml` or `Cargo.lock`, and every non-lock leaf runs `--locked` from its
first command.

**Canonical predicates (identical to §1.3, §4.2, §4.10.3 and ADR-0008).**
- `Manifest::is_all_static(&self) -> bool`:
  `self.app.is_none() && !self.routes.is_empty() && self.routes.iter().all(|r| r.static_policy().is_some() && r.fastcgi().is_none())`.
- `Manifest::can_boot_without_app(&self) -> bool`:
  `!self.routes.is_empty() && self.routes.iter().all(|r| r.fastcgi().is_some() || r.static_policy().is_some())`.
  `stage0::app_rule` refuses missing `app` iff `config.app.is_none() && !manifest.can_boot_without_app()`, keeping `critical_static_cli`'s `ALL_STATIC` and `MIXED` assertions green while booting the PHP page's manifest.
- Per-route `executor`: `Some(Pool)` when `fastcgi.is_some()`, `Some(View)` when `fastcgi.is_none() && (static_policy.is_none() || static_forward)`, `None` otherwise (`critical_ingress_enforcement`'s plain route gets `Some(View)`; `with_fastcgi` runs after `with_static`).
- Dead-declaration rule (both arms preserved): `static_policy.is_some() && fastcgi.is_none() && (spa || app.is_none())` keeps `StaticDeadDeclaration` and its exact 0.2.0 `why` phrases; `spa = true` with `[route.fastcgi]` is `FastCgiDeadDeclaration`.
- `check` branch order: (1) `manifest.is_all_static()` → exit `0`; (2) `config.app.is_none() && manifest.can_boot_without_app()` → external-executor mode (`-- <suite>` runs `check::serve_and_run`; empty `--` exits `2` with `route "<first_fastcgi_route>" runs an external executor: blastit check -- <suite>`); (3) everything else (`config.app.is_some()`, or an app-less mixed manifest with `!manifest.can_boot_without_app()`) → existing pytest path (`--` forwarded verbatim to `pytest`, keeping `docs/ci.md`'s `-- -k products` on app manifests and `critical_static_cli`'s `"no interpreter to run pytest on"` on app-less `MIXED`).

| Leaf ID | Files (count ≤ 7) | Lands (≤ 550 lines) | Acceptance |
|---|---|---|---|
| `MB.3.4.1` | `Cargo.lock`, `crates/blastit-fastcgi/{Cargo.toml,src/{lib,record,param,cgi}.rs}` (6) | `#![forbid(unsafe_code)]`, records, name-value codec, CGI head parser (`Status: 200..=599`, CTL & proxy-control refusal, caps), `RecordError`/`CgiError`, zero deps | `CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi && cargo test -p blastit-fastcgi --locked && test -z "$(cargo tree -p blastit-fastcgi -e normal --prefix none \| tail -n +2)"` |
| `MB.3.4.2` | `crates/blastit-fastcgi/src/{lib,lockdown}.rs`, `crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs` (3) | `pub mod lockdown;` in `src/lib.rs` (the file `MB.3.4.1` created), param builder, allowlist, `LockdownError`, `FastCgiPaths` (defined here), every §4.7 `lockdown` row (asserts `LockdownError::*`, never `Refusal`) | `cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked` |
| `MB.3.5.1` | `crates/blastit-manifest/src/{model,parse,validate,error,lib}.rs`, `crates/blastit-manifest/tests/docs_fences.rs` (6) | `FastCgiPolicy` (`#[derive(Debug, Clone, PartialEq, Eq)]`) as `{ entry: String, workers: Option<u32> }` — **no `listen` field**, `Option` because absent means derived — **without** the pool-path resolution (`resolved_workers`, `MAX_POOL_WORKERS`, the budget check and its two variants are the **pool leaf's**, gated on `MB.12` — see §8.1 item 4), `Executor` (`#[derive(Debug, Clone, PartialEq, Eq)]`, **not** `Copy`; `ALL_KINDS`, `kind()`), **both added to `lib.rs`'s `pub use model::{…}` list** (the crate root path `blastit_manifest::Executor`), `Route::fastcgi()`, `can_boot_without_app()`, `is_all_static()`, **grammar only**: `FastCgiListenDerived` (landed), `FastCgiDeadDeclaration`, `FastCgiWorkersInvalid` for an explicit value outside `1..=64`, and deletion of `FastCgiListenInvalid`/`FastCgiListenParentInvalid`; **`FastCgiPoolSocketInvalid` is `MB.3.5.2`'s** (the probe's refusal, below), `route_lines()`, dead-declaration rule, the strict parser's `fastcgi_policy` reading only `entry`/`workers` and refusing `listen` with the landed `FastCgiListenDerived` (no value; exact message as §4.1), `docs_fences.rs` (three edits: `const FILES: [&str; 7]` → `8` with the new path, `assert_eq!(total, 7)` → `8`, and the module-doc "seven" → "eight") | `cargo test -p blastit-manifest --locked` (includes the `lib.rs` doctest `use blastit_manifest::{Executor, FastCgiPolicy}; assert_eq!(Executor::View.kind(), "wsgi"); assert!(Executor::ALL_KINDS.contains(&"fastcgi"));`) |
| `MB.3.5.2` | `crates/blastit-manifest/src/{probe,lib}.rs` (`.1` first appends the two `pub use model::{…}` names; `.2` then appends `pub mod probe;`), `crates/blastit/src/{probe,lib,supervise}.rs`, `crates/blastit-manifest/tests/critical_fastcgi_manifest.rs`, `crates/blastit-manifest/src/error.rs`, `docs/reference/manifest.md` (8) | `ManifestError::FastCgiPoolSocketInvalid { location, path, rule }` (the probe's refusal; `MB.3.5.1` landed the grammar only), pure `SocketProbe` (`parent(run_root, route_index)` + `check_leaf(parent, uid)` + `check_entry` + `provides_parent_fd`) + `SocketProbeError` (`Missing`/`NotSocket`/`NotRegularFile`/`NotDirectory`/`InsecureMode{mode}`/`NotConnectable{uid}`/`Unreadable{uid}`/`OutsideRoot`/`Timeout`/`ForeignOwner{uid}`/`Symlink`/`Io`; **no** `ParentWritableOrSymlink`) + `from_reply_byte`, `ProbeUid`, `check_fastcgi_entries(.., probe, uid)`, `check_fastcgi_sockets(run_root, probe) -> Vec<(usize, OwnedFd)>` and `check_fastcgi_leaf(run_root, route_index, parent, probe)`, plus the private `probe::pool_socket_invalid(route_index, run_root, rule)` both of them format through in `blastit-manifest` (the trait lives here, not in `MB.3.5.1`); the syscall bodies of `check_entry` are in `crates/blastit/src/probe.rs`; `DevProbe`/`ProductionProbe` in `crates/blastit/src/probe.rs` (`rustix` already a dep; **no** manifest `Cargo.toml` change); `crates/blastit/src/lib.rs` gains `pub mod probe;` and `pub mod supervise;` (the single owner of `lib.rs`); in `supervise.rs`, `Parts`, `Supervisor` and `Supervisor::new` become **`pub(crate)`** while `render_pool_conf` (added by `MB.3.6.3.2`) and the later inline `pool_test` module are `pub` (so `private_interfaces` stays silent); protected `critical_fastcgi_manifest.rs` (injects `NotRegularFile`/`Unreadable { uid }`/`OutsideRoot` through the fake `check_entry`, `ForeignOwner`/`NotSocket`/`Symlink`/`NotDirectory` through the fake `parent(run_root, i)`, `NotConnectable`/`NotSocket` through the fake `check_leaf`, then **exactly** §7.2's `critical_fastcgi_manifest.rs` inventory — `connect_and_trace_are_boot_fatal`, `workers_zero_and_sixty_five_are_boot_fatal`, `listen_is_refused_by_name` (exact message), `planted_symlink_in_the_run_dir_is_refused`, `group_or_world_writable_segment_is_refused`, `regular_file_at_a_segment_is_refused`, `probe_timeout_is_boot_fatal` — the grammar/parse-bound rows only, because the **budget-aware** `Manifest::check_fastcgi_workers(budget)` does not exist until the pool leaf lands it (the private grammar-only `validate::check_fastcgi_workers(&location, policy, errors)` at `validate.rs:597` is a different function); while `resolved_workers_table`, `workers_at_budget_is_accepted`, `workers_over_budget_is_boot_fatal`, `absent_workers_under_a_512_budget_is_boot_fatal` (**pool leaf, gated on `MB.12`**) and `capacity_is_the_resolved_budget_when_workers_are_omitted` (`MB.3.6.2.1`) are that leaf's/that unit's, never rows here), the 3 §4.1 edits in `docs/reference/manifest.md` | `cargo test -p blastit-manifest --test critical_fastcgi_manifest --locked && cargo test -p blastit --locked && ! grep -qE 'rustix&#124;libc' crates/blastit-manifest/Cargo.toml && make docs-check` |
| `MB.9.1.1` | `Cargo.lock`, `crates/blastit-backend/{Cargo.toml,src/{lib,types,registry}.rs}` (5) | `#![forbid(unsafe_code)]`, `Upstream`, §4.2.1 types (`RouteId`, `RouteState` with `try_acquire(self: &Arc<Self>) -> Option<RoutePermit>` (`Send`, owns the `Arc`), `PinnedSocket::new(Arc<OwnedFd>)` + accessors + `PinnedSocketError::{BadName,Poisoned}`, `UpstreamHead`, `ResponseSink`, `Observation`, `UpstreamError`, `Refusal`), `Registry`, `BootError` (+ `std::error::Error`), object-safety/`Send`/line tests | `CARGO_NET_OFFLINE=true cargo test -p blastit-backend && cargo test -p blastit-backend --locked && ! grep -rn 'UnixStream\|TcpStream\|std::fs\|Command::new\|tokio' crates/blastit-backend/src` |
| `MB.9.1.2` | `crates/blastit-backend/src/{lib,conformance}.rs` (2) | `Case`, `Class`, `Capability`, `Expect`, `Inject`, `RouteFixture`, `HeadSpec`, `OutcomeKind`, `PeerScript`, `CancelAt`, `IMPLEMENTATIONS`, `implementation()`, `cases()` | `cargo test -p blastit-backend --locked` |
| `MB.9.2.1.1` | `Cargo.lock`, `crates/blastit-scheduler/{Cargo.toml,src/{route,config,lib}.rs}`, `crates/blastit-frame/src/raw.rs`, `crates/blastit/src/stage1.rs` (7) | `blastit-backend` + **`blastit-http`** deps + lock (the latter for `BodyFraming`), `ReqHeadView` `scheme() -> Scheme` (frame-owned; no new `blastit-frame` edge; `parse` stores `scheme: Scheme` via `Scheme::from_code(..)?`, rejecting an unknown code with `FrameError::BadEnum`, and the stale `parse` doc comment saying the scheduler never reads it is updated), `server_name`/`server_port`, `RouteInfo.executor` (`with_fastcgi` after `with_static`), `SchedulerConfig.{http_obs: Option<PathBuf>, pool_spawn, pinned_sockets, route_lines}` (`http_obs` is `None` until `MB.3.6.3.1` wires `Config.check_http_obs`) plus `pinned_socket_fds() -> Vec<RawFd>` and `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum PoolSpawn { Managed, External }` (`stage1.rs` sets `route_lines` from `Manifest::route_lines()` and forwards `pool_spawn`/`pinned_sockets`/`http_obs`; the literal ends in `..SchedulerConfig::default()`, so omitting them is a silent empty `route_lines` and every route is a `BootError`) | `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler && cargo test -p blastit-scheduler --locked && cargo tree -p blastit-scheduler -e normal \| grep -q blastit-http && cargo test -p blastit --locked` |
| `MB.9.2.1.2` | `crates/blastit-scheduler/{src/{upstream/mod,scheduler,error,lib}.rs,tests/head_allocs.rs}` (5) | `UpstreamHead` (`from_utf8_lossy`), `RouteRuntime.stages`, `ViewHub`/`SessionPort`, session task + `SessionDoneGuard` (`Drop` → `Done(Fault)`) + `ResponseSink` proxy-control denial, `http_obs.jsonl` writer, `run_with` + `RunError`/`SchedulerError::Boot` (`error.rs`), **`builtin_registry()` definition and `run` delegating to `run_with`**, `mod upstream;` declared in `lib.rs` (crate root, so `crate::upstream` resolves to `src/upstream/mod.rs`; `.3` only appends `pub use` lines), `BuildCtx.external_executor` from `pool_spawn`, the transitional `stages.is_empty()` path | `cargo test -p blastit-scheduler --locked` (crate root, not a `--lib` subset) |
| `MB.9.2.1.3` | `crates/blastit-scheduler/src/lib.rs`, `crates/blastit/src/roles.rs` (2) | `pub use crate::scheduler::{builtin_registry, run_with};` and `pub use crate::error::{RunError, SchedulerError};` **re-exports only** (definitions are in `.2`), `roles::scheduler` `run_with` + `RunError` print (Boot → stderr + `1`) | `cargo test -p blastit --locked` |
| `MB.9.2.2` | `crates/blastit-scheduler/src/{upstream/mod,upstream/static,scheduler}.rs`, `crates/blastit-scheduler/tests/{conformance_static.rs,common/fake_upstream.rs}` (5) | `StaticUpstream` (`plan` = `head_segments` + `.php` gate; `exchange` = `resolve` + `decide`), `pub mod static;` in `upstream/mod.rs`, deleted static branches, `"static"` appended to `builtin_registry()`'s list **in `scheduler.rs`**; `conformance_static.rs` includes the fake as `#[path = "common/fake_upstream.rs"] mod fake_upstream;` | `cargo test -p blastit-scheduler --test conformance_static --test critical_static_assets --test critical_static_resolve --test critical_rewrite --locked && cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings` |
| `MB.9.3` | `crates/blastit-scheduler/src/{upstream/wsgi,upstream/mod,scheduler,lib}.rs`, `crates/blastit-scheduler/tests/conformance_wsgi.rs` (5) | `WsgiUpstream`, `ExchangeChannel`, `"wsgi"` appended to `builtin_registry()`'s list **in `scheduler.rs`**, observation parity, **deletes the transitional `stages.is_empty()` fallback** (a missing builder becomes `RunError::Boot`) | `cargo test -p blastit-scheduler --test conformance_wsgi --test critical_ingress_enforcement --locked && cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings` |
| `MB.9.4` | `crates/blastit-backend/tests/conformance_table.rs`, `Makefile` (2) | exact `IMPLEMENTATIONS` assertion, `Expect` class invariants, fake double, `rs-test-conformance` (3 binaries; `MB.3.6.1.2` bumps it to 4, `MB.3.6.2.2` to 5) in `make check` | `make rs-test-conformance && ! grep -rn 'case\.id' crates/*/tests/conformance*.rs` |
| `MB.3.6.1.1` | `crates/blastit-sys/src/seccomp.rs`, `crates/blastit-frame/src/{ctl,frame}.rs`, `crates/blastit-http/tests/critical_head_refusals.rs`, `crates/blastit-scheduler/tests/common/mod.rs`, `crates/blastit/src/supervise.rs` (6) | scheduler `AF_UNIX` + `SYS_bind`/`SYS_listen` `EPERM` + `Scheduler`/`View` twins, **`Ctl::Spawn`'s doc comment updated to "view or pool, chosen by the route's executor"**, `PoolSpawned`/`PoolReady { route, sock_dev, sock_ino }`/`PoolExited` (`0x45`–`0x47`), HTTP head refusal rows, **the exhaustive `Ctl` matches in `tests/common/mod.rs` and `crates/blastit/src/supervise.rs:430-443` gain `Ctl::PoolSpawned { .. } \| Ctl::PoolReady { .. } \| Ctl::PoolExited { .. } => {}`** (otherwise `E0004`; `scheduler.rs:1043` already has an `other =>` catch-all) | `cargo test -p blastit-sys --locked && cargo test -p blastit-frame --locked && cargo test -p blastit-http --test critical_head_refusals --locked && cargo test -p blastit --locked && cargo test -p blastit-scheduler --test critical_rewrite --locked` |
| `MB.3.6.1.2` | `Cargo.lock`, `crates/blastit-scheduler/Cargo.toml`, `crates/blastit-fastcgi/{Cargo.toml,tests/conformance.rs}`, `crates/blastit/Cargo.toml`, `Makefile` (6) | workspace Cargo edges + lock — **`blastit-backend = { version = "0.2.0", path = "../blastit-backend" }` added to `crates/blastit/Cargo.toml`**, `blastit-fastcgi` dev-edge, `"fastcgi-codec"` conformance driver, append `-p blastit-fastcgi --test conformance` to `Makefile` | `CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --test conformance && cargo test -p blastit --locked && cargo tree -p blastit -e normal \| grep -q blastit-backend && make rs-test-conformance` |
| `MB.3.6.2.1` | `crates/blastit-scheduler/src/{upstream/fastcgi,upstream/fastcgi_builder,upstream/mod,scheduler}.rs` (4) | `FastCgiUpstream` session pump: builds `FastCgiPaths`, maps `LockdownError`/`CgiError`/`RecordError` to `Refusal`/`UpstreamError::Fault`, `PinnedSocket` `/proc/self/fd` connect + `C1` (inode/owner/`0600`/exact `SO_PEERCRED`), params, `CONTENT_LENGTH` buffering, `STDIN`/`STDOUT`/`STDERR` pumps, `END_REQUEST`, `ABORT_REQUEST`, credit, `ensure_ready` (`PoolPort::ensure_pool` only when `!external_executor`), `on_ctl` handling of `Ctl::PoolSpawned`/`PoolReady`/`PoolExited` (stores `bound_inode`, sets `RouteState.ready`, backoff on exit), `RouteState::new(pool_spawn == PoolSpawn::External, policy.resolved_workers(cfg.view_worker_cap()))` (Busy in `Managed` until `PoolReady`), `"fastcgi"` appended to `builtin_registry()`'s list **in `scheduler.rs`** | `cargo test -p blastit-scheduler --locked` |
| `MB.3.6.2.2` | `crates/blastit-scheduler/tests/{critical_fastcgi_dispatch,conformance_fastcgi}.rs`, `Makefile` (3) | dispatch + conformance suites: record order, chunk re-assembly, non-UTF-8, row 24/25, sentinel parent-rename, panic/`SessionDoneGuard`, `foreign_uid_peer_is_fault_502`, `"fastcgi-session"` driver, `Makefile` append + 5-binary assertion | `make rs-test-conformance && cargo test -p blastit-scheduler --test critical_fastcgi_dispatch --locked && cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings && ! grep -rn 'case\.id' crates/blastit-scheduler/tests/conformance_fastcgi.rs` |
| `MB.3.6.3.1` | `crates/blastit/src/{stage0,stage1,config,cli,dev_learn}.rs`, `crates/blastit/tests/critical_fastcgi_parent_fd.rs` (6) | `B1`–`B3` (stage 0; the derived-parent boot diagnostic is `stage0::fail_pool_parent(line, route, path, rule)`, asserted by `critical_fastcgi_parent_fd::production_parent_failure_names_line_route_and_rule`), `remove_entry` and its two units — `planted_entry_at_the_leaf_is_removed` (socket/symlink/regular file/empty dir/non-empty dir/`ENOENT`) and `eight_planted_stale_entries_leave_only_that_route_503` (a boot warning + that route's `Busy`/`503`, other routes unaffected, **never** `exit 1`), `check_fastcgi_entries(…, i, ProbeUid::Route(uid))` per FastCGI route in production and `stage0::run_dev` with `ProbeUid::Dev` and `check_fastcgi_workers(config.view_worker_cap())` in both, **and the §1.2 `dev` line** (`blastit dev: fastcgi route "…": pool workers N (…)`, emitted once per FastCGI route on stderr through the existing `eprintln!("blastit dev: …")` channel, naming the variable that supplied the cap), **`critical_fastcgi_parent_fd.rs`** (one row per §4.8.1 validation rule driven through the pure `validate_parent_fds(&[(usize, RawFd)], config_fd, dbproxy_fds) -> Result<(), BootError>` — so "no descriptor wrapped" is unit-tested before any `OwnedFd` — plus `cli::parse` rejects `--fastcgi-parent-fd` without `--stage=1` and `--help` lacks the string, and `view_child_holds_neither_parentfd_nor_ctl_chan`: mixed FastCGI+WSGI fixture (bind a `UnixListener` at the derived `<run_root>/fastcgi/0/fpm.sock`, `smoke.rs`-style `app`, one WSGI request, enumerate `/proc/<view_pid>/fd`); ~150 lines of harness, within the 550-line cap), `--fastcgi-parent-fd` argv + `Options.fastcgi_parent_fds` + validate-before-wrap + `Arc` sharing (stage 1), `SchedulerConfig.{route_lines, pinned_sockets, pool_spawn}`, `Config.check_*` (never in `to_json`), `cli.rs` internal arm, `Dev::adopt` wiring (`dev_learn.rs:180` already has it), `BLASTIT_CHECK_*` plumbing | `cargo test -p blastit --test critical_fastcgi_parent_fd --locked && cargo test -p blastit --locked` |
| `MB.3.6.3.2` | `crates/blastit/src/supervise.rs`, `crates/blastit/tests/{support/fake_php_fpm.rs,fake_php_fpm_smoke.rs}` (3) | `render_pool_conf`, `pool_rlimits` + `pool_rlimits_table`, `S0`–`S7`/`U1` (reap-to-`ECHILD`, pgid membership, freeze-before-unlink, `forking::fork(true)`), the inline `#[doc(hidden)] pub mod pool_test` harness (whose fake pool child is forked through the `S3` `ChildIo` literal with `limits: pool_rlimits(workers)` and reports `getrlimit(RLIMIT_NPROC/NOFILE)` on a pipe — `fake_pool_child_has_the_resolved_rlimits`), the fake `php-fpm` helper, and a one-line `tests/fake_php_fpm_smoke.rs` (`#[path = "support/fake_php_fpm.rs"] mod fake_php_fpm;`) so Cargo actually builds the helper | `cargo test -p blastit --test fake_php_fpm_smoke --locked && cargo test -p blastit --locked && cargo clippy -p blastit --all-targets --locked -- -D warnings` |
| `MB.3.6.3.3` | `crates/blastit/tests/critical_fastcgi_pool.rs` (1) | the protected pool suite: `S0`/`S3` fd-table negative oracle, pgid membership, escapee non-publication, `U1` freeze/unlink, `PoolReady` inode equality, `pool_parent_modes_are_0600_0511_0711`, `planted_entry_at_the_leaf_is_removed`'s non-empty-directory leg, `fake_pool_child_has_the_resolved_rlimits` | `cargo test -p blastit --test critical_fastcgi_pool --locked && cargo clippy -p blastit --all-targets --locked -- -D warnings` |
| `MB.3.6.4.1` | `crates/blastit/src/{audit_v1,dev_learn,check}.rs` (3) | two-shape `http_obs.jsonl` reader (`HttpObsLine` unchanged), `CheckObsV1::new_with_upstream`, combined-set guards, `executor_requests_per_route` (`executor_ran`), `413` `missing_ingress`, `--learn` static proposal | `cargo test -p blastit --locked` |
| `MB.3.6.4.2` | `crates/blastit/src/{cli,check}.rs`, `crates/blastit/tests/critical_fastcgi_learn.rs` (3) | 3-way `check` branch order, `check::{DEV_ROOT, SQL_OBS_FD, POOL_PARENT_FD}`, `check::serve_and_run` (`<CheckRoot>/dev` handoff, `Vec<(usize, OwnedFd)>` from `check_fastcgi_sockets` → `clear_cloexec` → child `dev`, `--port 8080`, no parent dbproxy, exit `3`), `critical_fastcgi_learn.rs` | `cargo test -p blastit --test critical_fastcgi_learn --locked && cargo test -p blastit --locked && cargo clippy -p blastit --all-targets --locked -- -D warnings` |
| `MB.3.7` | `e2e/php/**`, `e2e/php2shell.py`, `e2e/compose.yaml`, `e2e/run.py`, `e2e/rootless/**` (5 globs) | page fences (`e2e/php/suite/` + `e2e/php/assets/`), `P1`–`P12` (`24` client `E` probes), `e2e/rootless/run.py::`check_fastcgi_pool_dir_is_traversable_but_not_writable_for_the_route_uid` + `check_fastcgi_setsid_escapee_contained` + `check_fastcgi_second_boot_normalises_a_frozen_parent` + `check_fastcgi_b2_foreign_owner_segment_refused` + `check_fastcgi_production_workers_over_budget_exits_1` + `check_fastcgi_entry_0000_refused_for_route_uid` + route-uid `chmod`/`O_CREAT`/`unlink`/`renameat` negatives + cross-route isolation | `make e2e` |
| `MB.3.8` | `Cargo.toml`, `Cargo.lock`, `crates/*/Cargo.toml`, `CHANGELOG.md`, `ARCHITECTURE.md`, `docs/{security.md,decisions/ADR-0007-static-asset-serving-and-rewriting.md,design/static-and-spa.md}`, `scripts/{check-release-tag,release-notes}.sh` (7 path entries / 9 files; the release leaf is exempt from the file cap) | 0.2.1 version bump (first cargo run unlocked, then `--locked`), changelog, `ARCHITECTURE.md` **§2.2 (the `<runtime>/fastcgi/<route_index>/` layout and its three modes)** and §7.3, `docs/security.md`, `static-and-spa.md` §5.1, 1-line `ADR-0007` amendment banner | `CARGO_NET_OFFLINE=true cargo check --workspace && make check && make e2e` |

**Type-ownership matrix (single source of truth for assignments).** A type is defined in
exactly one crate and owned by exactly one leaf; every other leaf that names it is a
**reader** and must depend on that crate (or receive it by value through a pinned seam). A
type defined in a crate a leaf cannot see is a design bug, and a leaf that reads an unowned
file is a path violation.

| Type / trait | Defining crate::module | Owning leaf | Reader leaves |
|---|---|---|---|
| `Upstream`, `UpstreamPlan`, `UpstreamHead` | `blastit-backend` | `MB.9.1.1` | `MB.9.1.2`, `MB.9.2.1.2`, `MB.9.2.2`, `MB.9.3`, `MB.3.6.1.2`, `MB.3.6.2.1`, `MB.3.6.2.2` |
| `RouteId`, `RouteState`, `RoutePermit` | `blastit-backend` | `MB.9.1.1` | `MB.9.2.1.2`, `MB.9.2.2`, `MB.9.3`, `MB.3.6.2.1` |
| `PinnedSocket` | `blastit-backend` | `MB.9.1.1` (also owns `bad_socket_names_are_refused`: `PinnedSocketError::BadName` for empty/`.`/`..`/`/`-bearing names — the structural reason a manifest can never be a socket path) | `MB.9.2.1.1` (`config.rs`), `MB.3.6.2.1`, `MB.3.6.2.2`, `MB.3.6.3.1` (`stage1.rs`) |
| `PoolPort` | `blastit-backend` | `MB.9.1.1` | `MB.9.2.1.2`, `MB.3.6.2.1` |
| `BodySource`, `CancelToken` | `blastit-backend` | `MB.9.1.1` | `MB.9.2.1.2`, `MB.9.2.2`, `MB.9.3`, `MB.3.6.2.1` |
| `Observation`, `ObservationKind` | `blastit-backend` | `MB.9.1.1` | `MB.9.2.2`, `MB.9.3`, `MB.3.6.2.1`, `MB.3.6.4.1` |
| `UpstreamError`, `Refusal`, `ExchangeOutcome`, `ResponseSink` | `blastit-backend` | `MB.9.1.1` | `MB.9.2.1.2`, `MB.9.2.2`, `MB.9.3`, `MB.3.6.2.1`, `MB.3.6.2.2` |
| `BackendBuilder`, `Registry`, `BootError` | `blastit-backend` | `MB.9.1.1` | `MB.9.2.1.2`, `MB.9.2.2`, `MB.9.3`, `MB.3.6.2.1` (`MB.9.2.1.3` only re-exports `builtin_registry`, whose return type is `Registry`) |
| `Capability`, `HeadSpec`, `OutcomeKind`, `PeerScript`, `CancelAt` | `blastit-backend::conformance` | `MB.9.1.2` | `MB.9.4`, `MB.9.2.2`, `MB.9.3`, `MB.3.6.1.2`, `MB.3.6.2.2` |
| `RunError` | `blastit-scheduler::error` | `MB.9.2.1.2` | `MB.9.2.1.3` (`roles.rs`) |
| `FastCgiPolicy` (`entry`, `workers: Option<u32>`), `Executor` | `blastit-manifest` | `MB.3.5.1` | `MB.9.1.1`, `MB.9.2.1.1`, `MB.3.6.2.1`, `MB.3.5.2`, `MB.3.6.3.1`, `MB.3.6.3.2`, `MB.3.6.4.2` (call site only) |
| `resolved(cap)`, `MAX_POOL_WORKERS`, `Manifest::check_fastcgi_workers` | `blastit-manifest` (added by the pool leaf) | **pool leaf, gated on `MB.12`** | `MB.3.6.3.1`, `MB.3.6.3.2`, `MB.3.6.4.2` |
| `PoolSpawn` | `blastit-scheduler::config` | `MB.9.2.1.1` | `MB.9.2.1.2`, `MB.3.6.2.1`, `MB.3.6.3.1` (`stage1.rs`) |
| `SocketProbe`, `SocketProbeError`, `ProbeUid` | `blastit-manifest::probe` | `MB.3.5.2` | `MB.3.6.3.1`, `MB.3.6.4.2` |
| `FastCgiPaths`, `LockdownError` | `blastit-fastcgi::lockdown` | `MB.3.4.2` | `MB.3.6.2.1`, `MB.3.4.2` tests |
| `CgiError`, `RecordError` | `blastit-fastcgi::{cgi,record}` | `MB.3.4.1` | `MB.3.6.2.1` |
| `Session`, `SessionMsg` | `blastit-scheduler::scheduler` | `MB.9.2.1.2` | — (behaviour only; `pub(crate)`) |
| `PoolHarness`, `PoolReadyInfo` | `blastit::supervise::pool_test` | `MB.3.6.3.2` | `MB.3.6.3.3` (distinct from `Ctl::PoolReady`) |
| `PoolConf` | `blastit::supervise` | `MB.3.6.3.2` | `MB.3.6.3.3` |
| `DevProbe`, `ProductionProbe` | `blastit::probe` | `MB.3.5.2` | `MB.3.6.3.1` |
| `Case`, `Class`, `Expect`, `Inject`, `RouteFixture`, `IMPLEMENTATIONS`, `implementation()`, `cases()` | `blastit-backend::conformance` | `MB.9.1.2` | `MB.9.4`, `MB.9.2.2`, `MB.9.3`, `MB.3.6.1.2`, `MB.3.6.2.2` |

Every type in §4.2.1 not listed above (`Capacity`, `UpstreamSignal`, `BodyMode`, `BodyEvent`, `ReplyHead`, `SinkError`, `ProtocolStatus`, `SessionPort`, `ExchangeChannel`, `RouteSpec`, `BuildCtx`, `BuildLimits`, `ObsTarget`, `ParamDecision`, `OutputStream`) is defined in `blastit-backend` and owned by `MB.9.1.1` (the `Lands` catch-all).

**Docs.** `crates/blastit-scheduler/src/lib.rs` is `#![deny(missing_docs)]`, so every new
public item (`PoolSpawn`, the four `SchedulerConfig` fields, `run_with`, `builtin_registry`,
`RunError` and its variants, `SchedulerError::Boot`) carries a `///` doc comment on its
definition; `blastit-backend` and `blastit-manifest` are likewise `#![deny(missing_docs)]`.

**Test lints.** Every leaf that adds a test file is covered by `make check`'s workspace
clippy (`cargo clippy --workspace --all-targets --locked -- -D warnings`, `Makefile:86`);
the four FastCGI suites (`MB.9.2.2`, `MB.9.3`, `MB.3.6.2.2`, `MB.3.6.3.3`, `MB.3.6.4.2`)
also run it locally. New suites carry `#![allow(clippy::unwrap_used,
clippy::expect_used)]` exactly like the existing ones (the workspace sets both to `warn`).

**Crate-edge matrix (second source of truth).** One row per workspace edge the design
requires; the owning leaf edits that `Cargo.toml` (and, where it creates the crate, owns
`Cargo.lock`), and the acceptance command proves the edge exists.

| Edge | `Cargo.toml` | Owning leaf | Acceptance |
|---|---|---|---|
| `blastit-backend → blastit-http`, `blastit-manifest`, `serde = { version = "1.0.229", features = ["derive"] }` | `crates/blastit-backend/Cargo.toml` | `MB.9.1.1` (owns `Cargo.lock`) | `cargo tree -p blastit-backend -e normal &#124; grep -q blastit-http` (and `blastit-manifest`; `serde` with `derive`) |
| `blastit-fastcgi → (none in production)` | `crates/blastit-fastcgi/Cargo.toml` | `MB.3.4.1` (owns `Cargo.lock`) | `test -z "$(cargo tree -p blastit-fastcgi -e normal --prefix none &#124; tail -n +2)"` |
| `blastit-fastcgi [dev-dependencies] → blastit-backend` | `crates/blastit-fastcgi/Cargo.toml` | `MB.3.6.1.2` | `cargo test -p blastit-fastcgi --test conformance` |
| `blastit-scheduler → blastit-backend`, `blastit-http` | `crates/blastit-scheduler/Cargo.toml` | `MB.9.2.1.1` (owns `Cargo.lock`) | `cargo tree -p blastit-scheduler -e normal &#124; grep -q blastit-http` (and `blastit-backend`) |
| `blastit-scheduler → blastit-fastcgi` | `crates/blastit-scheduler/Cargo.toml` | `MB.3.6.1.2` | `cargo tree -p blastit-scheduler -e normal &#124; grep -q blastit-fastcgi` |
| `blastit-scheduler → blastit-frame`, `blastit-ingress`, `blastit-manifest` (existing) | `crates/blastit-scheduler/Cargo.toml` | unchanged | `cargo tree -p blastit-scheduler -e normal &#124; grep -q blastit-frame` |
| `blastit → blastit-backend` | `crates/blastit/Cargo.toml` | `MB.3.6.1.2` | `cargo tree -p blastit -e normal &#124; grep -q blastit-backend` |
| `blastit → blastit-fastcgi` | `crates/blastit/Cargo.toml` | `MB.3.6.1.2` | `cargo tree -p blastit -e normal &#124; grep -q blastit-fastcgi` |
| `blastit → blastit-scheduler` (existing) | `crates/blastit/Cargo.toml` | unchanged | `cargo tree -p blastit -e normal &#124; grep -q blastit-scheduler` |
| `blastit-manifest → no rustix/libc` | `crates/blastit-manifest/Cargo.toml` | unchanged; asserted by `MB.3.5.1`'s acceptance (no leaf edits the file) | `! grep -qE 'rustix&#124;libc' crates/blastit-manifest/Cargo.toml` |
| `blastit-frame → (no new edge)` | `crates/blastit-frame/Cargo.toml` | unchanged (`MB.9.2.1.1` adds only `ReqHeadView::scheme() -> Scheme`) | `! grep -q blastit-backend crates/blastit-frame/Cargo.toml` |
| `blastit-http → (no blastit-backend)` | `crates/blastit-http/Cargo.toml` | unchanged | `! grep -q blastit-backend crates/blastit-http/Cargo.toml` |

**Derive matrix (third source of truth).** A missing derive is a compile error in the
*consumer*, not the definer, so every type on the observation/`CheckObsV1` path is pinned:

| Type | Derives | Serde attributes | Whose `derive` breaks if missing |
|---|---|---|---|
| `Observation` | `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` | `#[serde(flatten)] kind` | `CheckObsV1` (`PartialEq, Eq`) |
| `ObservationKind` | `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` | `#[serde(tag = "kind", rename_all = "snake_case")]` | `Observation` flatten |
| `ObsTarget` | `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` | `#[serde(tag = "scope", rename_all = "snake_case")]` | `Observation` |
| `ParamDecision` | `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize` | — | `ObservationKind::Param` |
| `OutputStream` | `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize` | — | `ObservationKind::Output` |
| `WireRequest` variant | — | `#[serde(default = "default_true")] executor_ran` | the pinned learn line |
| `CheckObsV1` | `Debug, Clone, PartialEq, Eq` (existing; no serde derive) | — | `check`/`learn` comparisons |
| `UpstreamHead` | `Debug, Clone, PartialEq, Eq` | — | `UpstreamPlan`/test equality |
| `Tls` | `Debug, Clone, Copy, PartialEq, Eq` | — | `UpstreamHead` |
| `RouteId` | `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash` | — | `RouteSpec`/session maps |
| `PinnedSocket` | `Clone, Debug` (**not** `PartialEq`/`Eq`) | — | `SchedulerConfig: Clone` |
| `PoolConf` | `Debug, Clone, Copy` | — | `render_pool_conf` callers |
| `Executor`, `FastCgiPolicy` | `Debug, Clone, PartialEq, Eq` | — | `RouteInfo` |
| `PoolSpawn` | `Debug, Clone, Copy, PartialEq, Eq` | — | `SchedulerConfig` |
| `Capability` | `Clone, Copy, PartialEq, Eq, Debug` | — | `IMPLEMENTATIONS` |

**Budgets.** Changed lines (`git diff --numstat`): every leaf above is sized ≤ 7 files and
≤ 550 changed lines; each new `src/upstream/<kind>.rs` is ≤ 500 production lines.

### 8.1 Executable `tools/plan.py` checklist before dispatch

Before dispatching any `MB.3.4+` or `MB.9.*` leaf, `planner`/`lead` applies this exact list
against the current `plan/nodes/MB/**/NODE.md` tree (never adding an existing protected
`critical_*.rs` file to any node's `paths`). **Until that is applied, §8.1/§8.2 are the
authoritative gate: the `NODE.md` files on disk are pre-§8.1, and nobody implements from a
stale brief.**

1. **Sub-splits for the ≤ 7-file / ≤ 550-line cap.** Convert every parent to an epic
   **before** the first `new`: `uv run --no-project tools/plan.py --agent planner edit <parent>
   --kind epic` (a task with children is rejected with "convert it with `edit <id> --kind
   epic` first"), then (every parent in the mapping: `MB.3.4`, `MB.3.5`, `MB.3.6`, `MB.9`,
   `MB.9.1`, `MB.9.2`, `MB.9.2.1`, `MB.3.6.1`, `MB.3.6.2`, `MB.3.6.3`, `MB.3.6.4`):
   - Split `MB.3.5` into `MB.3.5.1` (`depends_on: [MB.3.4]`) and `MB.3.5.2` (`depends_on: [MB.3.5.1]`), each with **exactly the `Files` cell of §8's row for that id**; `MB.3.4.2` gets `depends_on: [MB.3.4.1]`.
   - Split `MB.9.1` into `MB.9.1.1` (`depends_on: [MB.3.3, MB.3.4, MB.3.5.1]`, owns `Cargo.lock`) and `MB.9.1.2` (`depends_on: [MB.9.1.1]`, no `Cargo.lock`), each with **exactly the `Files` cell of §8's row for that id**.
   - Split `MB.9.2.1` into `MB.9.2.1.1` (`depends_on: [MB.9.1]`, owns `Cargo.lock`), `MB.9.2.1.2` (`depends_on: [MB.9.2.1.1]`, no `Cargo.lock`) and `MB.9.2.1.3` (`depends_on: [MB.9.2.1.2]`, no `Cargo.lock`), each with **exactly the `Files` cell of §8's row for that id**; re-point `MB.9.2.2` `depends_on: [MB.9.2.1.3]`.
   - Split `MB.3.6.1` into `MB.3.6.1.1` (`depends_on: [MB.3.5, MB.9]`, no `Cargo.lock`) and `MB.3.6.1.2` (`depends_on: [MB.3.6.1.1]`, owns `Cargo.lock` + `Makefile`), each with **exactly the `Files` cell of §8's row for that id**; re-point `MB.3.6.2.1` `depends_on: [MB.3.6.1.2]`.
   - Split `MB.3.6.2` into `MB.3.6.2.1` (`depends_on: [MB.3.6.1.2]`) and `MB.3.6.2.2` (`depends_on: [MB.3.6.2.1]`, owns `Makefile`), each with **exactly the `Files` cell of §8's row for that id**; re-point `MB.3.6.3.1` `depends_on: [MB.3.6.2.1]`.
   - Split `MB.3.6.3` into `MB.3.6.3.1` (`depends_on: [MB.3.6.2.1]`), `MB.3.6.3.2` (`depends_on: [MB.3.6.3.1]`) and `MB.3.6.3.3` (`depends_on: [MB.3.6.3.2]`), each with **exactly the `Files` cell of §8's row for that id**.
   - Split `MB.3.6.4` into `MB.3.6.4.1` (`depends_on: [MB.3.6.3.3]`) and `MB.3.6.4.2` (`depends_on: [MB.3.6.4.1]`), each with **exactly the `Files` cell of §8's row for that id**.
2. **Exact `paths` adjustments on existing nodes:**
   - `MB.3.4.2`: add `crates/blastit-fastcgi/src/lib.rs` (`pub mod lockdown;`).
   - `MB.9.2.2`: add `crates/blastit-scheduler/src/upstream/mod.rs` (`pub mod static;`); `MB.9.2.1.2` declares `mod upstream;` in `lib.rs` (crate root) and owns `lib.rs`; `.3` only appends `pub use` lines.
   - `MB.3.5.2`: add `crates/blastit/src/supervise.rs` (narrow `Parts`/`Supervisor`/`new` to `pub(crate)`); the clippy gate belongs to `MB.3.6.3.2`, not here.
   - `MB.9.2.1.1`: add `crates/blastit/src/stage1.rs` (it populates `route_lines`/`pool_spawn`/`pinned_sockets`/`http_obs`; the literal ends in `..SchedulerConfig::default()`) and `cargo test -p blastit --locked` to its acceptance; 7 paths, no drop needed.
   - `MB.9.3`: narrow `crates/blastit-scheduler/src/**`, `crates/blastit-view/**`, `crates/blastit/src/**` to `crates/blastit-scheduler/src/{upstream/wsgi.rs,upstream/mod.rs,scheduler.rs,lib.rs}` and `crates/blastit-scheduler/tests/conformance_wsgi.rs`.
   - `MB.3.6` (parent) + `MB.3.6.1.2` + `MB.3.6.2.2`: add `Makefile` so both FastCGI conformance drivers wire into `rs-test-conformance`.
   - `MB.3.6.1.1`: add `crates/blastit-scheduler/tests/common/mod.rs` **and** `crates/blastit/src/supervise.rs` (both exhaustive `Ctl` matches), and add `cargo test -p blastit --locked` to its acceptance.
   - `MB.3.5.2`: add `crates/blastit/src/probe.rs` (the probe implementations; `blastit-manifest` stays `rustix`-free) **and** `crates/blastit/src/lib.rs` (its single owner: `pub mod probe;` + `pub mod supervise;`).
   - `MB.3.6.3.1`/`MB.3.6.4.2` do **not** import `smoke.rs` (a separate test binary); each suite duplicates the `children()`/`fds()` helpers locally, precedent `critical_dev_and_fail_closed.rs:482`.
   - `MB.3.6.3.1` owns `cli.rs` for the internal arm; `MB.3.6.4.2` later edits it for `CheckOptions` (serialized: `.2` `depends_on` `.1`), so there is no conflict. Add `crates/blastit/src/{cli,dev_learn}.rs` **and** `crates/blastit/tests/critical_fastcgi_parent_fd.rs`; `MB.3.6.3.2`: add `crates/blastit/tests/support/fake_php_fpm.rs` (it does **not** edit `lib.rs`; `MB.3.5.2` already made `supervise` public); `MB.3.6.3.3` keeps `critical_fastcgi_pool.rs`.
   - `MB.3.8`: add `docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md` (for the 1-line `Amended by ADR-0008` banner) alongside `docs/security.md` and `docs/design/static-and-spa.md`.
3. **Fallback splits for the remaining near-cap leaves** (each is **two numeric `new` children**, e.g. `MB.9.2.1.2.1`/`MB.9.2.1.2.2` — `plan.py`'s `ID_RE` rejects letter suffixes): `MB.9.2.1.2` → `.1` (`UpstreamHead` + `RouteRuntime.stages` + `ViewHub`/`SessionPort`) + `.2` (session task/`SessionDoneGuard` + `http_obs` writer + `run_with`/`RunError` + `builtin_registry()` + `run` + `BuildCtx.external_executor` + the transitional `stages.is_empty()` path); `MB.3.6.3.2` → `.1` (`render_pool_conf` + `PoolConf` + `pool_test` harness) + `.2` (`S0`–`S7`/`U1` + fake helper + smoke test); `MB.3.6.2.1` → `.1` (`fastcgi.rs` pump) + `.2` (`fastcgi_builder.rs`: `FastCgiPaths`, mapping, `ensure_ready`/`on_ctl`/registration; that file is in `.1`'s paths, so this split adds no path); `MB.3.5.2` → `.1` (`blastit-manifest/src/probe.rs` + `lib.rs` `pub mod probe;` + protected suite) + `.2` (`crates/blastit/src/probe.rs` + `lib.rs` `pub mod supervise;` + `supervise.rs` narrowing + `manifest.md`); `MB.3.6.3.1` → `.1` (`B1`–`B3`, `config`, `cli`) + `.2` (`stage1` handoff, `dev_learn`, entry call, parent-fd suite); `MB.9.2.2` → `.1` (`StaticUpstream`) + `.2` (deleted branches + `"static"` registration); `MB.9.3` → `.1` (`WsgiUpstream`) + `.2` (`ExchangeChannel` + registration); `MB.9.1.1` → `.1` (crate + `types.rs`, owns `Cargo.lock`) + `.2` (`registry.rs`/`lib.rs`, no lock). `MB.9.2.1.2`/`.3` and `MB.3.6.3.2`/`.3` are **already split unconditionally** (item 1), not fallbacks. Each split is mechanical and changes no path.

4. **Amendment sync (`MB.3.9.2`, 2026-11-14).** Every leaf below lands the amendment in the
   same change as its original scope; none of them re-opens a design question.
   - `MB.3.5.1` (**landed: grammar only**): `FastCgiPolicy { entry: String, workers: Option<u32> }`
     (**no `listen`**), `FastCgiListenDerived`, `FastCgiDeadDeclaration`, and deletion of
     `FastCgiListenInvalid` / `FastCgiListenParentInvalid`; the parser keeps the landed
     `FastCgiListenDerived` arm (it carries no value) and loses `!has_listen` `MissingKey`
     and `DEFAULT_WORKERS = 4`; `model.rs`'s doc comment names the derived socket. **Not
     this leaf's:** `resolved_workers`, `MAX_POOL_WORKERS`, `check_fastcgi_workers`,
     `FastCgiWorkersOverBudget`/`FastCgiWorkersCapExceeded` — they belong to the pool leaf
     below.
   - **Pool leaf (gated on `MB.12`)** — the pool path is frozen pending the embed-vs-pool
     decision, so these land with whichever leaf implements that mechanism (today
     `MB.3.6.3.1`/`MB.3.6.3.2`), in a change that also touches
     `crates/blastit-manifest/src/{model,error}.rs`: the `resolved(cap)` helper,
     `MAX_POOL_WORKERS`, `Manifest::check_fastcgi_workers` and its three ordered arms, the
     two worker-budget variants, and their oracles `workers_at_budget_is_accepted`,
     `workers_over_budget_is_boot_fatal` and `absent_workers_under_a_512_budget_is_boot_fatal`
     (plus `resolved_workers_table`). `workers` keeps its grammar meaning regardless; under
     an embedded SAPI `MB.12` may retire the resolution, the ceiling and the variants and
     leave the key inert. That leaf also fixes `model.rs`'s "the derivation is the
     scheduler's" comment to match §4.8.1.
   - `MB.3.5.2`: `ManifestError::FastCgiPoolSocketInvalid { location, path, rule }` in
     `crates/blastit-manifest/src/error.rs` (added to its files; the three `dev` refusal legs
     below need it), `SocketProbe::parent(run_root, route_index)` + `check_leaf(parent, uid)`,
     `check_fastcgi_sockets(run_root, probe)`, `DevProbe` creating and owning
     `<run_root>/fastcgi/<i>` (`0700`); the suite rows listed in §7.2; `manifest.md` item 2
     with **no** socket key.
   - `MB.3.6.3.1`: `stage0` calls `check_fastcgi_workers(config.view_worker_cap())`
     (production **and** `dev`) before any route process exists, runs `B1`/`B2` against
     `<runtime>` (not `manifest_dir`), and passes the run root to `DevProbe`.
   - `MB.3.6.3.2`: `render_pool_conf` writes `listen=<derived>` and `pm.max_children=<resolved>`
     (a conf file cannot carry an rlimit), and a separate `#[doc(hidden)] pub fn
     pool_rlimits(resolved: u32) -> Rlimits` is the one place `RLIMIT_NPROC=<resolved>+2` and
     `RLIMIT_NOFILE=max(1024, <resolved>*4+32)` are computed for `ChildIo.limits`.
   - `MB.3.6.4.2`: `critical_fastcgi_learn::{dev,check}_exits_n_when_workers_exceed_budget`,
     `dev_exits_1_under_a_512_budget` and
     `check_child_leaf_failure_exits_2_naming_the_derived_socket`; `check::execute` calls
     `check_fastcgi_workers`, then phase 1
     `check_fastcgi_sockets(&dev_root, …)` **before** `serve_and_run` (the returned fds are
     what it `clear_cloexec`s for the child); `MB.3.9.3` runs phase 2 `check_fastcgi_leaf`
     once that child's own pool reports ready — the failure `check` observes is the child
     `dev` exiting `1`, which `check` maps to `exit 2` (R20).
   - `MB.3.7`: replace the hand-written-pool/`listen` e2e legs with the derived-parent
     refusal and the traversable-but-not-writable pool-directory legs of §7.2; the **generated** pool file
     is the fence.
   - `MB.3.9.5` (dev pool suite) asserts `leaf_missing_after_spawn_exits_1_in_dev` and the §1.2 line: `dev_prints_resolved_pool_workers`
     (env-derived and explicit legs, exact stderr substring, once per FastCGI route, stdout
     still exactly three lines).
   - `MB.3.9.1` (page) and `MB.3.9.3`/`.4`/`.5` (dev pool lifecycle) are the sibling nodes;
     this document is their input, and none of them may reintroduce a socket key.

### 8.2 Brief re-sync before dispatch

| Plan node | Stale text on disk | Required brief sync |
|---|---|---|
| `MB.3.3` | `plan(&RequestHead)`; `RouteInfo` holds `upstream`; `kind()` called "the manifest backend name, for events only" | `plan(&UpstreamHead)`; handle in `RouteRuntime.stages` beside `RouteInfo`; `kind()` is "for diagnostics and tests only; never logged, the `request` event has no backend field" |
| `MB.3.3`, `MB.9.1.1`, `MB.9.2.1.2`, `MB.3.6.2.1` | briefs pin `exchange(..., body: BodyStream<'a>, sink: &'a mut dyn ResponseSink)` and stale `ExchangeOutcome`/`Observation` shapes | `exchange(&self, plan: UpstreamPlan, body: Box<dyn BodySource + Send>, sink: Box<dyn ResponseSink + Send>) -> BoxFuture<'_, Result<ExchangeOutcome, UpstreamError>>`; `ExchangeOutcome::{Answered{status,bytes_in,bytes_out,protocol_status,dropped,observations}, Declined{next,plan}}`; `Observation { target, #[serde(flatten)] kind }` with `ObservationKind` internally tagged; `Scheduler::session(..., permit: RoutePermit)` is `pub(crate)` |
| `MB.3.6` (parent) | `check -- <cmd>`; "in-process fake peers (socketpair …)" | `blastit check -- <suite>`; the fake peer is a hand-rolled temp-dir `UnixListener` wrapped in `PinnedSocket` (never a `socketpair`) |
| `MB.3.6.3` (`.1`/`.2`/`.3`) | "§4.4 (`can_boot_without_app`)" | that predicate is §4.2/§8; §4.4 is the WSGI port |
| `MB.3.4.1` | CGI head omits proxy-control header denial | `cgi.rs` refuses exactly `X-Accel-Redirect`, `X-Sendfile`, `X-Lighttpd-Send-File`, `X-Rewrite-URL` (`Fault`) and **drops** every other `X-Accel-*` name (`X-Accel-Buffering` → `200`, header absent) |
| `MB.3.4.2` | lists row 18 as client-status; no `src/lib.rs`; returns `Refusal` (plan `paths` drift is fixed by §8.1 item 2) | row 18 is internal builder `500` (`LockdownError::UnknownParam`, mapped by `MB.3.6.2.1`); colliding `HTTP_*` headers keep first (`200`, `Param { Dropped }`); `MB.3.4.2` owns `src/lib.rs` and adds `pub mod lockdown;`; `FastCgiPaths` is defined in `lockdown` |
| `MB.3.5` (`.1`/`.2`) | lists `render_pool_conf` here; `manifest.md` only `:142-145`; `Executor`/`FastCgiPolicy` not re-exported | `render_pool_conf` lives in `supervise.rs` (`MB.3.6.3.2`); `manifest.md` applies all 3 §4.1 edits; `.1` adds `Executor`/`FastCgiPolicy` to `lib.rs`'s `pub use model::{…}` (root path `blastit_manifest::Executor`) and a `lib.rs` doctest proves it; `.2` also owns `crates/blastit/src/{probe,lib,supervise}.rs` (`pub mod probe;` + `pub mod supervise;`; `Parts`/`Supervisor`/`new` become `pub(crate)`) |
| `MB.9.1` (`.1`/`.2`) | `depends_on: [MB.3.3, MB.3.4]`; "keyed by manifest backend name"; first cmd `--locked`; no `PinnedSocket`/`RouteId`/`RouteState`/`executor_ran` | add `MB.3.5.1` to `MB.9.1.1` `depends_on`; single source is `Executor::kind()`; first build runs without `--locked` then `--locked`; add `RouteId`, `RouteState` (`try_acquire(self: &Arc<Self>)`), `PinnedSocket::new(Arc<OwnedFd>)`/accessors, `UpstreamSignal::ExecutorReady { socket_inode }`, `WireRequest.executor_ran`; fallback split `.1a`/`.1b` if over 550 lines |
| `MB.9.2.1` (`.1`/`.2`/`.3`) | 11 files; first cmd `--locked`; `stage1.rs` vs `roles.rs` root; no `SessionDoneGuard` or proxy-control strip; no `mod upstream;` site | split `.1`/`.2`/`.3`; first build in `.1` runs without `--locked` then `--locked`; `.1` adds `SchedulerConfig.route_lines` and `crates/blastit/src/stage1.rs` (the `SchedulerConfig` literal) with `cargo test -p blastit --locked`; `.2` adds `src/error.rs` (`RunError`, `SchedulerError::Boot`), `run_with`, **`builtin_registry()` and `run` delegating to `run_with`**, **`mod upstream;` declared in `lib.rs`** (and owns `lib.rs`), `BuildCtx.external_executor`, the transitional `stages.is_empty()` path, `SessionDoneGuard` (`Drop` → `Done(Fault)`) + `ResponseSink` proxy-control denial; `.3` only re-exports `builtin_registry` from `lib.rs` and adds the `roles::scheduler` `RunError` print |
| `MB.9.2.2` | "`plan` = today's method/body gate plus `head_segments`" | `plan` = `head_segments` + `.php`/`.phar` `EntryScript` `404` (`BodyMode::None`); method `405` and body `413` stay in `exchange` via `rewrite::decide` on `StaticOutcome::File`; registers `"static"`; owns `upstream/mod.rs` and adds `pub mod static;`; `conformance_static.rs` includes the fake via `#[path = "common/fake_upstream.rs"] mod fake_upstream;` |
| `MB.9.3` | broad `src/**` globs | narrow `paths` to §8's 5 files; registers `"wsgi"` in `builtin_registry()`; **deletes the transitional `stages.is_empty()` fallback** (missing builder → `RunError::Boot`) |
| `MB.9.4` | generic `IMPLEMENTATIONS` check | exact 5-entry `assert_eq!(IMPLEMENTATIONS, …)` + per-class `Expect` completeness assertions |
| `MB.3.6.1` (`.1`/`.2`) | 9 files; `PoolReady { route }` only; first cmd `--locked`; no `Makefile` | split `.1`/`.2`; `PoolReady { route, sock_dev, sock_ino }`; `.1` also owns `crates/blastit/src/supervise.rs` (production `Ctl` match) and its acceptance builds `-p blastit`; `.2` runs unlocked once to write `Cargo.lock` and appends `blastit-fastcgi --test conformance` to `Makefile` |
| `MB.3.6.2` (`.1`/`.2`) | "(canonicalised, R2-checked)"; "in-process socketpair fake peer"; no `Makefile` | `.1` = session pump (`FastCgiPaths` from `openat2`-checked relative segments, **never** `canonicalize()`; `PinnedSocket` `/proc/self/fd` connect + `C1` + exact `SO_PEERCRED`; `external_executor` early return; `"fastcgi"` registered). `.2` = suites + `Makefile`: fake peer is a `TempDir` `UnixListener` wrapped in a `PinnedSocket` built via `PinnedSocket::new` (never a `socketpair`, never a path string); 5-binary `rs-test-conformance` check + post-ready parent-rename sentinel + row 25 `504`/`ABORT_REQUEST` + panic/non-UTF-8 + `foreign_uid_peer_is_fault_502` |
| `MB.3.6.3` (`.1`/`.2`/`.3`) | `BLASTIT_CHECK_HTTP_OBS`/`SQL_OBS_FD` only; no `DEV_ROOT` or `obs_fd` close; paths list a stale `zygote.rs` | `.1` = `B1`–`B3`, `render_pool_conf` is in `.2`, `BLASTIT_CHECK_*` (`Dev::adopt` already at `dev_learn.rs:180`; `bind_all` at `slot_offset = 0`), close `check_sql_obs_fd` after `fork_dbproxy_dev`, `CAP_CHOWN` boot error, `host=<runtime>/views/<uid>` + private `TMPDIR`, the `parentfd` handoff (`--fastcgi-parent-fd` in `exec_stage1`/`exec_stage1_dev`, `cli.rs` internal arm only with `--stage=1` and invisible in `--help`, `Options.fastcgi_parent_fds` validated **before** `OwnedFd`, `Arc<OwnedFd>` shared with init's pool state, `ChildIo.keep` via `pinned_socket_fds()`, `Config::for_dev()` reading `POOL_PARENT_FD_ENV` (`pub(crate) const` in `config.rs`) before `BLASTIT_*` stripping (`MB.3.6.4.2` aliases it as `check::POOL_PARENT_FD`), `Config::to_json` excludes it, `B1` `O_NOFOLLOW`+`lstat`). `.2` = `render_pool_conf`, `S0`–`S7`/`U1` (parent-side `setpgid`, `S1` reap-to-`ECHILD`, `S4`/`S6` pgid-membership, freeze-before-unlink `U1`, `forking::fork(true)`), `HOME=<runtime>/views/<uid>/tmp`, `pm.max_requests=max(BLASTIT_VIEW_MAX_REQUESTS, 500)`, the fake `php-fpm` helper (`tests/support/fake_php_fpm.rs`), the `tests/fake_php_fpm_smoke.rs` one-line include, and the inline `#[doc(hidden)] pub mod pool_test` harness; `.3` duplicates the `children()`/`fds()` helpers locally (no `smoke.rs` import); `.3` = `critical_fastcgi_pool.rs` (the `S0` fd-table / `setsid()` escapee / pgid-membership / `U1` suite) |
| `MB.3.5` (`.1`/`.2`) — **amended 2026-11-14, re-scoped 2026-11-14 (attempt 2)** | a `listen` field on `FastCgiPolicy`, a `listen` arm plus `!has_listen` `MissingKey` in `parse::fastcgi_policy`, `FastCgiListenInvalid`/`FastCgiListenParentInvalid`, `SocketProbe::check(manifest_dir, listen, route_index)`, `check_fastcgi_sockets(manifest_dir, probe)`, `workers: u32` with `DEFAULT_WORKERS = 4`; **(attempt 2)** a brief that also demands `resolved_workers`/`MAX_POOL_WORKERS`/`check_fastcgi_workers`/`FastCgiWorkersOverBudget`/`FastCgiWorkersCapExceeded` | drop the field, the two variants and `DEFAULT_WORKERS`; **keep** the landed `FastCgiListenDerived` arm (it carries no value) and lose only `!has_listen` `MissingKey`: `FastCgiPolicy { entry, workers: Option<u32> }` (**only** the optional value; the resolution, ceiling, check and two variants are the pool leaf's, **gated on `MB.12`** — not `MB.3.5.1`'s); `parent(run_root, route_index)`; `check_fastcgi_sockets(run_root, probe)`; this epic's `.1` lands grammar only and `.2` lands `FastCgiPoolSocketInvalid` (plus `crates/blastit-manifest/src/error.rs` in its files); the **pool leaf, gated on `MB.12`**, adds `Manifest::check_fastcgi_workers(budget)`, `FastCgiWorkersOverBudget`/`FastCgiWorkersCapExceeded` and their oracles; §7.2 names the suite rows |
| `MB.3.5.2` — **amended 2026-11-14** | brief still requires `shared_listen_parent_is_boot_fatal`, `ParentWritableOrSymlink`, `SocketProbe::check(manifest_dir, listen, route_index)` | delete all three; the suite's rows are **exactly** §7.2's `critical_fastcgi_manifest.rs` inventory (9 names, quoted there) plus the fake-probe legs for `parent`/`check_leaf`; `SocketProbeError` loses `ParentWritableOrSymlink` and gains `NotDirectory`/`InsecureMode`/`NotConnectable` |
| `MB.3.9.5` — **sibling node (new)** | brief does not name the amendment's two oracles | its protected suite `crates/blastit/tests/critical_fastcgi_dev_pool.rs` must include `dev_prints_resolved_pool_workers` (three legs) and `dev_starts_one_pool_per_route` (two routes → two masters in two `<route_index>` dirs, none after `SIGTERM`); §7.1 lists the file |
| `MB.3.9.3`/`MB.3.9.4` — **sibling nodes (new)** | briefs say `<run>/<route>/fpm.sock` (two segments), a project-local `.blastit/run`, and `dev`/`check` `External` | the normative path is `<run_root>/fastcgi/<route_index>/fpm.sock` with `<route_index>` = `Manifest::routes` index; `<run_root>` is whatever blastit-created root the provider uses (`Dev::adopt`'s, or `.blastit/run` if that node chooses it — its call); `dev` and `check`'s child `dev` are `PoolSpawn::Managed`, `dev` starts/stops the pool it owns, phase 1 of §4.8.1 runs before the pool and phase 2 after it, and `dev_starts_one_pool_per_route` (`MB.3.9.5`) replaces `external_pool_never_spawns_php_fpm` |
| `MB.3.6.3`/`MB.3.6.4` — **amended 2026-11-14** | `B1`/`B2` resolve a manifest parent from `manifest_dirfd`; `render_pool_conf` takes a manifest `listen`; `RouteState::new(…, workers)`; `check` probes a manifest socket | `B1`/`B2` anchor on `<runtime>` and create `<runtime>/fastcgi/<i>`; `render_pool_conf`/`RouteState::new` use `resolved_workers(view_worker_cap())`; `stage0` and `check::execute` call `check_fastcgi_workers`; who starts the `dev` pool is `MB.3.9.3` |
| `MB.3.6.4` (`.1`/`.2`) | unsplit; no `BLASTIT_CHECK_DEV_ROOT`; `-- <command>` | split `.1`/`.2`; `<CheckRoot>/dev` handoff via `BLASTIT_CHECK_DEV_ROOT`; `check::POOL_PARENT_FD` (`BLASTIT_CHECK_POOL_PARENT_FD`): thread the `Vec<(usize, OwnedFd)>` from `check_fastcgi_sockets`, `clear_cloexec` immediately before spawning child `dev`; `blastit check -- <suite>`; `executor_ran` exercise count |

---

## 9. Review Resolution

### Round history (truthful state on disk)

1. **Round 1 (cheap `designer` draft + 4 critics):** All four `*.review-*.md` files returned
   `VERDICT: FAIL` (Security: 1 BLOCKER, 2 MAJOR, 5 MINOR; Implementability: 2 MAJOR,
   12 MINOR; Testability: 3 MAJOR, 11 MINOR; Consistency: 2 MAJOR, 5 MINOR).
2. **Round 2 (`expert` Escalation #8 + planner split + Round-2/3 critics):** Resolved the
   Round-1/2 `fchownat` owner+mode freeze BLOCKER, ancestor scope, 3-way `check` predicate
   preserving `critical_static_cli` `MIXED`, and ADR-0007 amendments, but Round-3 critics
   (`security` 0/3/10, `implementability` 0/6/5, `testability` 0/3/5) and Round-2
   `consistency` (0/4/8) still ended `VERDICT: FAIL` on disk.
3. **Architect pass (Escalation #9, current):** Resolved every remaining MAJOR and MINOR
   across all four lenses in `docs/design/php-fastcgi.md`, `ADR-0008`, and
   `docs/design/php-fastcgi.resolve-architect.md`. The four `*.review-*.md` files on disk
   intentionally retain their pre-architect `VERDICT: FAIL` tails until `lead`/`conductor`
   runs the fresh-context Round-4 critic gate.
4. **Round 4 (`security`, fresh context):** 0 BLOCKER, 2 MAJOR (**MAJOR-A** the `dev`/`check`
   `parentfd` is `O_CLOEXEC` and cannot cross the stage-0→stage-1 `execve`, with no handoff
   specified; **MAJOR-B** `S0`'s `route_uid` `php-fpm -t` child inherited `ctl.chan`/
   `parentfd`, falsifying `S7`), 10 MINOR (`19`–`28`). All twelve are resolved in this pass
   (table below); `docs/design/php-fastcgi.review-security.md` keeps its `VERDICT: FAIL`
   tail until the lens is re-run.
5. **Round 5 (`security`, fresh context):** 0 BLOCKER, 2 MAJOR (**MAJOR-A residual** —
   descriptor ownership after `fork_scheduler`, no `cli.rs` owner, duplicate validation after
   `OwnedFd::from_raw_fd`, `SocketProbe` returns no descriptor; **MAJOR-C** — the `S2` bind
   window lets a `setsid()` escapee rebind and be published, and the "different inode" oracle
   passes on the attack), 9 MINOR (`29`–`37`). The first pass resolved **MAJOR-A, MAJOR-C and
   MINOR-29/30** (first table below); a follow-up pass fixed **MINOR-31..37** (second table
   below), so every Round-5 finding is now resolved.

6. **Round 4 (`implementability`, fresh context, parallel to the security rounds):** 12 MAJOR
   (`M1`–`M12`: `BootError.line`, `run_with`/`RunError`, `PinnedSocket` API, `BuildCtx`
   `External`, `RoutePermit` lifetime, probe crate deps, `Ctl` test match, `peer_gid` typo,
   `MB.3.6.3` paths, leaf budgets, conformance types, transitional registry) plus six
   one-line MINORs. All are resolved in the table below; the review file keeps its
   `VERDICT: FAIL` tail until the lens is re-run.

### Disposition of Round-3 `security`, `implementability`, `testability` & Round-2 `consistency` findings

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `security` R1/R2 BLOCKER-1 & MAJOR-2 | BLOCKER / MAJOR | **Closed (verified in R3 security §1, hardened in §4.8.3 `B1`–`B3`, `S0`–`S7`, `U1`, `C1` & ADR-0008 §6).** Walk from `/` → `manifest_dirfd` → `parentfd` (`O_RDONLY\|O_DIRECTORY`, `openat2(RESOLVE_BENEATH\|NO_SYMLINKS\|NO_MAGICLINKS)`), non-route-writable ancestors, `AT_SYMLINK_NOFOLLOW` socket verify/chown/chmod `0600`, restore **both** `init_uid:init_gid` and `0555`, post-freeze inode re-verify, `Ctl::PoolReady { route, sock_dev, sock_ino }`, and full unwind `U1` on every failure. |
| `security` MAJOR-3 / `implementability` M1 | MAJOR | **Accepted → §4.2.1 (`PinnedSocket`, `BuildCtx.socket`, `UpstreamSignal::ExecutorReady { socket_inode }`), §4.5.2, §4.8.1, §4.8.3 `C1`.** Added server-agnostic `PinnedSocket` carrying `Arc<OwnedFd>` `parentfd`, `(parent_dev, parent_ino)`, `sock_name` and `bound_inode`, threaded via `SchedulerConfig.pinned_sockets` in both production and `dev`/`check`; deleted "pool paths are opened by the role that uses them"; keyed `SessionPort` by `RouteId`. |
| `security` MAJOR-4 | MAJOR | **Accepted → §4.2.1, §4.4, §4.5.2, §7.2.** Pinned `String::from_utf8_lossy` on every wire/pool-to-`String` conversion and added `SessionDoneGuard` + `RoutePermit` `Drop` guards so a panicking session task synthesises `Done(Fault)` once and never strands a `BLASTIT_MAX_WORKERS` slot; added non-UTF-8 and panic tests to `critical_fastcgi_dispatch.rs`. |
| `security` MAJOR-6 | MAJOR | **Accepted → §4.2.1 (`ResponseSink::head`), §4.5.1, §4.5.2, §4.7 row 24, §4.11 (`forbidden_response_header`), §5.1, §7.2.** Denied `X-Accel-*` (`X-Accel-Redirect`), `X-Sendfile`, `X-Lighttpd-Send-File` and `X-Rewrite-URL` as `Fault` (`502` before head) in both `blastit-fastcgi::cgi` and the shared `ResponseSink` (covering WSGI too); narrowed to the four exact names in Round 4 (MINOR-19). |
| `security` MINOR-7..10, 13..18 | MINOR | **All 10 accepted:** m7 (`Refusal::status()` sole authority; `StaticUpstream::plan` runs `head_segments` + `.php` `404` before `exchange`'s `405`/`413` `decide`, §4.2.1/§4.3); m8 (hard `2048`-byte STDERR accumulator cap + `O(1)` discard, §4.5.2); m9 (`HTTP_PROXY`/`http_proxy` stripped from `env[…]`, §4.1/§4.8.2); m10 (`FastCgiPaths` never calls `canonicalize()`, §4.5.1/§8.2); m13 (`setsid()` bounded by `RLIMIT_NPROC` + `0555` `parentfd` + `PoolReady` inode; `php-fpm` absolute-path bind scoped explicitly, §4.5.2/§4.8.2/§4.8.3); m14 (aggregate `BLASTIT_MAX_WORKERS × BLASTIT_MAX_BODY_BYTES` bound stated, R19/§4.5.2); m15 (private `<runtime>/views/<uid>/tmp` in `TMPDIR` & `open_basedir` + `host=<runtime>/views/<uid>` rewrite, §4.1/§4.8.2); m16 (`B3` freeze `EPERM` is boot-fatal exit `1` naming `CAP_CHOWN`/`CAP_FOWNER`, §4.8.3); m17 (`route_uid` `chmod`/`O_CREAT`/`unlink` `EPERM`/`EACCES` assertions after `PoolReady`, §7.2); m18 (`check_sql_obs_fd` closed in `stage0` right after `fork_dbproxy_dev`, §1.3/§4.10.3/§5.1). |
| `consistency` C7-M1 | MAJOR | **Accepted → §9 & `RESULT.md`.** Replaced false passing-verdict claim with exact Round 1–3 history; on-disk review tails remain `FAIL` until Round-4 critics run. |
| `consistency` C7-M2 | MAJOR | **Accepted → §4.2, §8, §8.2.** Synced `plan(&UpstreamHead)` across `MB.3.3`/`MB.9.1` rows, pinned `roles::scheduler` calling `run_with(builtin_registry(), …)` as sole composition-root call site, and pinned builder registration order (`MB.9.2.2` `"static"`, `MB.9.3` `"wsgi"`, `MB.3.6.2` `"fastcgi"`). |
| `consistency` C7-M3 / `implementability` MINOR-1 | MAJOR | **Accepted → §8, §8.1, §8.2.** Replaced non-existent letter IDs with exact numeric plan IDs, wrote the complete executable `plan.py` split/`depends_on`/`paths` checklist in §8.1, assigned `stage0.rs` exclusively to `MB.3.6.3`, and added the `MB.3.6.2` `TempDir` `UnixListener` sync to §8.2. |
| `consistency` C7-M4 | MAJOR | **Accepted → §4.2, §8.2, ADR-0008 §1 & Consequences.** Dropped "manifest backend name"; named `blastit_manifest::Executor` (`Executor::kind()`, `ALL_KINDS`) as the single source of executor kinds (1 enum variant + 1 `Upstream` + 1 `BackendBuilder`). |
| `consistency` C7-m1..m8 | MINOR | **All 8 accepted:** m1 (`!routes.is_empty()` & both dead-declaration arms in §8.2/ADR-0008); m2 (`audit_v1::unexercised_routes` `:950` combined-set guard named, §4.10.1/§4.10.3); m3 (`WireRequest.executor_ran` keeps head-`413` `exercised: 0` while firing `missing_ingress`, §4.2.1/§4.10.1/§4.10.3); m4 (`check -- <suite>` unified & pytest message scoped to app-less `MIXED`, §1.3/§4.10.3/§8); m5 (`FastCgiDeadDeclaration` only when `fastcgi && spa`, 0.2.0 `StaticDeadDeclaration` strings untouched, §4.1/§7.2); m6 (`ctl,frame.rs` solely in `MB.3.6.1.1`, no protected test in `paths`, §4.8.3/§8); m7 (3 `manifest.md` edits specified verbatim, §4.1/§8); m8 ("static-only" defined in ADR-0008, `ADR-0007` 1-line banner scheduled in `MB.3.8`, §8/§8.1). |
| `testability` M1 | MAJOR | **Accepted → §4.11, §7.2, §8, §8.1.** Pinned whole-slice `assert_eq!(IMPLEMENTATIONS, …)` in `conformance_table.rs` (`MB.9.4`) and added `Makefile` to `MB.3.6.1.2` and `MB.3.6.2` with a 5-binary `Makefile` assertion so neither a dropped capability bit nor a missing driver binary can pass `make rs-test-conformance` or `make check`. |
| `testability` M2 | MAJOR | **Accepted → §4.7, §7.2, §7.3.** Assigned exactly one `Owner file::fn` per §4.7 row, marked row 18 `—` (internal `500` guard, 24 client `E` probes in `PROBES`), pinned row 11 `server_keys` + lockdown stream, added row 25 `504` + `ABORT_REQUEST` fake-peer oracle to §7.2, and required non-empty discriminating subsets in `--self-test`. |
| `testability` M3 | MAJOR | **Accepted → §7.2, §8 (`MB.3.6.2`, `MB.3.7`).** Replaced passive `st_uid` check with an active decoy sentinel `UnixListener` at `/sentinel/fpm.sock`: post-`PoolReady` rename + symlink `run -> /sentinel` still serves `200` from pinned `entry` while sentinel records **`0` `accept`s**, and respawn while symlinked fails closed (`503`, Unwind U1, sentinel **`0` `accept`s**). |
| `testability` m1..m5 | MINOR | **All 5 accepted:** m1 (`critical_fastcgi_lockdown.rs` in `critical (new)`, §7.1); m2 (`critical_head_refusals.rs` marked "new file", §7.1/§7.2); m3 (strict per-class `Expect` assertions, §4.11); m4 (ADR-0008 Consequences narrowed to contract/refusal/body-cap/observation); m5 (Round 1–3 dispositions recorded here). |
| `implementability` M2 | MAJOR | **Accepted → §4.2.1 (`RouteState`, `BuildCtx.state`), §4.4, §4.8.1.** One shared `Arc<RouteState>` created by the scheduler and passed in `BuildCtx.state`; both scheduler and adapters read the exact same atomics. |
| `implementability` M3 | MAJOR | **Accepted → §8, §8.1.** Split all 5 over-budget leaves (`MB.3.5` → `.1`/`.2`, `MB.9.1` → `.1`/`.2`, `MB.9.2.1` → `.1`/`.2`, `MB.3.6.1` → `.1`/`.2`, `MB.3.6.4` → `.1`/`.2`) so every leaf is ≤ 7 files and ≤ 550 lines. |
| `implementability` M4 | MAJOR | **Accepted → §1.3, §4.10.3, §8 (`MB.3.6.3`, `MB.3.6.4.2`).** Added `BLASTIT_CHECK_DEV_ROOT=<CheckRoot>/dev` (`check::DEV_ROOT`) + `Dev::adopt` + `bind_all` at `slot_offset = 0` so `prepare_env` and child `dev`'s `<CheckRoot>/dev/views/0/.s.PGSQL.<port>` match. |
| `implementability` M5 | MAJOR | **Accepted → §4.2, §8.** Documented and wrote into every lock-owning leaf's acceptance cell the two-step rule: first build runs offline without `--locked` to update `Cargo.lock`, then `--locked` and `make check`. |
| `implementability` M6 | MAJOR | **Accepted → §8, §8.1, §8.2.** Added `MB.3.5.1` to `MB.9.1.1`'s `depends_on` so `blastit_manifest::Executor` exists before `blastit-backend` compiles. |
| `implementability` MINOR-1..5 | MINOR | **All 5 accepted:** 1 (numeric plan IDs everywhere, §8); 2 (`roles::scheduler` calls `builtin_registry()`, `SchedulerConfig.pinned_sockets` avoids premature Cargo edge, 3-kind assertion in `MB.3.6.2`); 3 (`SchedulerConfig.pool_spawn: PoolSpawn { Managed, External }` scoped to `Pool` routes, §4.8.1); 4 (boot + lifecycle kind sites named explicitly vs dispatch, §4.2/ADR-0008); 5 ("zero I/O in its own source", R14/§7.1). |

### Disposition of Round-4 `security` findings (MAJOR-A/B, MINOR-19..28)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `security` MAJOR-A | MAJOR | **Accepted → §4.2.1 (`PinnedSocket`), §4.5.2 (Socket identity), §4.8.1 (new "parentfd handoff"), §4.8.3 (`B3`, `C1`), §8.2 (`MB.3.6.2`/`MB.3.6.3`/`MB.3.6.4.2`), ADR-0008 decision 6.** Stage 0 opens `parentfd` `O_CLOEXEC`, crosses the stage-0→stage-1 `execve` as the internal argv entry `--fastcgi-parent-fd=<route>:<n>` (the existing `--dev-obs-fd` pattern — never `SCM_RIGHTS`, since `ctl.chan` does not exist yet), stage 1 wraps each in `OwnedFd`, re-sets `FD_CLOEXEC` before any fork, extends `fork_scheduler`'s `ChildIo.keep` with `SchedulerConfig::pinned_socket_fds()`, and `check::serve_and_run` passes `BLASTIT_CHECK_POOL_PARENT_FD` read by `Config::for_dev()` before `BLASTIT_*` stripping. Negative rule: no post-boot `open`/`openat`/`openat2` on the parent segment; the only socket op is `/proc/self/fd/<parentfd>/<name>`. Test: `critical_fastcgi_learn` fd-survives-exec + renamed-parent sentinel. |
| `security` MAJOR-B | MAJOR | **Accepted → §4.8.2 (Command), §4.8.3 (`S0`, `S3`, `S7`), §7.2 (`R2,R3,R20`), ADR-0008 decision 6.** `S0`'s `route_uid` `php-fpm -t` child and the `S3` pool master fork through `child::ChildIo { keep: vec![], stdout, stderr }`, so `close_fds_except` leaves only fd 0/1/2: `ctl.chan`, `parentfd`, the events pipes and every `BLASTIT_CHECK_*` fd are closed before `execve`; `S7`'s "held only by `init` and `scheduler`" is now true by construction. Test: `critical_fastcgi_pool::config_test_and_pool_child_exec_with_only_stdio` (recorded `readlink /proc/self/fd/*` == `{0,1,2}`). |
| `security` MINOR-19 | MINOR | **Accepted → §4.2.1 (`ResponseSink`), §4.5.1 (`cgi`), §4.5.2 (`STDOUT`), §4.11 (`error_mapping`), §5.1, §7.2, §8.2 (`MB.3.4.1`), ADR-0008 decision 4.** Deny exactly `X-Accel-Redirect`, `X-Sendfile`, `X-Lighttpd-Send-File`, `X-Rewrite-URL`; **drop** every other `X-Accel-*` name (`X-Accel-Buffering`/`-Expires`/`-Charset`/`-Limit-Rate`) without a fault, so a benign `X-Accel-Buffering: no` stays `200`. Conformance case `benign_x_accel_header_dropped`; row 24 unchanged (exact names). |
| `security` MINOR-20 | MINOR | **Accepted → §4.2.1 (`PinnedSocket`), §4.5.2 (Socket identity), §4.8.3 (`C1`), ADR-0008 decision 6.** `expected_uid` = socket owner (`scheduler_uid` prod / `invoking_uid` dev); `peer_uid`/`peer_gid` = listening pool master (`RouteSpec.uid` and its layout `route_gid` prod, `invoking_uid`/`invoking_gid` dev); `peer_uid = scheduler_uid` in production is named a bug. |
| `security` MINOR-21 | MINOR | **Accepted → §4.8.2 (Command), §4.8.3 (`S3`, `U1`), ADR-0008 decision 6.** Parent calls `setpgid(child_pid, child_pid)` right after the fork (`EACCES` = already set), so `pgid` exists before any `kill(-pgid)`; a failed group kill (`ESRCH`/`EPERM`) never skips `unlinkat` + re-freeze. |
| `security` MINOR-22 | MINOR | **Accepted → §4.8.2 (Pool file), §7.2 (`R11`), ADR-0008 decision 6.** `env[HOME]=<runtime>/views/<uid>/tmp` (same root as `TMPDIR`, already inside `open_basedir`); the private dir itself stays out of `open_basedir` so the app cannot read/rewrite its own `php-fpm.conf`. |
| `security` MINOR-23 | MINOR | **Accepted → §4.8.2 (Pool file, `workers` row), §7.2 (`R11`), ADR-0008 decision 6.** `pm.max_requests = max(BLASTIT_VIEW_MAX_REQUESTS, 500)` (`supervise::DEFAULT_POOL_MAX_REQUESTS`); for a pool `0` does not mean "never", so a future param-wall regression cannot persist for the process lifetime. |
| `security` MINOR-24 | MINOR | **Accepted → §4.8.3 (`B1`, `B2`), ADR-0008 decision 6.** `manifest_dirfd = open(..., O_NOFOLLOW)` plus `fstat`/`lstat` inode equality, `S_ISDIR`, `!S_ISLNK` and an absolute `/proc/self/fd` readlink; the non-symlink claim is scoped to `manifest_dir` and the segments below it (intermediate components stay covered by the sticky-bit rule). |
| `security` MINOR-25 | MINOR | **Accepted → §4.7 rows 10–11, §5.2, ADR-0008 decision 3.** Row 10 is CVE-2024-8926 only (query decode); CVE-2024-8927 (`cgi.force_redirect`/`REDIRECT_STATUS` collision) moves to row 11 and gets its own §5.2 row with the `server_keys` oracle. |
| `security` MINOR-26 | MINOR | **Accepted → §5.2, §7.3, ADR-0008 decision 3.** `P4b` is a **positive control**: a `PHP_VALUE`-accepting pool with the pins executes the chain, so its canary is **present**; it is excluded from the "canary absent" assertion, which stays on `P4`. |
| `security` MINOR-27 | MINOR | **Accepted → §4.6.2 (Value bytes), §7.2 (`R7`).** A single value `> 8192` bytes is **dropped** (`200`, `Param { Dropped }`), not `400` and not `ParamsTooLarge`; test `over_long_header_value_is_dropped_not_refused`. |
| `security` MINOR-28 | MINOR | **Accepted → §4.8.2 (Command), §7.2 (`R2,R3,R20`), ADR-0008 decision 6.** Named the escapee test: `critical_fastcgi_pool::setsid_escapee_after_retire_cannot_rebind_and_new_pool_has_new_inode` (`MB.3.6.3`) and `e2e/rootless/run.py::check_fastcgi_setsid_escapee_contained` (`MB.3.7`). |

### Disposition of Round-5 `security` findings (MAJOR-A residual, MAJOR-C, MINOR-29/30)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `security` MAJOR-A (R5 residual) | MAJOR | **Accepted → §4.1 (`SocketProbe`), §4.8.1 (steps 1–4, `check` paragraph), §4.8.2 (Who spawns), §8 (`MB.3.5.2`, `MB.3.6.3`, `MB.3.6.4.2`), §8.2, ADR-0008 decision 6.** (i) Deleted "the parent's copies close when `fork_scheduler` returns": `init` keeps one `Arc<OwnedFd>` per route in the pool supervisor's per-route state for the process lifetime (`S1`–`S7`/`U1` run on it; closes only at `init` exit), the scheduler gets its own copy by fork, `http`/`dbproxy`/`view`/`pool` drop theirs. (ii) `MB.3.6.3` now owns `crates/blastit/src/cli.rs` (drops `zygote.rs`): `cli::parse` accepts the internal repeatable `--fastcgi-parent-fd=<route>:<n>` only with `--stage=1`, invisible in `--help`, mapped into `stage1::Options`. (iii) Stage 1 validates the list **before** wrapping (unique route, unique fd, `fd >= 3`, not `config_fd`/dbproxy fd, `S_ISDIR`), so a duplicate can never be double-owned. (iv) `SocketProbe::check -> Result<Option<OwnedFd>, …>`; `DevProbe` returns the opened `parentfd`, `check_fastcgi_sockets -> Vec<(usize, OwnedFd)>`, `serve_and_run` threads it to child `dev`; one resolution, no second handoff source. |
| `security` MAJOR-C | MAJOR | **Accepted → §4.8.2 (Command), §4.8.3 (`S1`, `S2`, `S4`, `S6`, `U1`), §7.2 (`R2,R3,R20`), ADR-0008 decision 6.** `S1` reaps the previous pgid to `ECHILD` (bounded; not `ECHILD` → `U1`, window never opens) before `S2`; `S4` records an inode only if it appears as `socket:[<ino>]` in a task whose `/proc/<pid>/stat` `pgrp == master_pid` (the new pgid), and `S6` re-runs that check after the freeze; `U1` freezes **before** `unlinkat` so an escapee cannot rebind in the gap. The false "never matches a subsequent `PoolReady`" claim is replaced by the membership rule plus its residual (same-uid fd injection/`ptrace` only). Oracle now asserts `PoolReady.sock_ino` **equals** the inode the new master bound, and an escapee rebind yields **no** `PoolReady`, `U1`, `Busy` → `503`, and `0` `accept`s. |
| `security` MINOR-29 & MINOR-30 | MINOR | **Accepted → §4.5.2 (Socket identity), §4.8.3 (preamble, `C1`), ADR-0008 decision 6, resolve-architect §5.3.** (29) "zero path-based `chmod`" reworded everywhere to "no **caller-path-resolved** `chown`/`chmod`/`unlink`/`open`/`connect`; the one path-shaped call is `chmod("/proc/self/fd/<sockfd>")` on an `O_PATH` fd whose inode was just verified (`S5`), which is descriptor-derived". (30) `SO_PEERCRED` accepts exactly `{route_uid:route_gid}` in production (`peer_uid = RouteSpec.uid`, `peer_gid = route_gid`; the **listening** master is the socket creator) and `{invoking_uid:invoking_gid}` in `dev`/`check`; the gVisor `uid == expected_uid` tolerance is deleted (`scheduler_uid` is rejected), and `critical_fastcgi_dispatch::foreign_uid_peer_is_fault_502` names the negative test. |

### Disposition of the Round-5 follow-up MINORs (31–37)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `security` MINOR-31 | MINOR | **Accepted → §1.3, §4.8.1 (`check` paragraph), §4.10.3, ADR-0008 decision 8.** The child `dev` is spawned as `Host::stage1()`'s `Reexec` (`program` + `prefix`) + `dev --port 8080`: `/proc/self/exe` for a standalone binary, `<python> -I -m blastit` for a wheel (`host.rs:150-220`; `python/blastit/__main__.py`), never `<current_exe> dev` (which is `python dev` for the published wheel). The "while dbproxy starts" clause is gone: `clear_cloexec` runs immediately before the spawn and this branch starts no dbproxy. |
| `security` MINOR-32 | MINOR | **Accepted → §4.8.1, §7.2 (`R17`/`R18`), ADR-0008 decision 8.** `Config::to_json` (`config.rs:1532`) is a hand-written allowlist of scalar fields; `check_pool_parent_fds` and `check_sql_obs_fd` are never inserted and `Config` has no `Serialize` derive, so the stage-1 configuration document cannot carry either fd number. Test `critical_fastcgi_learn::config_document_carries_no_check_fd` asserts no `check_*`/`obs_fd` key. |
| `security` MINOR-33 | MINOR | **Accepted → §4.8.3 (`S0`), §7.2 (`R2,R3,R20`), ADR-0008 decision 6.** The oracle is now the negative half only: the fake `php-fpm` helper's first `fcntl(3, F_GETFD)`/`read(3, …)` returns `EBADF`, and no `/proc/self/fd` target is the pinned parent path or a `socket:[<ino>]`. The exact `{0,1,2}` set is not asserted because a wrapper's own script fd and the scanner's directory handle make it unobservable. |
| `security` MINOR-34 | MINOR | **Accepted → §4.5.2 (Socket identity), §5.1, ADR-0008 decision 6.** The ancestor claim is narrowed to `B1`'s `ARCHITECTURE.md` §5.3 sticky-bit rule above `manifest_dir` plus `B2`'s non-route-writable segments from `manifest_dir` to `parentfd`; a route that owns `manifest_dir` can rename the parent, which the pinned descriptor turns into a self-DoS `503`, never a redirect. |
| `security` MINOR-35 | MINOR | **Accepted → §4.5.1 (`cgi`), §4.5.2 (Response head caps), §4.7 row 24, §7.2, ADR-0008 decision 4.** A header **name** that is not an RFC 7230 `tchar` token (empty, NUL/CTL, space, `:`, obs-fold) is `Fault` (`502`) before any head is sent; the four proxy-control names are compared on the token name, case-insensitively. Tests `cgi::tests::non_token_header_name_is_fault` + row 24. |
| `security` MINOR-36 | MINOR | **Accepted → §4.8.2 (Who spawns), §4.8.3 (`S3`), ADR-0008 decision 6.** `supervise::spawn_pool` forks through `crate::forking::fork(true)` (the GIL is held for the whole `Supervisor::run(py)` loop, so `PyOS_BeforeFork`/`PyOS_AfterFork_{Parent,Child}` are the matching halves), exactly as `zygote::fork_view` does — never a raw `libc::fork`; the child runs `ChildIo::prepare` only after `PyOS_AfterFork_Child`, then drops privileges, installs the filter, `execve`s and calls `child_exit` on failure. |
| `security` MINOR-37 | MINOR | **Accepted → §4.6.2 (Count / total), §4.7 row 17.** The boundary is derived from the 18-param fixed set: `> 128` params total means `> 110` `HTTP_*` headers with a body and `> 112` without (`CONTENT_TYPE`/`CONTENT_LENGTH` absent), not `> 114`. |

### Disposition of Round-4 `implementability` findings (M1–M12 + one-liners)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` M1 (`BootError.line` carrier) | MAJOR | **Accepted → §4.2.1 (`RouteSpec`, `SchedulerConfig`), §8 (`MB.9.2.1.1`).** `RouteSpec` gains `line: u32`, filled by `run_with` from `SchedulerConfig.route_lines[index]` (built by `stage1` from `Manifest::route_lines()`, routing order); a short/missing entry is itself a `BootError` naming the route. `BootError`'s shape is unchanged. |
| `implementability` M2 (`run_with` vs `run`) | MAJOR | **Accepted → §4.2 (new `run_with` paragraph), §8 (`MB.9.2.1.2`).** `run_with(...) -> Result<(), RunError>` with `RunError { Boot(BootError), Scheduler(SchedulerError) }` and `SchedulerError::Boot(BootError)` in `crates/blastit-scheduler/src/error.rs` (added to `MB.9.2.1.2`'s paths); `run` keeps its 0.2.0 signature as `run_with(...).map_err(RunError::into_scheduler)`. No unowned file is edited. |
| `implementability` M3 (`PinnedSocket` API) | MAJOR | **Accepted → §4.2.1 (`PinnedSocket`), §8 (`MB.9.1.1`, `MB.3.6.2.2`).** `PinnedSocket::new(parent, name, parent_dev, parent_ino, expected_uid, peer_uid, peer_gid, enforce_socket_inode) -> Result<Self, PinnedSocketError>` (pure name validation, `BadName`; `Poisoned` for the `Mutex`), accessors `parent_fd`/`name`/`parent_inode`/`expected_uid`/`peer`/`enforce_inode`/`bound_inode`/`set_bound_inode`; tests build one from a `TempDir` `UnixListener`. |
| `implementability` M4 (`BuildCtx` cannot see `External`) | MAJOR | **Accepted → §4.2.1 (`BuildCtx`), §4.8.1, §8 (`MB.9.2.1.2`, `MB.3.6.2.1`).** Contract-owned `external_executor: bool`, set by `run_with` from `pool_spawn == External`; `FastCgiUpstream::ensure_ready` returns `Ok(())` immediately and never calls `SessionPort::spawn`. |
| `implementability` M5 (`RoutePermit` lifetime) | MAJOR | **Accepted → §4.2.1 (`RouteState`), §8 (`MB.9.1.1`).** `acquire(self: &Arc<Self>) -> RoutePermit`; `RoutePermit { state: Arc<RouteState> }` is `Send + 'static` and owns the `Arc`, so a `'static` `tokio::spawn` task can hold it. |
| `implementability` M6 (manifest crate has no `libc`/`rustix`) | MAJOR | **Accepted → §4.1, §8 (`MB.3.5.2`), §8.1.** `SocketProbe`/`SocketProbeError`/`check_fastcgi_sockets` stay pure in `blastit-manifest`; `DevProbe`/`ProductionProbe` move to `crates/blastit/src/probe.rs` (`rustix` already a dep), so no `Cargo.toml`/lock leaf is needed. |
| `implementability` M7 (`Ctl` breaks `tests/common/mod.rs`) | MAJOR | **Accepted → §8 (`MB.3.6.1.1`), §8.1.** `crates/blastit-scheduler/tests/common/mod.rs` is added to `MB.3.6.1.1`'s paths with the arm `Ctl::PoolSpawned { .. } \| Ctl::PoolReady { .. } \| Ctl::PoolExited { .. } => {}`. |
| `implementability` M8 (`peer_gid` typo) | MAJOR | **Accepted → §4.8.1.** `peer_uid = invoking_uid`, `peer_gid = invoking_gid` (was `peer_gid = invoking_uid`, which `502`s every `dev`/`check` request). |
| `implementability` M9 (`MB.3.6.3` paths) | MAJOR | **Accepted → §8 (`MB.3.6.3.1`/`.2`), §8.1, §8.2.** `cli.rs` and `dev_learn.rs` added (the internal `--fastcgi-parent-fd` arm and `Dev::adopt`, which already lives at `dev_learn.rs:180`); stale `zygote.rs` dropped; §8.1 item 2 lists the change. |
| `implementability` M10 (leaf budgets) | MAJOR | **Accepted → §8, §8.1, §8.2.** `MB.3.6.2` → `MB.3.6.2.1` (session pump) + `MB.3.6.2.2` (suites/`Makefile`); `MB.3.6.3` → `MB.3.6.3.1` (`B1`–`B3`, handoff, `cli`/`config`/`stage0`) + `MB.3.6.3.2` (`S0`–`S7`/`U1` + pool tests + fake helper); Order line, §8.1 sub-splits and §8.2 rows updated; lock ownership stays in the one serialized chain. |
| `implementability` M11 (undefined conformance types) | MAJOR | **Accepted → §4.11.** `Capability(u32)` (hand-rolled `const` bits, `union`, `contains`, `BitOr`, `ALL`; no `bitflags`), `HeadSpec`, `OutcomeKind`, `PeerScript`, `CancelAt` defined in the shared block. |
| `implementability` M12 (transitional registry) | MAJOR | **Accepted → §4.2 (transitional rule), §8 (`MB.9.2.2`, `MB.9.3`).** Until `MB.9.3`, `run_with` builds a stage only for a route with a registered builder and leaves `stages` empty (0.2.0 path serves it); `MB.9.3` deletes the fallback and a missing builder becomes `RunError::Boot`. No fallback backend is added. |
| `implementability` MINORs | MINOR | **Accepted:** `SocketProbeError` variants defined (§4.1); `ProductionProbe` returns `Ok(None)` so `check_fastcgi_sockets` returns `Vec::new()` for it (§4.1); the full `ChildIo` literal (`limits`, `working_dir`) pinned (§4.8.2, referenced by `S0`/`S3`); `builtin_registry() -> Result<Registry, BootError>` (§4.2); the fake `php-fpm` helper lives at `crates/blastit/tests/support/fake_php_fpm.rs` (§8 `MB.3.6.3.2`); §8.1 no longer repeats "with the exact file sets in §8"; fallback splits for `MB.9.2.1.2`/`MB.9.2.2`/`MB.9.3` stated (§8.1 item 3). |

### Disposition of Round-5 `implementability` findings (MAJOR-1..4 + folded one-liners)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §4.2.1 (`PinnedSocket`), §4.8.1 step 2, §8 (`MB.9.1.1`, `MB.3.6.3.1`), ADR-0008 decision 1.** `PinnedSocket::new(parent: Arc<OwnedFd>, …)` and `parent_fd(&self) -> BorrowedFd<'_> { self.parent.as_fd() }`; `SchedulerConfig.pinned_sockets: Vec<(usize, PinnedSocket)>` is built from `Arc::clone(&parent)`, the same `Arc` init keeps — no `try_unwrap`, no `dup`, no `unsafe`, no double close. |
| `implementability` MAJOR-2 | MAJOR | **Accepted → §4.2 (trait + dispatch snippet + new `Session` block), §4.2.1 (`BodyStream`, `SessionPort`, new `Session`/`SessionMsg` row), §4.5.2, ADR-0008 decision 1.** `Upstream::exchange` takes `body: BodyStream<'static>` and `sink: Box<dyn ResponseSink + Send>` (owned; no `sink.clone()`); `Scheduler::session(upstream, plan, body, sink) -> Session` is a scheduler-side `tokio::spawn` (`Session { id, done: mpsc::Receiver<SessionMsg> }`), `SessionDoneGuard::finish`/`Drop` delivers exactly one `Done`, and `SessionPort` is stated **WSGI-only**. |
| `implementability` MAJOR-3 | MAJOR | **Accepted → §8 (`MB.3.6.1.1`), §8.1 item 2, §8.2.** `crates/blastit/src/supervise.rs` added to `MB.3.6.1.1`'s paths (6 files) with the three ignored `Ctl::Pool*` arms at `supervise.rs:430-443`, and its acceptance now runs `cargo test -p blastit --locked`; `scheduler.rs:1043` already has an `other =>` catch-all. |
| `implementability` MAJOR-4 | MAJOR | **Accepted → §8 (`MB.3.5.2`), §8.1 item 2, §8.2.** `crates/blastit/src/lib.rs` has exactly one owner, `MB.3.5.2`, which adds `pub mod probe;` **and** `pub mod supervise;` (so `MB.3.6.3.2`'s integration test reaches `render_pool_conf`); `MB.3.6.3.2` does not edit `lib.rs`; the lock chain is unchanged. |
| `implementability` MINOR-5 & MINOR-11 | MINOR | **Folded → §4.2.** `run` is written out as `run_with(builtin_registry().map_err(SchedulerError::Boot)?, …).await.map_err(RunError::into_scheduler)`; `SchedulerError::Boot` is `#[error("{0}")]` with no `#[source]` and `BootError` implements `std::error::Error` (`MB.9.1.1`). |
| `implementability` MINOR-6..10 | MINOR | **MINOR-6 fixed in Round 7** (the unconditional `MB.9.2.1.2`/`.3` split); **MINOR-7..10 open** (recorded, not claimed fixed): MSRV of `self: &Arc<Self>`, `MB.3.8` path count, `TempDir` wording, `geteuid`/`getegid` spelling. |

### Disposition of Round-6 `implementability` findings (MAJOR-A..D + one-liners)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-A | MAJOR | **Accepted → §4.2 (Executor paragraph), §4.2.1 (`BuildCtx`), §8 (`MB.3.5.1`, `MB.9.2.1.1`), ADR-0008 decision 1.** `Executor` and `FastCgiPolicy` derive `Debug, Clone, PartialEq, Eq` (not `Copy`: `Pool` owns `String`s), which satisfies `RouteInfo`'s `Debug, Clone, PartialEq, Eq`; the pool spawn mode is `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum PoolSpawn { Managed, External }` (renamed from `SpawnMode` to avoid the `crate::config::SpawnMode` `Lazy`/`Eager` collision), owned by `MB.9.2.1.1`. |
| `implementability` MAJOR-B | MAJOR | **Accepted → §4.2.1 (`PoolPort`, `BuildCtx`), §4.8.1, ADR-0008 decision 1.** FastCGI pool starts use a distinct `PoolPort::ensure_pool(&self, route: RouteId) -> Result<(), UpstreamError>` (one `Ctl::Spawn { route }`, idempotent/rate-limited by `RouteState.ready`), created by `run_with` as `CtlPoolPort` and passed in `BuildCtx.pool`; `ensure_ready` calls it only when `!external_executor`, and the first request waits for `PoolReady` (5 s) under `request_timeout`. `SessionPort` stays WSGI-only. |
| `implementability` MAJOR-C | MAJOR | **Accepted → §4.2 (dispatch snippet + `Session` block), §4.2.1 (`RouteState`, `Session`/`SessionMsg`), §4.4 (Completion & panic guard), ADR-0008 decision 1.** One `SessionMsg::Done { id, result }`; bounded `mpsc::Sender<SessionMsg>` (capacity `1`); the receiver is kept in `self.sessions` (never dropped in dispatch); admission is `RouteState::try_acquire() -> Option<RoutePermit>` **before** `Scheduler::session(..., permit)`, with `None` → `Unavailable { retry_after: Some(1) }` (`503`); `SessionDoneGuard` is the single `Done` emitter. |
| `implementability` MAJOR-D | MAJOR | **Accepted → §4.8.3 (new "Integration-test seam"), §8 (`MB.3.6.3.2`), §8.1, §8.2.** `supervise.rs` carries an inline `#[doc(hidden)] pub mod pool_test` with `PoolHarness::{new,spawn,retire,assert_ready}` and `PoolReady { sock_dev, sock_ino }`; it runs the unprivileged subset (`S1`, `S2` as `euid`, `S4`–`S7`, `U1`) against a hand-rolled temp dir + `UnixListener` fake pool and a forked fake child, no `php-fpm`; `lib.rs` stays owned by `MB.3.5.2`. |
| `implementability` one-liners | MINOR | **Accepted:** `BodyStream` is lifetime-free (no `'a`, no `into_owned`); `MB.3.8` merges the three doc paths into one entry (7 total); `SchedulerConfig::pinned_socket_fds()` assigned to `MB.9.2.1.1`; `SocketProbe`/`ProbeUid`/`check_fastcgi_entries` all in `MB.3.5.2` (removed from `MB.3.5.1`); `MB.3.6.2.1` owns the `on_ctl` `Ctl::Pool*` handling; `foreign_uid_peer_is_fault_502` attributed to `MB.3.6.2.2`; fallback splits named for `MB.9.1.1` (`.1a`/`.1b`) and `MB.3.6.3.2` (`.2a`/`.2b`). |

### Disposition of Round-7 `implementability` findings (MAJOR-1..4)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §4.2 (trait), §4.2.1 (`BodySource`, `CancelToken`, `Session`), §4.5.2, ADR-0008 decision 1.** `Upstream::exchange` takes `Box<dyn BodySource + Send>`; the contract defines `trait BodySource { fn next -> BoxFuture<BodyEvent>; fn cancel_token -> CancelToken }` and `CancelToken(Arc<AtomicBool>)` (std only), while the tokio `BodyStream` (`mpsc::Receiver<BodyEvent>` + `Arc<CancelToken>`) lives in `blastit-scheduler`. `Credit` is scheduler-internal and never crosses the seam. `MB.9.1.1`'s `! grep -rn '…&#124;tokio' crates/blastit-backend/src` still passes. |
| `implementability` MAJOR-2 | MAJOR | **Accepted → §4.4, §4.5.2, §4.8.1.** §4.4 now says `PoolPort::ensure_pool(self.route)` for pool start (never `SessionPort`), and `SessionPort`/`ExchangeChannel` are described as the **WSGI** capability only; the whole document was re-grepped so every remaining `SessionPort` mention is WSGI (view spawn/open). |
| `implementability` MAJOR-3 | MAJOR | **Accepted → §8 (`MB.9.2.1.1`), §8.1 item 2, §8.2.** `crates/blastit/src/stage1.rs` added to `MB.9.2.1.1` (7 paths) because the `SchedulerConfig` literal must populate the four fields (it ends in `..SchedulerConfig::default()`, so omission is a silent empty `route_lines`); its acceptance now runs `cargo test -p blastit --locked`. `MB.3.6.3.1` already owns `stage1.rs`/`config.rs`, and `MB.3.6.1.1` owns `supervise.rs`; no other leaf adds a required field to a struct it cannot construct. |
| `implementability` MAJOR-4 | MAJOR | **Accepted → §8 (table, mapping, Order), §8.1, §8.2.** Unconditional splits: `MB.9.2.1.2` (session machinery: `upstream/mod`, `scheduler`, `error`, `head_allocs`) + `MB.9.2.1.3` (`lib.rs`, `roles.rs`); `MB.3.6.3.2` (`supervise.rs` + fake helper + inline `pool_test`) + `MB.3.6.3.3` (`critical_fastcgi_pool.rs`). `depends_on` chains, the `Cargo.lock` chain and the protected-test rules are unchanged. |

### Disposition of Round-8 `implementability` findings (MAJOR-1..3 + MINOR-1..6)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §4.2, §8 (`MB.9.2.1.2`, `.3`, `MB.9.2.2`, `MB.9.3`, `MB.3.6.2.1`), §8.1, §8.2, ADR-0008 decision 1.** `builtin_registry()` is **defined in `scheduler.rs`** (owned by `MB.9.2.1.2`, where `run` lives) and only **re-exported** from `lib.rs` (`MB.9.2.1.3`); `run` delegates to `run_with` in the same file; `MB.9.2.2`/`MB.9.3`/`MB.3.6.2.1` each append their builder to the list **in `scheduler.rs`** (all three own it). |
| `implementability` MAJOR-2 | MAJOR | **Accepted → §4.2.1 (`RouteState`), §4.5.2, §4.8.1, §8 (`MB.3.6.2.1`), ADR-0008 decision 1.** One rule: `RouteState::new(pool_spawn == PoolSpawn::External, policy.workers)` — `Ready` immediately in `External` (developer pool already up/validated), `Busy` in `Managed` until the verified `Ctl::PoolReady`; the first request waits on `ensure_pool` + `PoolReady` under `request_timeout`, then `503` on timeout/`PoolExited`. |
| `implementability` MAJOR-3 | MAJOR | **Accepted → §4.2.1 (`Observation`, `ObservationKind`), ADR-0008 decision 1.** `ObservationKind` is `#[serde(tag = "kind", rename_all = "snake_case")]` (internally tagged), which is what makes `#[serde(flatten)]` on `Observation` legal; `Query` serialises as `{"kind":"query",…}`; `UpstreamError`/`Refusal`/`BodyMode` are not serde types. Exact-line tests: `blastit-backend` `types::tests::wire_request_line_is_exact` and `critical_fastcgi_learn::http_obs_line_is_exact`. |
| `implementability` MINOR-1 | MINOR | **Accepted → §8.2.** `MB.9.2.1` and `MB.3.6.3` row labels now read (`.1`/`.2`/`.3`). |
| `implementability` MINOR-2 | MINOR | **Accepted → §8 (`MB.9.2.1.1`), §8.2.** The `stage1.rs` justification is corrected: the literal ends in `..SchedulerConfig::default()`, so adding fields compiles; `stage1.rs` is needed to **populate** `route_lines`/`pool_spawn`/`pinned_sockets`/`http_obs` (omission = silent empty `route_lines` → every route a `BootError`). |
| `implementability` MINOR-3 | MINOR | **Accepted → §8 (`MB.3.6.3.1`).** `SchedulerConfig.{route_lines, pinned_sockets, pool_spawn}` (the non-existent `external_executor` field removed; it is `BuildCtx`'s). |
| `implementability` MINOR-4 | MINOR | **Accepted → §8 (`MB.9.2.1.1`).** `SchedulerConfig.http_obs: Option<PathBuf>` (path, not fd); `None` until `MB.3.6.3.1` wires `Config.check_http_obs`. |
| `implementability` MINOR-5 | MINOR | **Accepted → §8.1 item 1.** Every parent is converted with `plan.py --agent planner edit <parent> --kind epic` before the first `new` (a task with children is rejected). |
| `implementability` MINOR-6 | MINOR | **Accepted → §8.1 item 3.** Named fallback split for `MB.9.2.1.2`: `.2a` (`UpstreamHead` + `RouteRuntime.stages` + `ViewHub`/`SessionPort`) + `.2b` (session task/`SessionDoneGuard` + `http_obs` writer + `run_with`/`RunError`). |

### Disposition of Round-9 `implementability` findings (MAJOR-1..2 + MINOR-1..5)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §8 (`MB.3.4.2`, `MB.9.2.2`, `MB.9.2.1.2`), §8.1 item 2, §8.2.** `MB.3.4.2` owns `crates/blastit-fastcgi/src/lib.rs` and adds `pub mod lockdown;` (3 files); `MB.9.2.2` owns `crates/blastit-scheduler/src/upstream/mod.rs` and adds `pub mod static;` (5 files); `MB.9.2.1.2` declares `mod upstream;` in `scheduler.rs` (not `lib.rs`, owned by `.3`). |
| `implementability` MAJOR-2 | MAJOR | **Accepted → §8 (`MB.3.5.2`), §8.1 item 2, §8.2.** `MB.3.5.2` owns `crates/blastit/src/supervise.rs` (7 paths), narrows `Parts`/`Supervisor`/`Supervisor::new` to `pub(crate)` (only `render_pool_conf` and the inline `pool_test` module are `pub`), and its acceptance adds `cargo clippy -p blastit --all-targets --locked -- -D warnings`, so the leaf that would trip `private_interfaces` is the leaf that fails. |
| `implementability` MINOR-1 | MINOR | **Accepted → ADR-0008 decision 1.** `RouteState::try_acquire(self: &Arc<Self>) -> Option<RoutePermit>` (the stale `acquire` spelling is gone). |
| `implementability` MINOR-2 | MINOR | **Accepted → §8 (`MB.3.6.3.2`), §8.2.** `tests/fake_php_fpm_smoke.rs` (`#[path = "support/fake_php_fpm.rs"] mod fake_php_fpm;`) is a real test target, so the helper is built by `.2`'s acceptance. |
| `implementability` MINOR-3 | MINOR | **Accepted → §8 (`MB.9.2.2`).** `conformance_static.rs` includes the fake as `#[path = "common/fake_upstream.rs"] mod fake_upstream;`, so `.2.2` needs no edit to `tests/common/mod.rs`. |
| `implementability` MINOR-4 | MINOR | **Accepted → §8.1 item 3.** The `MB.9.2.1.2` fallback `.2b` now also carries `builtin_registry()`, `run`, `BuildCtx.external_executor` and the transitional `stages.is_empty()` path. |
| `implementability` MINOR-5 | MINOR | **Accepted → §8.1 item 1.** The `--kind epic` enumeration now lists all eleven parents (`MB.3.4`, `MB.3.5`, `MB.3.6`, `MB.9`, `MB.9.1`, `MB.9.2`, `MB.9.2.1`, `MB.3.6.1`, `MB.3.6.2`, `MB.3.6.3`, `MB.3.6.4`) and `MB.3.4.2` gets `depends_on: [MB.3.4.1]`. |

### Disposition of Round-10 `implementability` findings (MAJOR-1..3 + MINOR-1..6 + matrix)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §4.5.1, §4.7 row 18, §8 (`MB.3.4.1`, `MB.3.4.2`, `MB.3.6.2.1`), §8.2, ADR-0008 decision 4.** `blastit-fastcgi` defines `LockdownError { Malformed, UnknownParam, ParamsTooLarge, DuplicateParam }`, `CgiError { BadHead, NonTokenName, ForbiddenProxyHeader }` and `RecordError { Truncated, ProtocolStatus }`; `critical_fastcgi_lockdown.rs` asserts those variants; `MB.3.6.2.1` maps them to `Refusal`/`UpstreamError::Fault`. The crate stays zero-dependency (no `blastit-backend` edge before `MB.3.6.1.2`). |
| `implementability` MAJOR-2 | MAJOR | **Accepted → §4.5.1, §8 (`MB.3.4.2`, `MB.3.6.2.1`), §8.2, ADR-0008 decision 4.** `FastCgiPaths { script_filename: PathBuf, document_root: PathBuf, script_name: String }` is defined in `blastit-fastcgi::lockdown` (std types, no I/O); `FastCgiBuilder` (`MB.3.6.2.1`) builds it; the scheduler row no longer claims it. |
| `implementability` MAJOR-3 | MAJOR | **Accepted → §4.8.2 (new `render_pool_conf` block), §8 (`MB.3.5.2`, `MB.3.6.3.2`), ADR-0008 decision 6.** `#[doc(hidden)] pub fn render_pool_conf(spec: &PoolConf<'_>) -> String` with `#[doc(hidden)] pub struct PoolConf<'a>` (std types only); `supervise` is `pub mod` only for those two, `Parts`/`Supervisor`/`new` stay `pub(crate)`; the clippy `-D warnings` gate moved from `MB.3.5.2` to `MB.3.6.3.2`. |
| `implementability` MINOR-1 | MINOR | **Accepted → §4.1.** The fake probe returns `ForeignOwner { uid: other_uid }` (no `Owner` variant). |
| `implementability` MINOR-2 | MINOR | **Accepted → §8 (`MB.9.2.1.3`).** `pub use crate::scheduler::{builtin_registry, run_with};` and `pub use crate::error::{RunError, SchedulerError};`. |
| `implementability` MINOR-3 | MINOR | **Accepted → §4.2.** `run_with<C: CtlChannel + Send + 'static>(...)`. |
| `implementability` MINOR-4 | MINOR | **Accepted → §8.1 item 3.** Named fallback `MB.3.6.3.2` → `.2a` (`render_pool_conf` + `PoolConf` + `pool_test`) / `.2b` (`S0`–`S7`/`U1` + fake helper + smoke test). |
| `implementability` MINOR-5 | MINOR | **Accepted → §8.1 intro.** §8.1/§8.2 are the authoritative gate until `planner` applies them; the on-disk `NODE.md` files are pre-§8.1 and nobody implements from a stale brief. |
| `implementability` MINOR-6 | MINOR | **Accepted → §8 (acceptance cells of `MB.9.2.2`, `MB.9.3`, `MB.3.6.2.2`, `MB.3.6.3.3`, `MB.3.6.4.2`), §8 "Test lints".** Each test-adding leaf runs `cargo clippy -p <crate> --all-targets --locked -- -D warnings`; suites carry `#![allow(clippy::unwrap_used, clippy::expect_used)]` like the existing ones. |
| `implementability` structural | — | **Accepted → §8 "Type-ownership matrix".** One row per pinned public type/trait → defining crate → owning leaf → reader leaves; it is the single source of truth for assignments, and §4.x/§8/§8.1/§8.2 were reconciled against it. |

### Disposition of Round-11 `implementability` findings (MAJOR-1..3 + MINOR-1..7 + matrices)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §8 (`MB.3.6.1.2`), §8 crate-edge matrix, type matrix, ADR-0008 decision 1.** `MB.3.6.1.2` adds `blastit-backend = { version = "0.2.0", path = "../blastit-backend" }` to `crates/blastit/Cargo.toml`; its acceptance adds `cargo test -p blastit --locked` and `cargo tree -p blastit -e normal \| grep -q blastit-backend`; `MB.3.6.3.1` is now a `PinnedSocket` reader. |
| `implementability` MAJOR-2 | MAJOR | **Accepted → §4.2 crate table, §8 (`MB.9.2.1.1`), §8 crate-edge matrix, ADR-0008 decision 1.** `blastit-scheduler` gains `blastit-http` (for `BodyFraming`) in `MB.9.2.1.1` (it owns `crates/blastit-scheduler/Cargo.toml`); acceptance adds `cargo tree -p blastit-scheduler -e normal \| grep -q blastit-http`. |
| `implementability` MAJOR-3 | MAJOR | **Accepted → §4.2.1 (`Observation`, `ObservationKind`, `ObsTarget`, `ParamDecision`, `OutputStream`, `UpstreamHead`, `PinnedSocket`, `PoolConf`), §8 derive matrix, ADR-0008 decision 1.** The observation path derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` (`ParamDecision`/`OutputStream` also `Copy`); `UpstreamHead` is `Debug, Clone, PartialEq, Eq`; `PinnedSocket` is `Clone, Debug` only; `PoolConf` is `Debug, Clone, Copy`. |
| `implementability` MINOR-1 | MINOR | **Accepted → §8.1 item 2, §8.2.** The clippy gate is only in §8's `MB.3.6.3.2` acceptance; §8.1/§8.2 no longer assign it to `MB.3.5.2`. |
| `implementability` MINOR-2 | MINOR | **Accepted → §4.2.1 (`Session`/`SessionMsg`), §8 type matrix.** `Session`/`SessionMsg`/`Scheduler::session` are `pub(crate)`; `MB.3.6.2.2` tests behaviour only. |
| `implementability` MINOR-3 | MINOR | **Accepted → §8 type matrix.** `MB.9.2.1.3` dropped from the `BackendBuilder`/`Registry`/`BootError` reader row (it only re-exports `builtin_registry`). |
| `implementability` MINOR-4 | MINOR | **Accepted → §4.5.1 (`param` row).** A truncated/straddling pair is `RecordError::Truncated`; no separate `ParamError`. |
| `implementability` MINOR-5 | MINOR | **Accepted → §4.8.3, §8 type matrix.** The harness type is `PoolReadyInfo` (distinct from `Ctl::PoolReady`). |
| `implementability` MINOR-6 | MINOR | **Accepted → §4.10.1.** `CheckObsV1::new_with_upstream(schema_version, http, upstream, sql)` is pinned. |
| `implementability` MINOR-7 | MINOR | **Accepted → §8.1 item 3.** Named fallback `MB.3.6.2.1` → `.1a` (`fastcgi.rs` pump) + `.1b` (`fastcgi_builder.rs`: `FastCgiPaths`, mapping, `ensure_ready`/`on_ctl`/registration). |
| `implementability` structural | — | **Accepted → §8 crate-edge matrix and derive matrix.** Both are declared sources of truth beside the type-ownership matrix; the §4.2 crate table, type matrix and every `Lands` cell are reconciled against them. |

### Disposition of Round-12 `implementability` findings (MAJOR-1..2 + MINOR-1..10)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §8 (`MB.9.2.1.2`, `MB.9.2.1.3`), §8.1 item 2, §8.2.** `mod upstream;` is declared in `crates/blastit-scheduler/src/lib.rs` (crate root, so `crate::upstream` resolves to `src/upstream/mod.rs`); `lib.rs` joins `MB.9.2.1.2`'s paths (5 files) and `.3` only appends `pub use` lines; acceptance is `cargo test -p blastit-scheduler --locked` (crate root, not a `--lib` subset). |
| `implementability` MAJOR-2 | MAJOR | **Accepted → §4.2.1 (`UpstreamHead`), §8 (`MB.9.2.1.1`), §8 crate-edge matrix, ADR-0008 decision 1.** `ReqHeadView::scheme() -> Scheme` (the frame-owned `blastit_frame::frame::Scheme`); `begin_raw` maps `Scheme::Http&#124;Https → Tls::Http&#124;Https`; the matrix adds `blastit-frame → (no new edge)` with `! grep -q blastit-backend crates/blastit-frame/Cargo.toml`. |
| `implementability` MINOR-1 | MINOR | **Accepted → §8 type matrix.** `MB.3.6.3.1` added to the `PinnedSocket` and `PoolSpawn` reader lists. |
| `implementability` MINOR-2 | MINOR | **Accepted → §8 type matrix.** `Session`/`SessionMsg` reader is "— (behaviour only; `pub(crate)`)". |
| `implementability` MINOR-3 | MINOR | **Accepted → §8 (`MB.9.1.2`).** `Lands` names `HeadSpec`, `OutcomeKind`, `PeerScript`, `CancelAt` alongside `Case`/`Class`/`Capability`/`Expect`/`Inject`/`RouteFixture`. |
| `implementability` MINOR-4 | MINOR | **Accepted → §8 (`MB.3.6.2.1`), §8.1 item 3.** `upstream/fastcgi_builder.rs` is in `.1`'s paths, so the `.1a`/`.1b` split adds no path. |
| `implementability` MINOR-5 | MINOR | **Accepted → §8 "Test lints".** Reworded: every test-adding leaf is covered by `make check`'s workspace clippy; the five FastCGI suites also run it locally. |
| `implementability` MINOR-6 | MINOR | **Accepted → §8 (`MB.3.6.3.1`).** `config.rs` defines `pub(crate) const POOL_PARENT_FD_ENV`; `MB.3.6.4.2` aliases it as `check::POOL_PARENT_FD`. |
| `implementability` MINOR-7 | MINOR | **Accepted → §8 type matrix.** Footnote: every unlisted §4.2.1 type is `blastit-backend`/`MB.9.1.1` (the `Lands` catch-all). |
| `implementability` MINOR-8 | MINOR | **Accepted → §8 (`MB.9.4`).** `rs-test-conformance` starts at 3 binaries, `MB.3.6.1.2` bumps to 4, `MB.3.6.2.2` to 5. |
| `implementability` MINOR-9 | MINOR | **Accepted → §8 (`MB.3.6.2.1`).** `lib.rs` dropped from its paths (registration is in `scheduler.rs`); 4 paths. |
| `implementability` MINOR-10 | MINOR | **Accepted → §4.2.** The illustrative `Session`/`SessionMsg` block is `pub(crate)`, matching §4.2.1. |

### Disposition of Round-13 `implementability` findings (MAJOR-1 + MINOR-1..8)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §4.2.1 (`RouteSpec`), §8 (`MB.3.5.1`), §8.2, type matrix, ADR-0008 decision 1.** `MB.3.5.1` adds `Executor`/`FastCgiPolicy` to `crates/blastit-manifest/src/lib.rs`'s `pub use model::{…}` list, so `blastit_manifest::Executor` is the crate-root path; its acceptance keeps `cargo test -p blastit-manifest --locked` and adds a `lib.rs` doctest `use blastit_manifest::{Executor, FastCgiPolicy};`. |
| `implementability` MINOR-1 | MINOR | **Accepted → §4.2.1, ADR-0008 decision 1.** The public path is `blastit_frame::Scheme` (root re-export); `raw.rs` uses `crate::frame::Scheme`. |
| `implementability` MINOR-2 | MINOR | **Accepted → §4.2.1.** `ReqHeadView::parse` stores `scheme: Scheme` via `Scheme::from_code(..)?` (unknown code → `FrameError::BadEnum`) and the stale "scheduler never reads it" doc comment is updated. |
| `implementability` MINOR-3 | MINOR | **Accepted → §8 "Docs".** `blastit-scheduler` is `#![deny(missing_docs)]`; every new public item carries a `///` on its definition (`blastit-backend`/`blastit-manifest` likewise). |
| `implementability` MINOR-4 | MINOR | **Accepted → §8 derive matrix.** `Tls` derives `Debug, Clone, Copy, PartialEq, Eq`. |
| `implementability` MINOR-5 | MINOR | **Accepted → §8 type matrix.** Added `PoolConf` (`blastit::supervise`, `MB.3.6.3.2`) and `Case`/`Class`/`Expect`/`Inject`/`RouteFixture`/`IMPLEMENTATIONS`/`implementation()`/`cases()` (`blastit-backend::conformance`, `MB.9.1.2`). |
| `implementability` MINOR-6 | MINOR | **Accepted → §8 crate-edge matrix.** Added `blastit-http → (no blastit-backend)` and spelled `serde = { version = "1.0.229", features = ["derive"] }`. |
| `implementability` MINOR-7 | MINOR | **Accepted → §4.2, ADR-0008 decision 1.** `pub async fn run_with<C: CtlChannel + Send + 'static>(…)`. |
| `implementability` MINOR-8 | MINOR | **Accepted → §8 (`MB.3.8`).** The row says "7 path entries / 9 files; the release leaf is exempt from the file cap". |

### Disposition of Round-14 `implementability` findings (MAJOR-1 + MINOR-1..7)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §4.1 (`SocketProbe`, `SocketProbeError`, `check_fastcgi_entries`), §1.3, §8 (`MB.3.5.2`), ADR-0008 decision 2.** `SocketProbe` gains `check_entry(&self, manifest_dir, entry, root, uid) -> Result<(), SocketProbeError>`; `SocketProbeError` gains `NotRegularFile`, `Unreadable { uid }`, `OutsideRoot`; `check_fastcgi_entries(&self, manifest_dir, probe: &dyn SocketProbe, uid)` passes the probe; the syscall body lives in `crates/blastit/src/probe.rs` (`rustix`), so `blastit-manifest` stays I/O-free and the `! grep rustix&#124;libc` acceptance keeps passing; `MB.3.5.2`'s protected suite injects the three entry variants. |
| `implementability` MINOR-1 | MINOR | **Accepted → §1.3, §4.1.** One spelling: `ProbeUid::Dev` (the §1.3 `check::execute` snippet now compiles). |
| `implementability` MINOR-2 | MINOR | **Accepted → §4.2.1 (`Executor`).** `pub const ALL_KINDS: &'static [&'static str] = &["wsgi", "fastcgi"];`. |
| `implementability` MINOR-3 | MINOR | **Accepted → §8 (`MB.3.5.1`).** `docs_fences.rs` is three edits (`FILES: [&str; 7]` → `8`, `assert_eq!(total, 7)` → `8`, module-doc "seven" → "eight"). |
| `implementability` MINOR-4 | MINOR | **Accepted → §8 type matrix.** `FastCgiPolicy`/`Executor` readers add `MB.3.5.2`, `MB.3.6.3.1`, `MB.3.6.3.2`. |
| `implementability` MINOR-5 | MINOR | **Accepted → §8 (`MB.3.5.2`).** Serialized `lib.rs` ownership: `.1` appends the two `pub use model::{…}` names, `.2` then appends `pub mod probe;`. |
| `implementability` MINOR-6 | MINOR | **Accepted → §8 (`MB.3.5.1` acceptance).** The doctest also asserts `Executor::View.kind() == "wsgi"` and `ALL_KINDS.contains(&"fastcgi")`, proving the surface, not just the path. |
| `implementability` MINOR-7 | MINOR | **Accepted → §8 crate-edge matrix.** The `blastit-manifest → no rustix/libc` row says "unchanged; asserted by `MB.3.5.1`'s acceptance (no leaf edits the file)". |

### Disposition of Round-15 `implementability` findings (MAJOR-1 + MINOR-1..7)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §4.1 (`check_entry` mechanism, `MB.3.5.2`), §8 (`MB.3.5.2`, `MB.3.7`), ADR-0008 decision 2.** `ProductionProbe::check_entry` brackets `openat2` + `faccessat2(AT_EACCESS)` with `setfsuid(route_uid)`/`setfsgid(route_gid)` and restores on every path (restore failure = `SocketProbeError::Io`, `exit 1`); `DevProbe` skips it. Oracles: `MB.3.5.2` fake-probe unit + `e2e/rootless/run.py::check_fastcgi_entry_0000_refused_for_route_uid` (`MB.3.7`). No caller-path `chown`/`chmod`/`connect`. |
| `implementability` MINOR-1 | MINOR | **Accepted → §4.1, §1.3, §8 (`MB.3.6.3.1`).** `check_fastcgi_entries(manifest_dir, probe, route_index, uid)` validates one route; `check::execute`/`stage0` call it per FastCGI route with that route's uid. |
| `implementability` MINOR-2 | MINOR | **Accepted → §4.1.** `SocketProbe::provides_parent_fd()` (default `false`, `DevProbe` `true`); `check_fastcgi_sockets` refuses `Some(fd)` when it is `false`. |
| `implementability` MINOR-3 | MINOR | **Accepted → §4.1.** Fixed phrases `must be a regular file`, `must be readable by uid <uid>`, `must be under static root "…"` (`OutsideRoot`). |
| `implementability` MINOR-4 | MINOR | **Accepted → §4.1.** `ProbeUid` is authoritative; `DevProbe.uid` is only for `check`'s connect-as-uid. |
| `implementability` MINOR-5 | MINOR | **Accepted → §4.1.** `ProbeUid` derives `Debug, Clone, Copy, PartialEq, Eq`; every `ProbeUid`/`SocketProbeError` variant carries a `///` (`#![deny(missing_docs)]`). |
| `implementability` MINOR-6 | MINOR | **Accepted → §8 (`MB.3.5.2` acceptance).** `! grep -qE 'rustix\|libc' crates/blastit-manifest/Cargo.toml` is in the leaf that adds `probe.rs`. |
| `implementability` MINOR-7 | MINOR | **Accepted → §8.1 item 3.** Named fallback `MB.3.5.2` → `.2a` (manifest `probe.rs` + `lib.rs` + protected suite) / `.2b` (`blastit/src/probe.rs` + `lib.rs` + `supervise.rs` + `manifest.md`). |

### Disposition of Round-16 `implementability` findings (MAJOR-1 + MINOR-1..4)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §4.1 (probe-child mechanism), §8 (`MB.3.5.2`, `MB.3.6.3.1`, `MB.3.7`), ADR-0008 decision 2.** `ProductionProbe::check_entry` forks a probe child from single-threaded stage 0 (`forking::fork(false)`) over a pipe; the child `drop_privileges(uid)` (`setresgid(uid,uid,uid)` + `setresuid(uid,uid,uid)`, caps dropped on the transition), `harden_child`, `SeccompRole::View { egress: false }`, `set_dumpable(false)`, then `openat2` + `fstat` + `faccessat2(AT_EACCESS)` and one reply byte (`O`/`U`/`N`/`S`/`R`/`I`); a missing/garbled/short/non-zero reply is boot-fatal `SocketProbeError::Io`, never "readable". `DevProbe` needs no child. `setfsuid` is rejected (`rustix` lacks it, `CAP_DAC_OVERRIDE` bypasses DAC, no error return). |
| `implementability` MINOR-1 | MINOR | **Accepted → §4.1.** `route_gid` is `uid` (`setresgid(uid, uid, uid)`); no separate gid source. |
| `implementability` MINOR-2 | MINOR | **Accepted → §4.1.** `FastCgiEntryInvalid { location, entry, rule: String }` with the 7-row error→phrase table (`must exist`, `must be a regular file`, `must be readable by uid <uid>`, `must not be a symlink`, `must be under static root "<root>"`, `must be owned by uid <uid>`, `must be stat-able`). |
| `implementability` MINOR-3 | MINOR | **Accepted → §8 (`MB.3.6.3.1`).** Both call sites named: `stage0::run_dev` with `ProbeUid::Dev`, production per FastCGI route with `ProbeUid::Route(uid)`. |
| `implementability` MINOR-4 | MINOR | **Accepted → §4.1, §8.** Oracles split explicitly: `MB.3.5.2`'s fake probe is unprivileged; `e2e/rootless/run.py::check_fastcgi_entry_0000_refused_for_route_uid` (`MB.3.7`) is root-only and exercises the real probe child. |

### Disposition of Round-17 `implementability` findings (MAJOR-1..3 + MINOR-1..11)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` MAJOR-1 | MAJOR | **Accepted → §4.1 (step 4), ADR-0008 decision 2.** The readability check is `unsafe { libc::syscall(libc::SYS_faccessat2, fd, c"".as_ptr(), libc::R_OK, libc::AT_EMPTY_PATH \| libc::AT_EACCESS) }` with a `// SAFETY:` comment; `rustix::fs::accessat` rejects `EMPTY_PATH` (`EINVAL`). No `blastit-sys` helper (unowned file). |
| `implementability` MAJOR-2 | MAJOR | **Accepted → §4.1 (step 2), ADR-0008 decision 2.** The probe child calls **only** `blastit_sys::drop_privileges(uid)` (it `capset`s explicitly); inlining `setresgid`/`setresuid` is forbidden because the kernel clears caps only when all uids cross 0 and a rootless `init` never was 0. `harden_child` + `set_dumpable(false)` keep their order. |
| `implementability` MAJOR-3 | MAJOR | **Accepted → §4.1 (steps 4–6), ADR-0008 decision 2.** `openat2(..., O_PATH\|O_NOFOLLOW\|O_CLOEXEC, ...)` (never `O_RDONLY`) + `fstat` `S_ISREG` + `faccessat2`; the parent waits at most 5 s (`child::wait_readable`) and on timeout `SIGKILL` + `waitpid` → `SocketProbeError::Timeout`; `DevProbe` uses the same flags. |
| `implementability` MINOR-1 | MINOR | **Accepted → §4.1 (step 1).** `crate::forking::fork(interpreter)` (the `stage0` flag), not a hard-coded `false`. |
| `implementability` MINOR-2 | MINOR | **Accepted → §4.1 (step 5, table).** `M` for `Missing`; `ForeignOwner` removed from the entry table (it is socket-path-only). |
| `implementability` MINOR-3 | MINOR | **Accepted → §4.1 (step 6).** Parent closes the write end after the fork, then `libc::waitpid(..., 0)`; `blastit_sys::reap()` is not used (`WNOHANG`, reaps any child). |
| `implementability` MINOR-4 | MINOR | **Accepted → §4.1 (`ProductionProbe` comment).** "`check` returns `Ok(None)`; B1/B2 run in `stage0` (`MB.3.6.3.1`)". |
| `implementability` MINOR-5 | MINOR | **Accepted → §4.1 (`check_entry` doc).** "from a `manifest_dirfd` opened `O_RDONLY\|O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC`". |
| `implementability` MINOR-6 | MINOR | **Accepted → §4.1 (step 4).** `entry.strip_prefix(root)` (else `OutsideRoot`); `root` opened `O_PATH\|O_DIRECTORY` with `RESOLVE_BENEATH\|RESOLVE_NO_SYMLINKS`. |
| `implementability` MINOR-7 | MINOR | **Accepted → §8 (`MB.3.5.2`).** "leave `render_pool_conf` (added by `MB.3.6.3.2`) and `pool_test` `pub`". |
| `implementability` MINOR-8 | MINOR | **Accepted → §8.1 item 3.** Named fallbacks: `MB.3.5.2` `.2a`/`.2b`, `MB.3.6.3.1` `.1a`/`.1b`, `MB.3.6.3.2` `.2a`/`.2b`. |
| `implementability` MINOR-9 | MINOR | **Accepted → §8 type matrix.** `DevProbe`/`ProductionProbe` (`blastit::probe`, `MB.3.5.2`, reader `MB.3.6.3.1`). |
| `implementability` MINOR-10 | MINOR | **Accepted → §4.1.** The duplicated fake-`SocketProbe` paragraph is deleted. |
| `implementability` MINOR-11 | MINOR | **Accepted → §4.1 (step 5).** errno mapping pinned (`ENOENT→M`, `EACCES/EPERM→U`, `ELOOP→S`, `EXDEV→R`, `EISDIR/ENXIO/ENODEV→N`, else `I`); a garbled reply has its own message (`probe child exited without a reply`), not `must be stat-able`. |

### Disposition of Round-18 `implementability` findings (four one-liners)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `implementability` R18-1 | MINOR | **Accepted → §4.1 steps 5–6, ADR-0008 decision 2.** The reply set is `O`/`U`/`N`/`S`/`R`/`M`/`I`/`T`; any reply that is not exactly one byte from that set is boot-fatal `SocketProbeError::Io`, and a diagnostic byte maps to its variant — every non-`O` byte still aborts boot. |
| `implementability` R18-2 | MINOR | **Accepted → §8 (`MB.3.5.2`).** `Timeout` added to the `SocketProbeError` enumeration; `critical_fastcgi_manifest::probe_timeout_is_boot_fatal` (fake probe that never replies → `Timeout`, boot-fatal) is in the protected suite. |
| `implementability` R18-3 | MINOR | **Accepted → §4.1 (`ProductionProbe`, step 3).** `ProductionProbe { …, pub seccomp: bool }`; the child calls `install_filter(self.seccomp, …)`, consistent with `BLASTIT_SECCOMP=0` handling in `stage0`. |
| `implementability` R18-4 | MINOR | **Accepted → §4.1 ("`ENOSYS` policy").** `faccessat2` `ENOSYS` refuses boot with `./blastit.toml:<line>: fastcgi entry "<entry>": faccessat2 is unavailable (Linux < 5.8); blastit requires it for the route-uid readability check`; kernels < 5.8 (RHEL/CentOS 8, Debian 10, Ubuntu 18.04, Amazon Linux 2) are unsupported for `[route.fastcgi]`; no weaker fallback. |

### Disposition of Round-4 `testability` findings (MAJOR-T1..T3 + MINOR-T1..T12)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `testability` MAJOR-T1 | MAJOR | **Accepted → §7.1, §7.2, §8 (`MB.3.6.3.1`), ADR-0008 decision 6.** New `crates/blastit/tests/critical_fastcgi_parent_fd.rs` (6th path of `MB.3.6.3.1`, unprivileged): crafted `--fastcgi-parent-fd` argv per rule — duplicate route, duplicate fd, `fd < 3`, `fd == config_fd`, `fd ==` a dbproxy fd, non-directory fd, missing entry — each asserts `exit 1` + the exact message; one positive case asserts the fd is `FD_CLOEXEC` before the first fork (absent from the http child's `/proc/<pid>/fd`). No rule needs root; the root-only pool lifecycle stays in `MB.3.7`. |
| `testability` MAJOR-T2 | MAJOR | **Accepted → §7.2 (`R9`/`R20` row).** `critical_fastcgi_dispatch::lazy_pool_bind_timeout_is_503_retry_after_never_502`: no `PoolReady`, advance 5 s (`tokio::time::pause()`), assert `503`, `Retry-After: 1`, no `502`, slot released, and a later `PoolReady` admits the next request. |
| `testability` MAJOR-T3 | MAJOR | **Accepted → §7.2 (`R4` row).** `critical_fastcgi_dispatch::workers_cap_answers_503_for_the_second_concurrent_request`: `workers = 2`, three concurrent requests admit two, the third gets `503` before connect, the slot is released on completion, and `RouteState::capacity()` equals the policy's `workers` for both `Pool` and `View` executors. |
| `testability` MINOR-T1 | MINOR | **Accepted → §4.7 row 17.** Owner renamed `over_budget_param_count_or_bytes_is_params_too_large`; `conformance_fastcgi::params_too_large` is the 400 oracle. |
| `testability` MINOR-T2 | MINOR | **Accepted → §4.7 row 2.** Owner is `conformance_static::malformed_path` (dispatch-level 400; `critical_static_resolve` asserts `StaticOutcome`). |
| `testability` MINOR-T3 | MINOR | **Accepted → §7.2 (`R17`/`R18`).** The `MIXED` reasoning is stated: `api` has no `[route.static]`/`[route.fastcgi]`, so `can_boot_without_app() == false` and branch 3 keeps `"no interpreter to run pytest on"`. |
| `testability` MINOR-T4 | MINOR | **Accepted → §5.2 (`P10_param_edges`).** `400`/`431`/`200`/`200` per row. |
| `testability` MINOR-T5 | MINOR | **Accepted → §4.11.** `conformance_table.rs` asserts the exact `cases()` id set as well as `IMPLEMENTATIONS`. |
| `testability` MINOR-T6 | MINOR | **Accepted → §7.1 (docs row).** The whole `--json` document is asserted; the Deployments `blastit --preset compose-traefik` fence is explicitly exempt. |
| `testability` MINOR-T7 | MINOR | **Accepted → §7.1 (unit row).** `critical_fastcgi_manifest.rs` covers the `check_entry` `ENOSYS` refusal and the reply-byte parser (`O/U/N/S/R/M/I/T`). |
| `testability` MINOR-T8 | MINOR | **Accepted → §7.2 (`R1`–`R6`).** Named fns: `connect_and_trace_are_boot_fatal`, `workers_zero_and_sixty_five_are_boot_fatal`, `probe_timeout_is_boot_fatal`; `blastit-sys` `scheduler_af_unix_allowed_bind_listen_denied` + `view_twin`. *(Superseded by `MB.3.9.2`: the shared-parent case no longer exists — the parent is derived — and `listen_key_is_unknown` + `workers_over_budget_is_boot_fatal` replace it; the latter is now the **pool leaf's**, gated on `MB.12`.)* |
| `testability` MINOR-T9 | MINOR | **Accepted → §8.2 (`MB.3.4.2`).** Plan `paths` drift is fixed by §8.1 item 2 (`src/lib.rs`). |
| `testability` MINOR-T10 | MINOR | **Accepted → §7.2 (`R16`).** Retained symbols named: `rewrite::decide`, `static_response::StaticBody`, `head_segments`, `resolve`. |
| `testability` MINOR-T11 | MINOR | **Accepted → §7.2 (`R19`/`R10`/`R18`).** Tests for the 60-lines/request `STDERR` cap, the 8 KiB CGI value cap, the `BLASTIT_MAX_WORKERS × BLASTIT_MAX_BODY_BYTES` aggregate, `drain`, and the parked-static-read slot; body sentinel in `http_obs.jsonl`. |
| `testability` MINOR-T12 | MINOR | **Accepted → §7.2 (`R2,R3,R20`).** *(Superseded by `MB.3.9.2`: the modes are `0711`/`0511`/`0600` and the oracle is `pool_parent_modes_are_0600_0511_0711`.)* The pool suite's single-UID fixture pins `fchownat`/`0555`/`0600` assertions as `#[ignore]`d `requires-root`; the root-only legs run in `e2e/rootless`. |

### Disposition of Round-5 `testability` findings (MAJOR-T1 + MINOR-T1..T10)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `testability` MAJOR-T1 | MAJOR | **Accepted → §4.8.1 (step 4), §7.2 (`R2`/`R3`), §8 (`MB.3.6.3.1`), ADR-0008 Consequences.** `critical_fastcgi_parent_fd::view_child_holds_neither_parentfd_nor_ctl_chan` boots a mixed `[route.fastcgi]` + WSGI manifest, issues one WSGI request, and asserts `/proc/<view_pid>/fd` has no entry resolving to the pinned parent inode and no `ctl.chan` socket; the code rule is pinned — the view/zygote `ChildIo.keep` **omits** `pinned_socket_fds()` (the monitor/zygote parent keeps its own copy). Unprivileged, so it runs in `make check`. |
| `testability` MINOR-T1 | MINOR | **Accepted → §7.1.** `critical_fastcgi_parent_fd.rs` (`MB.3.6.3.1`) added; `critical_fastcgi_pool.rs` is `MB.3.6.3.3`. |
| `testability` MINOR-T2 | MINOR | **Accepted → §8.1 item 2.** `MB.3.6.3.1` adds `crates/blastit/tests/critical_fastcgi_parent_fd.rs`; `.3` keeps `critical_fastcgi_pool.rs`. |
| `testability` MINOR-T3 | MINOR | **Accepted → §8 (`MB.3.6.3.1`).** `cli::parse` rejects `--fastcgi-parent-fd` without `--stage=1`; `--help` lacks the string. |
| `testability` MINOR-T4 | MINOR | **Accepted → §8 (`MB.3.6.4.2`).** `dev` with `BLASTIT_CHECK_POOL_PARENT_FD=0:/etc` or `=99:/run` exits `1` naming the route, never a path connect. |
| `testability` MINOR-T5 | MINOR | **Accepted → §8 (`MB.3.6.3.1`).** The suite drives the pure `validate_parent_fds(&[(usize, RawFd)], config_fd, dbproxy_fds) -> Result<(), BootError>`, so "no descriptor wrapped" is unit-tested before any `OwnedFd`. |
| `testability` MINOR-T6 | MINOR | **Accepted → §7.2 (`R9`/`R20`).** Advance `5 s + 1 ms`; assert `UpstreamError::Unavailable { retry_after: Some(1) }`; the never-`PoolReady` `PoolPort` fake is named in §7.1. |
| `testability` MINOR-T7 | MINOR | **Accepted → §7.2 (`R4`).** A `tokio::sync::Barrier` in the fake `exchange` holds the first two permits; exactly two `connect`s are observed. |
| `testability` MINOR-T8 | MINOR | **Accepted → §7.2.** The parent-fd suite is in the `R2`/`R3` row; the `workers_cap` reference is the `R9`/`R20` row (there is no `R4` row). |
| `testability` MINOR-T9 | MINOR | **Accepted → §7.3.** `--self-test` parses the `#n` column and asserts the union is exactly `{1..20}`. |
| `testability` MINOR-T10 | MINOR | **Accepted → §8.1 item 2.** `MB.3.6.3.1` owns `cli.rs` for the internal arm; `MB.3.6.4.2` later edits it for `CheckOptions` (serialized `depends_on`), so there is no conflict. |

### Disposition of Round-6 `testability` findings (MAJOR-T1 + MINOR-T1..T6)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `testability` MAJOR-T1 | MAJOR | **Accepted → §7.1, §7.2 (`R9`/`R20`, `R17`/`R18`), §8 (`MB.3.6.2.2`, `MB.3.6.4.2`), ADR-0008 decision 6.** (a) `critical_fastcgi_dispatch::external_mode_never_calls_ensure_pool` *(superseded for the `dev` half by `MB.3.9.2`: `dev` is `Managed` after `MB.3.9.4`, so and after `MB.3.9.4` no shipped mode keeps the external flag — see §7.2 and ADR `Amends`)* — recording `PoolPort` fake (`Arc<AtomicUsize>` + panic-on-call), `external_executor = true`, one request → `200`, counter `0`; the `false` twin → counter `1` and waits for `PoolReady`. (b) `critical_fastcgi_learn::external_pool_never_spawns_php_fpm` — after one request under `dev`, `/proc/<init_pid>/task/<init_pid>/children` has no pid whose `/proc/<pid>/comm` is `php-fpm` (`children()` in `smoke.rs`). Both unprivileged, in `make check`. |
| `testability` MINOR-T1 | MINOR | **Accepted → §7.1.** A "Proves" clause for `critical_fastcgi_parent_fd.rs`: parent-fd argv validation + `cli::parse` guard + the mixed-manifest view fd oracle. |
| `testability` MINOR-T2 | MINOR | **Accepted → §8 (`MB.3.6.3.1`).** The `Lands` cell names `view_child_holds_neither_parentfd_nor_ctl_chan` (mixed fixture, `UnixListener` at the derived socket path, `smoke.rs`-style `app`, one WSGI request, `/proc/<view_pid>/fd`), ~150 lines within the cap. |
| `testability` MINOR-T3 | MINOR | **Accepted → §7.2 (`R2`/`R3`).** The predicate is inode identity: the inode read from init's `ctl.chan` entry before the request is absent from the view's table; the view's `view.chan` inode **is** present (a socketpair's two ends share an inode, so "no socket in common with init" would fail on the correct program). |
| `testability` MINOR-T4 | MINOR | **Accepted → §7.2 (`R2`/`R3`).** Preconditions asserted: WSGI response `200`, view pid found via `/proc/<init_pid>/task/<init_pid>/children`, fd set non-empty and containing `view.chan` + stdio; no `#[ignore]`/skip (the test is unprivileged and `critical-skip-check` would flag it). |
| `testability` MINOR-T5 | MINOR | **Accepted → §7.2 (`R2`/`R3`).** Fixture contract named: the manifest needs `app` (a WSGI route with neither block makes `can_boot_without_app()` false); the test binds a `UnixListener` at the derived `<run_root>/fastcgi/0/fpm.sock` in its temp dir even though it never speaks FastCGI; it never starts `php-fpm`; it reuses `smoke.rs`'s `Fixture`/`fds()`/`children()`. |
| `testability` MINOR-T6 | MINOR | **Accepted → §7.2 (`R2`/`R3`).** The init-side assertion (`/proc/<init_pid>/fd` still resolves the pinned parent after `PoolReady` and after `RETIRE`/respawn) is `requires-root` and lives in `e2e/rootless`; the unprivileged `dev` addition checks `/proc/<init_pid>/fd` holds a directory fd whose `st_dev`/`st_ino` equal the fixture's `run/` inode. |

### Disposition of Round-7 `testability` findings (MINOR-T1..T4; lens PASSES)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `testability` MINOR-T1 | MINOR | **Accepted → §7.2 (`R17`/`R18`).** `external_pool_never_spawns_php_fpm` *(superseded by `MB.3.9.2`; replaced by `dev_starts_one_pool_per_route` in `MB.3.9.5`)*: it stated the `200` fake-peer precondition, walked `/proc/<init_pid>/task/*/children` **transitively** (a double-forked `php-fpm` is not a direct child of the main thread), and adds a **positive control** (a deliberately renamed `/bin/sleep` child the same walk finds). |
| `testability` MINOR-T2 | MINOR | **Accepted → §7.2 (`R2`/`R3`).** The view predicate asserts **both** halves: the pinned parent's `(st_dev, st_ino)` and init's `ctl.chan` inode are absent from the view's table, while `view.chan` is present. |
| `testability` MINOR-T3 | MINOR | **Accepted → §7.2 (`R2`/`R3`/`R20` Suite column).** `critical_fastcgi_parent_fd.rs` added. |
| `testability` MINOR-T4 | MINOR | **Accepted → §7.2, §8.1 item 2, §8.2 (`MB.3.6.3`).** `smoke.rs` is a separate test binary and cannot be imported; `MB.3.6.3.1`/`MB.3.6.4.2` duplicate the `children()`/`fds()` helpers locally (precedent `critical_dev_and_fail_closed.rs:482`), so no path changes are needed and the ≤ 7-file cap is untouched. |

### Disposition of Round-3 `consistency` findings (C8-M1..M4 + C8-m1..m9)

| Lens & ID | Grade | Resolution & exact section |
|---|---|---|
| `consistency` C8-M1 | MAJOR | **Accepted → §4.1, §4.8.2, §7.2 (`R11`), ADR-0008 decision 2.** `env[HOME]=<runtime>/views/<uid>/tmp` everywhere (the `tmp` form the `open_basedir`/`R11` reasoning requires); the §4.1 occurrence is fixed. |
| `consistency` C8-M2 | MAJOR | **Accepted → §4.8.2 (Pool file row), §4.1 (`R11`), §7.2 (`R11`), ADR-0008 decision 6.** One formula: `pm.max_requests = max(BLASTIT_VIEW_MAX_REQUESTS, 500)`; `supervise::DEFAULT_POOL_MAX_REQUESTS` lives in `supervise.rs` and is owned by `MB.3.6.3.2`. |
| `consistency` C8-M3 | MAJOR | **Accepted → §8.2 (new row for `MB.3.3`/`MB.9.1.1`/`MB.9.2.1.2`/`MB.3.6.2.1`).** Briefs must say `exchange(&self, plan, body: Box<dyn BodySource + Send>, sink: Box<dyn ResponseSink + Send>)`, the current `ExchangeOutcome`/`Observation` shapes, and the `pub(crate)` `Scheduler::session(..., permit)`. |
| `consistency` C8-M4 | MAJOR | **Accepted → §8.1 item 1.** Every split bullet now says "exactly the `Files` cell of §8's row for that id"; the §8 cells are complete and each acceptance file is in its leaf's `paths`. |
| `consistency` C8-m1 | MINOR | **Accepted → §4.10.3.** `BLASTIT_CHECK_POOL_PARENT_FD=<route_index>:<fd>` in both places. |
| `consistency` C8-m2 | MINOR | **Accepted → §4.1.** Fixed-phrase list adds `must exist`/`must be stat-able`, drops the duplicate `must be under static root`, and prefixes the `FastCgiListenParentInvalid` example with `route "app" (index 0):`. |
| `consistency` C8-m3 | MINOR | **Accepted → §4.1 (`listen` row).** The ancestor rule is `B1`'s sticky-bit walk from `/` to `manifest_dir` plus `B2`'s non-route-writable segments down to the parent. *(Superseded by `MB.3.9.2`: the row is gone with the key, and `B1`/`B2` now anchor on `<runtime>` and create `<runtime>/fastcgi/<i>`.)* |
| `consistency` C8-m4 | MINOR | **Accepted → §4.1 (`manifest.md` edit 1).** The app-present rule uses the predicate it actually implements — "every route is `[route.static]` and none is `[route.fastcgi]`" — and says `app` is accepted and ignored on a manifest with any `[route.fastcgi]`. |
| `consistency` C8-m5 | MINOR | **Accepted → §9 history.** The stale `SpawnMode` spelling is `PoolSpawn`. |
| `consistency` C8-m6 | MINOR | **Accepted → §8.1 item 3.** Fallback splits are "two numeric `new` children" (`MB.x.y.1`/`.2`); `plan.py`'s `ID_RE` rejects letter suffixes. |
| `consistency` C8-m7 | MINOR | **Accepted → §8.2.** Three rows added: `MB.3.3` `kind()` wording, `MB.3.6.3` §4.4 predicate, `MB.3.6` parent (`check -- <suite>`, temp-dir `UnixListener` fake peer). |
| `consistency` C8-m8 | MINOR | **Accepted → ADR-0008 decision 1.** `Registry::new` rejects duplicate kinds and any kind outside `"static"` ∪ `Executor::ALL_KINDS`. |
| `consistency` C8-m9 | MINOR | **Accepted → §8 (`MB.3.6.1.1`).** `Ctl::Spawn`'s doc comment is updated to "view or pool, chosen by the route's executor". |

---

### Amendment round (`MB.3.9.2`, 2026-11-14): scope, two review passes and the round-3 verification

**Scope of this amendment.** Only the **manifest grammar and the socket's identity** changed:
no `listen`, a derived per-route socket keyed by the route's index, `workers` optional and
bounded by the deployment budget, and the error variants that say so. Everything here is
**mechanism-independent**: `MB.12` may replace `php-fpm` with an embedded PHP SAPI, and the
manifest, the derived path, the error messages and the worker bound survive that change
unchanged. Findings that depend on *whether a pool process exists at all* — who spawns it in
`dev`/`check`, the pool file's ownership, dev inode pinning, deleting `PoolSpawn::External` —
are recorded in `RESULT.md` as **deferred to `MB.12`'s embed-vs-pool decision**, not decided
here; the `dev`/`check` sequencing itself is `MB.3.9.3`'s.

| Pass | Lens | Verdict | BLOCKER / MAJOR opened | Disposition |
|---|---|---|---|---|
| 1 | `implementability` | FAIL | A1 directory modes (a `0700` container makes `php-fpm`'s absolute `bind` `EACCES`), A2 the probe conflated create-parent with leaf-exists | both fixed: the three-mode table (`<runtime>` root `0755`, `fastcgi` `0711`, `<i>` `0700`→`0755 route_uid`→`0511`; `dev`/`check` `0700`, no window) and the two-phase probe (`parent` then `check_leaf`) |
| 1 | `security` | FAIL | S-1 omission bypasses the `1..=64` ceiling (`pm = static` pre-forks), S-2 pin/freeze not re-entrant over a preserved `<runtime>`, S-3 a planted non-empty directory wedges every route | S-1 `MAX_POOL_WORKERS = 64` *(now owned by the pool leaf, gated on `MB.12` — attempt 3)* + `FastCgiWorkersCapExceeded`; S-2 normalise-before-verify in `B2`; S-3 `remove_entry` (unlink → `AT_REMOVEDIR` → rename-aside, bounded) |
| 1 | `testability` | FAIL | T1 the design pinned `UnknownKey` while the shipped parser returns `FastCgiListenDerived`; T2–T8 unowned/absent oracles | T1 adopted the shipped variant (it carries no value and names the owner); T2–T8 each got one named test with an owner leaf (inventory lives in §7.2) |
| 1 | `consistency` | FAIL | C-01 the same mode error as A1 plus an ADR that said `0555`; C-02…C-04 surviving "`dev` never spawns a pool"; C-05 `ARCHITECTURE.md` §2.2; C-06 path shape | all fixed; `MB.3.8` now owns the §2.2 layout row, and the ADR carries dated retractions |
| 2 | `implementability` | FAIL | I-1 residual `0700`/`0555` in the ADR, I-2 unowned phase-2 mapper, I-3 `check` ordering, I-4 un-ordered worker arms, I-5 inventory count, I-6 the `External`→`Managed` transition unpinned, I-7 `check` mode vs the sibling briefs, I-8 the e2e oracle named the pre-fix invariant | all fixed; the design is declared normative over the two stale brief sentences (recorded in §8.2 and `RESULT.md`) |
| 2 | `security` | FAIL | SEC-R2-1 `remove_entry` still failed on a **non-empty** planted directory, SEC-R2-2 `dev`/`check` would inherit `S2`/`S6` and then `EACCES` their own retire | rename-aside for `ENOTEMPTY`; the single-uid variant (`S0`,`S1`,`S3`,`S4`,`S7`,`U1`; no `S2`/`S5`/`S6`) |
| 2 | `testability` | FAIL | T17 the `MB.3.5.1` leaf row and ADR still said `UnknownKey`; T19 ADR `0555`; T20 a `capacity()` assertion that cannot compile; T21–T27 ownerless oracles | T17/T18 already resolved by the pass-1 fix (verified on disk); T19–T27 fixed (permit-count oracle, call-site owners, `InsecureMode`, `remove_entry`/`B2`/`check_leaf` oracles, one inventory) |
| 2 | `consistency` | FAIL | F1 three spellings of the `listen` refusal, F2 `0o555` vs `0511` + ADR modes, F3 residual `0700` prose, F4 §9 rows still asserting the retired dev oracle | all fixed |
| 3 | `implementability` | FAIL | R3-1 decision 6's `0555` + a self-contradicting `U1` sentence; R3-2 the retired "unreachable" e2e leg name and a duplicated `verify_private_dir`; R3-4 `check_fastcgi_workers`' doc comment inverted the arms *(the function itself is now the pool leaf's, gated on `MB.12` — attempt 3)* | all fixed: decision 6 opens with a mode note; the leg is renamed `…_traversable_but_not_writable_…`; `verify_private_dir` moved to one home; the doc comment states the three arms |
| 3 | `security` | FAIL | SEC-R3-1 the rename-aside still moved the wedge; SEC-R3-2 the "single-uid variant" kept `drop_privileges` and `U1`'s freeze | both fixed: eight rename slots, empty `stale.*` cleaned, an unfreeable leaf fails **one route's pool** (warning + `503`), never the boot; the variant is a delta table (no `drop_privileges`, no `S2`/`S5`/`S6`, `U1` without the freeze) |
| 3 | `testability` | FAIL | R3-1 the mode trio had no named oracle; R3-2 `check_leaf`'s exit codes had no named test | `pool_parent_modes_are_0600_0511_0711`; `leaf_missing_after_spawn_exits_1_in_dev` + `check_child_leaf_failure_exits_2_naming_the_derived_socket` |
| 3 | `consistency` | **PASS** | none | M1–M3 taken |
| 3b | `consistency` re-verify | FAIL | one MAJOR, introduced by the pass-3 `B3` rewrite (a non-`ENOENT` removal still called a boot error) | fixed: `B3` defers to `remove_entry`, and its failure cell splits capability `EPERM` (`exit 1`) from an unfreeable leaf (warning + that route's pool `Busy`/`503`) |

**The critic cap, stated so the record cannot be misread.** The node's acceptance asks for
"fresh goldfish critics PASS". Round 3 ended **FAIL** on three lenses (implementability,
security, testability) and PASS on consistency; the `3b` re-verify then found one MAJOR. Every
one of those findings was resolved and **verified by the reviewer's next read, not by a fourth
critic round** — the lead capped review at round 3. So the last artifacts on disk are not a
clean critics-PASS: they are `FAIL` rounds whose BLOCKER/MAJOR/agreed-MINOR findings are closed,
plus a reviewer-verified sweep (attempts 3 and 4). Attempt 4's three edits (the
`FastCgiPoolSocketInvalid` owner, the `validate::check_fastcgi_workers` wording, this note) are
likewise reviewer-verified, not critic-verified.

**MINORs.** The two passes opened 46 MINORs between them. All were accepted and applied
except the following, which are recorded rather than edited: (a) §9's **historical** rows keep
their original `0555` / `external_pool_never_spawns_php_fpm` text, because rewriting a past
disposition would falsify the record — each is annotated "superseded by `MB.3.9.2`" at the
point where a reader would otherwise follow it; (b) the two sibling briefs
(`MB.3.9.3`/`MB.3.9.4`) still say "`PoolSpawn::External` survives for `check`", which this
document contradicts deliberately (§8.2 marks this document normative there, because an
external pool no longer exists); (c) the real 5 s probe-timeout path is a `requires-root`
leg, because no unprivileged test can hold a silent child for five seconds and observe the
`SIGKILL`.

**Deferred (recorded in `RESULT.md`, not decided here).** Whether a pool process exists at
all is `MB.12`'s embed-vs-pool decision, so these stay open: the pool file's ownership
hardening (it is `0600` route-uid today and protected only by `open_basedir`), `dev`'s
`enforce_socket_inode` value, deleting `PoolSpawn::External`, and any `php-fpm.conf` path or
directive. The grammar, the derived path, the error messages and the worker bound do not
depend on that decision.
