---
id: MB.3.3
title: "Design the FastCGI session, param lockdown and the injected Upstream backend contract (ADR-0008)"
kind: task
status: ready
owner: designer
priority: 1
depends_on: [MB.3.2]
paths:
  - "docs/design/php-fastcgi.md"
  - "docs/design/php-fastcgi.review-security.md"
  - "docs/design/php-fastcgi.review-implementability.md"
  - "docs/design/php-fastcgi.review-testability.md"
  - "docs/design/php-fastcgi.review-consistency.md"
  - docs/design/README.md
  - "docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md"
  - docs/decisions/README.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T01:54"
---

# Goal

Turn the accepted `docs/frameworks/php.md` into `docs/design/php-fastcgi.md` plus
`ADR-0008-fastcgi-backend-and-param-lockdown.md`, covering **two halves in one
document**:

1. The FastCGI half: the exact `[route.fastcgi]` grammar, the session/framing
   contract, the param allowlist with a **binary refusal matrix**, per-route pool
   workers, `--learn`/`check` attribution for PHP routes, and the CVE defence table.
2. The **language-agnostic backend contract** half (owner requirement 2026-11-14):
   one object-safe `Upstream` trait in a contract crate that knows nothing about any
   language or server, one **injected** `Arc<dyn Upstream>` per route built at boot
   by a factory/registry, crate-by-seam layering, and one shared behavioural
   conformance table. FastCGI, static and WSGI all implement that one trait.

Everything is pinned to the shipped supervisor / `RawFrame` scheduler, with no second
supervisor and no second static responder.

# Context

- Input of record: `docs/frameworks/php.md` (accepted by both DX lenses) and
  `docs/research/php-fastcgi-wire.md` + `docs/research/php2shell.md` (all citations).
  The design may not contradict the page; if it must, `human add` the conflict rather
  than weakening either silently.
- **Grammar to pin** (exact keys, defaults, error messages, and what fails at boot):
  the `[route.fastcgi]` block — entry script (`public/index.php`), pool `listen`
  socket path, `workers`, and the environment map — plus how it composes with
  `[route.static]` on the same route (`root = "public"` serving assets while the
  entry script and dynamic paths reach the worker). Decide and justify per-route
  `[route.fastcgi]` (like `[route.static]`) versus a named top-level block; the page
  shows per-route, so per-route wins unless there is a real reason.
  Boot-fatal list: missing/absolute/`..` entry script; entry script outside the
  static root when both are declared; a socket path that is relative, missing, not a
  socket, or not owned by the route uid when production; `workers = 0`; unknown keys;
  a `[route.fastcgi]` on a route whose `methods` include something the worker cannot
  answer. State what `blastit check`/`dev` validate as the invoking uid.
- **Wire contract:** which record types blastit emits and consumes; one `BEGIN_REQUEST`
  per request; `requestId` strategy (strictly increasing per connection, wrap or
  reconnect at exhaustion), whether a connection is reused or one-per-request; PARAMS
  terminator; `STDIN` body framing under the existing response/request credit;
  `STDOUT` → the HTTP head/body split (`Status:`/headers parsing, the
  `CGI`-style default status), `STDERR` → `app.output` events with the line cap,
  `END_REQUEST` → accounting (protocol status vs app status, a truncated stream, a
  worker that dies mid-response), and cancellation (`ABORT_REQUEST` on client
  disconnect). Say which of this lives in the pure crate `crates/blastit-fastcgi`
  (`MB.3.4`: codec + guards only, no I/O) and which in the scheduler (`MB.3.6`:
  connect/spawn/pump/account).
