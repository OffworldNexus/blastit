---
id: MB.3.3
title: "Design the FastCGI session, [route.fastcgi] grammar and param lockdown (ADR-0008)"
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
  - docs/design/README.md
  - "docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md"
  - docs/decisions/README.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T01:09"
---

# Goal

Turn the accepted `docs/frameworks/php.md` into `docs/design/php-fastcgi.md` plus
`ADR-0008-fastcgi-backend-and-param-lockdown.md`: the exact `[route.fastcgi]`
grammar, the FastCGI session/framing contract, the param allowlist with a **binary
refusal matrix**, per-route pool workers, `--learn`/`check` attribution for PHP
routes, and the CVE defence table — pinned to the shipped supervisor / `RawFrame`
scheduler, with no second supervisor and no second static responder.

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
- **ADR-0008** records the lasting decisions only (FastCGI client in the scheduler,
  pure codec crate, per-route block, pinned `SCRIPT_FILENAME`, param allowlist,
  pool ownership), one page, `ADR-NNNN-<slug>.md` per skill `design-docs`.
- Fresh-context critics with lenses `security`, `implementability`, `testability` write
  the three `docs/design/php-fastcgi.review-*.md` files; resolve every `BLOCKER`/
  `MAJOR` and list finding → fix under `## Review Resolution` in `RESULT.md`.

# Acceptance

```console
$ test -f docs/design/php-fastcgi.md && test -f docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md && echo "design + ADR exist"
$ for k in 'route.fastcgi' 'BEGIN_REQUEST' 'requestId' 'SCRIPT_FILENAME' 'PATH_INFO' 'DOCUMENT_ROOT' 'REDIRECT_STATUS' 'PHP_ADMIN_VALUE' 'httpoxy' 'auto_prepend_file' 'public/index.php' 'route.static' 'ABORT_REQUEST'; do grep -q -- "$k" docs/design/php-fastcgi.md || echo "missing: $k"; done
$ grep -q 'CVE-2012-1823' docs/design/php-fastcgi.md && grep -q 'CVE-2024-4577' docs/design/php-fastcgi.md
$ test "$(grep -c '^|' docs/design/php-fastcgi.md)" -ge 30 && echo "param contract, refusal matrix and CVE table are tables"
$ grep -q 'MB.3.7' docs/design/php-fastcgi.md && grep -q 'e2e/php2shell.py' docs/design/php-fastcgi.md
$ for f in docs/design/php-fastcgi.review-security.md docs/design/php-fastcgi.review-implementability.md docs/design/php-fastcgi.review-testability.md; do tail -1 "$f" | grep -q 'VERDICT: PASS'; done && echo "three lenses pass"
$ grep -n 'php-fastcgi' docs/design/README.md && grep -n 'ADR-0008' docs/decisions/README.md
$ make docs-check
```

# Notes

- No code, no tests, no manifest changes: two documents, three review artefacts, two
  index rows.
- Keep it terse: tables and signatures, not narrative. Kernel/privilege mechanics stay
  in `ARCHITECTURE.md` (synced by `MB.3.8`), not repeated here.
- Give the implementer **file-level seams**: `crates/blastit-fastcgi` (pure codec +
  guards), `crates/blastit-manifest` (grammar), `crates/blastit-scheduler`
  (`RouteInfo`, dispatch, session), `crates/blastit` (spawn, `dev`/`check`/`--learn`).
  If the design cannot be implemented inside those four seams, say so before code.
- Every refusal row must name a client-visible outcome; "rejected internally" with no
  status is not testable and will be sent back by the `testability` lens.
- `docs/design/` and `docs/decisions/` are excluded from the public site; `make
  docs-check` only proves the site still builds.
