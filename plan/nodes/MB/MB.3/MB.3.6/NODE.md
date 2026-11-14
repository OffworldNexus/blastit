---
id: MB.3.6
title: "Dispatch FastCGI routes and wire dev/check/--learn PHP attribution"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.3.5]
paths:
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-scheduler/tests/critical_fastcgi_dispatch.rs"
  - "crates/blastit/src/**"
  - crates/blastit/tests/critical_fastcgi_learn.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T01:09"
---

# Goal

Make the PHP route real end to end inside the product: the scheduler connects to the
route's `php-fpm` pool over its Unix socket, sends `BEGIN_REQUEST` → `PARAMS` →
`STDIN`, pumps `STDOUT` back as the HTTP response, turns `STDERR` into `app.output`
events and `END_REQUEST` into accounting; `blastit dev` runs a PHP route beside
`php-fpm` with zero `sudo`, and `blastit check` / `--learn` attribute PHP routes and
their grants like every other backend.

# Context

- Design of record: `docs/design/php-fastcgi.md` (accepted) — session/framing contract,
  per-route pool ownership, and the exact `--learn`/`check` attribution rules. The
  grammar arrives from `MB.3.5`, the codec and guards from `MB.3.4`; this node wires
  them and adds no third copy of either.
- **Scheduler** (`crates/blastit-scheduler`): extend the route info with the FastCGI
  policy; a dispatched request opens a session, sends the param stream produced by
  `blastit-fastcgi`'s lockdown, streams the request body as `STDIN` under the existing
  credit, parses the CGI-style `STDOUT` head (`Status:` or the design's default,
  `Content-Type`, hop-by-hop stripping), pumps the body under the existing response
  credit, and finishes on `END_REQUEST`. Client disconnect sends `ABORT_REQUEST` and
  releases the slot; a worker that closes mid-response is a `502`, never a hang and
  never a leaked slot (see `docs/knowledge/LESSONS.md` on slots that are never
  released). `STDERR` lines become `app.output` events with the existing cap.
- **Pool workers**: spawn/connect per the design (who owns the child, which uid, socket
  permission check, restart, `BLASTIT_MAX_WORKERS` accounting). If the design places
  the spawn in the supervisor rather than the scheduler, that is still this node.
- **Static interaction**: the `0.2.0` responder keeps answering `public/` assets; the
  entry script (`*.php` under the static root) must never be served as a file — prove
  the rule at this boundary as well as in the manifest test.
- **CLI** (`crates/blastit`): `dev` boots a route whose backend is `fastcgi` (no
  Python callable required, no `app` line), `--learn` records the request and every
  outgoing operation with PHP route attribution and writes manifest edits in the same
  one-pass shape as the WSGI path, and `check` reports an unexercised PHP route, a
  missing grant and a dead grant with the documented `--json` finding shape and exit
  code. PHP has no pytest suite: `check` must not treat "no pytest" as "nothing to
  check" — the design says what replaces it.
- Protected tests in the same node: `crates/blastit-scheduler/tests/critical_fastcgi_dispatch.rs`
  (new) and `crates/blastit/tests/critical_fastcgi_learn.rs` (new). Both are additions;
  no existing `critical_*.rs` may change.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_fastcgi_dispatch --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_fastcgi_learn --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit --locked
$ make rs-lint
$ make critical-guard BASE=develop NODE=MB.3.6
$ make rs-test
$ make check
```

# Notes

- Use an **in-process fake FastCGI peer** (a Unix socketpair serving canned
  `STDOUT`/`END_REQUEST` frames) for the scheduler suite: deterministic, no Docker, no
  `php-fpm` in the unit gate. The real `php-fpm` image is `MB.3.7`'s job only.
- Every failure mode needs a test: bad `BEGIN_REQUEST` reply, `STDOUT` with no
  `Content-Type`, body larger than the credit, worker exit before `END_REQUEST`,
  client cancel, `STDERR` flood, and a refused param reaching the wire (it must not).
- Do not touch `crates/blastit-fastcgi/**`, `crates/blastit-manifest/**`,
  `docs/**`, `e2e/**`, or `Cargo.toml`/`Cargo.lock` — if a dependency is genuinely
  required, stop and log a follow-up (the lockfile is `MB.3.4`'s and `MB.3.8`'s).
- The no-sudo `dev` promise is the page's headline: if the pool cannot be started
  unprivileged as designed, stop and `human add` the conflict instead of documenting
  or shipping a `sudo` step.
- Budget ~600 changed lines. If the CLI half would push past it, land the scheduler
  session + dispatch + its critical suite first, leave `check`/`--learn` attribution
  as an explicit follow-up child in `RESULT.md`, and say so — a half-wired `check`
  that reports PHP routes as unexercised-but-green is worse than a scoped gap.