- **The param contract, as tables.** (a) The **complete** set of params blastit sends
  for a request, each with its source and whether it is pinned, derived, or dropped
  (`SCRIPT_FILENAME` pinned to the declared entry script resolved from the route;
  `SCRIPT_NAME`, `PATH_INFO`, `DOCUMENT_ROOT` derived; `QUERY_STRING` from the URL;
  `REQUEST_URI`/`REQUEST_METHOD`/`SERVER_PROTOCOL`/`CONTENT_TYPE`/`CONTENT_LENGTH`
  from the validated head; host/port environment; `REDIRECT_STATUS` decision;
  `HTTP_*` from a header allowlist). (b) **`HTTP_PROXY` must not be reachable from a
  `Proxy:` request header** (`httpoxy`): name the rule that makes it impossible.
  (c) **Refusal matrix**, one row per attack → param/path byte → status/refusal:
  `SCRIPT_FILENAME` override and traversal, `PATH_INFO` into `SCRIPT_FILENAME`,
  `PHP_VALUE`, `PHP_ADMIN_VALUE`, `PHP_ADMIN_*` unknown keys, `auto_prepend_file` /
  `allow_url_include` / `open_basedir` / `disable_functions` via any param,
  CVE-2012-1823 query-string `-d …` argument injection, the CVE-2024-4577
  best-fit/encoding probe, `REDIRECT_STATUS` and `DOCUMENT_ROOT` spoofing,
  `HTTP_PROXY`, a duplicate param name, a NUL/over-long name, and an unknown param.
  Every row binary: refused with what the client sees (`400` vs a dropped param and
  the request proceeding), so a cheap implementer can table-drive it.
  (d) **Canonicalisation rule:** case-folding of names, forbidden characters, one
  decode policy, duplicate handling, maximum count and total byte budget — no
  "validate then re-serialise differently" gap.
- **Per-route pool workers:** who spawns `php-fpm -F -y`, how it is configured as a
  non-root pool (uid per route), how `workers`/`pm.max_children` relate, socket
  ownership/permissions, restart on crash, and budget accounting under the existing
  `BLASTIT_MAX_WORKERS`. No new role if the existing view role can own it; if a new
  role is unavoidable, say so explicitly and justify against ADR-0001/ADR-0007.
- **Static coordination:** `public/` assets keep being served by the shipped `0.2.0`
  `[route.static]` responder. Pin the interaction: which requests the responder
  answers, that the **entry script itself is never served as a file** (`*.php` under
  the root is refused/forwarded, never `200 text/x-php`), and the precedence with the
  FastCGI route. Link `docs/frameworks/static-and-spa.md` and
  `docs/design/static-and-spa.md`; do not redefine the responder. Note `MB.6`/`MB.8`
  own the static page and the Django leg.
- **`--learn` / `check` attribution:** what a PHP route contributes to the wire
  record, how a request is attributed to a route when `PATH_INFO` is the app's own
  URL, what `check` reports for an unexercised PHP route (`--json` shape and exit
  code), and how `--learn` writes the route/static/proxy lines in one pass without a
  per-language plugin.
- **CVE defence table:** technique → blastit control → the e2e row that proves it
  (`MB.3.7`), every row traceable to `docs/research/php2shell.md`.

## The backend contract (DI/IoC) — normative section of the same document

Owner requirement, verbatim intent: *"DI/IoC in the spirit. Each language/server
interface provides a same trait implementation."* The design must pin all of this;
`MB.9` implements it and `MB.3.6` makes FastCGI one more implementation of it.

- **Current reality, stated honestly.** Static is *not* injected today:
  `crates/blastit-scheduler/src/static_asset.rs` (`AnswerKind`, resolution) and
  `static_response.rs` are called directly from `scheduler.rs` / `rewrite.rs`;
  `RouteInfo` carries `Option<StaticPolicy>` and `static_forward`, and dispatch
  branches in place. The WSGI path is likewise wired in place inside the view
  request path. There is no contract crate. The retrofit is real work, owned by
  `MB.9`; the design must not describe today's code as if it were already DI.
- **The trait.** `pub trait Upstream` in `crates/blastit-backend`, object-safe,
  knowing nothing about PHP, Python, static files or any server:
  - `fn kind(&self) -> &'static str` — the manifest backend name, for events only.
  - `fn plan(&self, head: &RequestHead) -> Result<UpstreamPlan, UpstreamError>` —
    validate the request head and state body/ingress expectations
    (`BodyMode::None | Buffered { max } | Stream { credit }`, `expects_continue`,
    method/body refusal). Planning is synchronous and allocation-bounded; a refusal
    is a client-visible `400`/`405`/`413`, never an internal drop with no status.
  - `fn exchange<'a>(&'a self, plan: UpstreamPlan, body: BodyStream<'a>,
    sink: &'a mut dyn ResponseSink) -> BoxFuture<'a, Result<ExchangeOutcome,
    UpstreamError>>` — perform the exchange under the existing credits and return
    what the implementation observed. Object safety is explicit: a `BoxFuture`
    alias (`Pin<Box<dyn Future<Output = …> + Send + 'a>>`), **no `async_trait`, no
    new dependency**; the document names the fallback if the alias cannot be made
    to work on MSRV 1.88.
  - `ExchangeOutcome { status, bytes_in, bytes_out, protocol_status,
    observations: Vec<Observation> }`; an implementation may not write events,
    push metrics or touch the manifest — it returns data.
  - `Observation` is **language-agnostic**: `kind ∈ { WireRequest, Query, Param,
    StaticFile, Output }` plus `target`, optional `detail`. There is no
    `PhpParam`/`SqlQuery` variant; `--learn`/`check` consume the same records
    whatever implements the trait, so neither grows a PHP-specific code path.
  - `UpstreamError` carries the **client-visible** status (`Refused { status,
    reason }`, `Unavailable`, `Fault`). Error mapping lives in the contract crate so
    the conformance table can assert it once for every implementation.
- **Injection, not selection.** `RouteInfo` holds `upstream: Arc<dyn Upstream>`
  (non-optional, no `Option<Box<dyn …>>` unwrapped on the hot path), built exactly
  once at boot. Dispatch calls `plan` then `exchange`; the document states, in one
  sentence, that dispatch **never `match`es on backend kind** — no `cfg!`, no
  `match backend_kind`, no globals/`lazy_static`/`OnceCell` registry lookup per
  request. An unknown backend fails closed **at boot** with an error naming the
  manifest line (`./blastit.toml:<line>: unknown backend "<kind>" …`).
- **Factory/registry.** `crates/blastit-backend` defines `BackendBuilder` (name →
  `Arc<dyn Upstream>`) and `Registry::build(&Manifest) -> Result<Vec<Arc<dyn
  Upstream>>, BootError>`; the composition root (`crates/blastit`) registers static,
  WSGI and (later) FastCGI builders. Duplicate registration and unknown kind are
  boot errors naming the manifest line; there is no fallback backend.
- **Crates by seam, dependencies inward.** `blastit-backend` depends only on
  `blastit-http` (head/status types) and `blastit-manifest` *types*; it may never
  depend on `blastit-scheduler`, `blastit-view`, `blastit-fastcgi` or `blastit`.
  `blastit-fastcgi` (codec in `MB.3.4`, adapter in `MB.3.6`) depends on the
  contract, never the reverse. Static and WSGI adapters live where their I/O already
  lives (`blastit-scheduler`, `blastit-view`) and also depend inward.
- **One shared conformance table.** `crates/blastit-backend/src/conformance.rs`
  holds the behavioural cases every implementation must satisfy: refusals (status +
  reason), body caps, observation record shape, and error mapping. Each
  implementation registers its row; the table is the single artefact that stops
  FastCGI drifting from static/WSGI. `--learn`/`check` get no per-backend branch.
- **Ports without behaviour change.** The design states the invariants the ports
  must preserve: static refusal statuses (`400`/`404`), immutable/no-cache headers,
  `ETag`/`304`/`HEAD`, directory-index vs SPA-precedence, and forward-to-app on a
  miss; WSGI forwarding semantics unchanged (status, hop-by-hop stripping, body
  credit, `app.output`). Protected suites
  (`crates/blastit-scheduler/tests/critical_static_assets.rs`,
  `critical_static_resolve.rs`, `critical_rewrite.rs`,
  `critical_ingress_enforcement.rs`) must pass **unmodified**. If a protected
  contract genuinely has to change, **stop** and report it as its own node.
- **ADR-0008 records the split.** The contract crate, the static port, the WSGI
  port and the conformance suite land as their own epic **`MB.9` under `MB`**, which
  must complete **before `MB.4` (Node) is implemented**; FastCGI implements the same
  trait inside `MB.3` (`MB.3.6` adds its conformance row). The ADR also records the
  FastCGI decisions (client in the scheduler, pure codec crate, per-route block,
  pinned `SCRIPT_FILENAME`, param allowlist, pool ownership).
- Fresh-context critics with lenses `security`, `implementability`, `testability` and
  **`consistency`** write the four `docs/design/php-fastcgi.review-*.md` files; the
  `consistency` lens covers language-agnosticism (no per-backend variant in the
  scheduler, one observation vocabulary, one error mapping). Resolve every
  `BLOCKER`/`MAJOR` and list finding → fix under `## Review Resolution` in
  `RESULT.md`.

# Acceptance

```console
$ test -f docs/design/php-fastcgi.md && test -f docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md && echo "design + ADR exist"
$ for k in 'route.fastcgi' 'BEGIN_REQUEST' 'requestId' 'SCRIPT_FILENAME' 'PATH_INFO' 'DOCUMENT_ROOT' 'REDIRECT_STATUS' 'PHP_ADMIN_VALUE' 'httpoxy' 'auto_prepend_file' 'public/index.php' 'route.static' 'ABORT_REQUEST'; do grep -q -- "$k" docs/design/php-fastcgi.md || echo "missing: $k"; done
$ grep -q 'CVE-2012-1823' docs/design/php-fastcgi.md && grep -q 'CVE-2024-4577' docs/design/php-fastcgi.md
$ test "$(grep -c '^|' docs/design/php-fastcgi.md)" -ge 30 && echo "param contract, refusal matrix and CVE table are tables"
$ grep -q 'trait Upstream' docs/design/php-fastcgi.md && echo "one contract trait pinned"
$ grep -q 'Arc<dyn Upstream>' docs/design/php-fastcgi.md && echo "per-route injected handle pinned"
$ grep -qi 'conformance' docs/design/php-fastcgi.md && echo "shared conformance table pinned"
$ grep -q 'never `match`es on backend kind' docs/design/php-fastcgi.md && echo "no backend-kind dispatch pinned"
$ grep -q 'MB.9' docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md && grep -q 'before .*MB.4' docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md && echo "ADR-0008 records the split and its order"
$ for f in docs/design/php-fastcgi.review-security.md docs/design/php-fastcgi.review-implementability.md docs/design/php-fastcgi.review-testability.md docs/design/php-fastcgi.review-consistency.md; do tail -1 "$f" | grep -q 'VERDICT: PASS'; done && echo "four lenses pass"
$ grep -q 'MB.3.7' docs/design/php-fastcgi.md && grep -q 'e2e/php2shell.py' docs/design/php-fastcgi.md
$ grep -n 'php-fastcgi' docs/design/README.md && grep -n 'ADR-0008' docs/decisions/README.md
$ make docs-check
```

# Notes

- No code, no tests, no manifest changes: two documents, four review artefacts, two
  index rows.
- Keep it terse: tables and signatures, not narrative. Kernel/privilege mechanics stay
  in `ARCHITECTURE.md` (synced by `MB.3.8`), not repeated here.
- Give the implementer **file-level seams**: `crates/blastit-fastcgi` (pure codec +
  guards), `crates/blastit-manifest` (grammar), `crates/blastit-scheduler`
  (`RouteInfo`, dispatch, session), `crates/blastit` (spawn, `dev`/`check`/`--learn`),
  `crates/blastit-backend` (the contract, `MB.9.1`). If the design cannot be
  implemented inside those seams, say so before code.
- Every refusal row must name a client-visible outcome; "rejected internally" with no
  status is not testable and will be sent back by the `testability` lens.
- The `consistency` lens exists because the same trait must carry three languages: a
  design that gives FastCGI its own dispatch path, its own observation record or its
  own error enum fails it, and `MB.9` will not be buildable.
- `docs/design/` and `docs/decisions/` are excluded from the public site; `make
  docs-check` only proves the site still builds.
