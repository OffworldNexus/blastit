---
id: MB.3.9
title: "blastit dev spawns, configures, supervises and cleans up the php-fpm pool itself"
kind: epic
status: ready
priority: 1
depends_on: [MB.3.6]
paths:
  - docs/frameworks/php.md
  - "docs/design/php-fastcgi.md"
  - "docs/decisions/**"
  - "crates/blastit/src/**"
  - "crates/blastit/tests/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T13:17"
updated: "2026-11-14T13:17"
---

# Goal

Make `blastit dev` own the PHP pool end to end, so a PHP developer runs **two
commands**: write `blastit.toml`, run `blastit dev`. `blastit dev` generates the pool
config, finds the binary, spawns, supervises, reaps, group-kills and cleans up — exactly
as it already does for Python view workers. Nothing is hand-written, nothing needs
`sudo`, nothing lands in `$HOME`.

Owner correction (2026-11-14): the accepted `docs/design/php-fastcgi.md` §4.8.1 — "the
developer writes `php-fpm.conf`, runs `php-fpm -F -y php-fpm.conf`, and `blastit dev`
never spawns a pool" — is **rejected as developer experience**. "Everything needs to be
automatic."

This epic does **not** re-decide the runtime-provider architecture: epic `MB.12`
(`docs/design/backend-runtimes.md` + ADR) owns "who owns the app process, who strips
secrets, how a per-route uid maps onto a runtime". This epic is the first real consumer
of that contract for `dev`, and must implement the dev lifecycle as **the thing a
provider does**, so the same code serves production with privileges dropped.

# Context

- The whole design change is one sentence: **`blastit dev` flips from `PoolSpawn::External`
  to managed**, reusing the production spawn/supervise code minus the privilege drop,
  the cgroup and the capability steps. `check` keeps the external-executor shape it needs
  (it must not spawn a pool inside a child `dev` that the suite will kill).
- That has a second-order consequence the later leaves must not forget: `DevProbe::check`
  stops being "a socket must already exist, be `S_ISSOCK` and be connectable" and starts
  being "the run directory is ours to create and the leaf is absent or stale". The
  `listen` boot-fatal rows in §4.1 are re-read against the new owner, not silently kept.
- `docs/frameworks/php.md` is the only page that teaches the by-hand pool. Its Local
  section must shrink to the manifest plus `blastit dev`; the `php-fpm.conf` block, the
  `mkdir` + `php-fpm -F` commands and the `uv init --bare`/`uv add` ceremony go. The
  distribution story (which package a host installs) belongs to `MB.12` — do not invent
  npm/standalone commands here.
- Ordering is DX-first: the **page** is rewritten first (target DX), then the design/ADR
  amendment, then the code, then the failure-mode tests.
- Constraints that do not move: protected `critical_*` suites are never edited to pass;
  no backend-kind `match` in dispatch; FastCGI stays an implementation of the injected
  `Upstream`; `make check` per commit; local commits on `release/0.2.1` only.

# Acceptance

- [ ] `MB.3.9.1` (page) `done`: `docs/frameworks/php.md` Local is two steps, the by-hand
  pool is gone, CI/Deploy stay consistent, and the revised page passes fresh `dx` and
  `ai-ergonomics` critics. The test is "a PHP developer with a Symfony app runs two
  commands and sees their app".
- [ ] `MB.3.9.2` (design) `done`: `docs/design/php-fastcgi.md` §4.8.1 rewritten and
  `ADR-0008` carries an explicit amendment banner covering the dev lifecycle, provider
  reuse, the binary search order, the cleanup contract and the error contract — written
  against `MB.12`'s runtime-provider model, not against a one-off in the scheduler.
- [ ] `MB.3.9.3` (code) `done`: `blastit dev` generates, spawns, supervises and cleans up
  one pool per FastCGI route through the **same** code path production uses (privilege
  drop, cgroup and capability steps skipped), and `check` keeps its external-executor
  branch.
- [ ] `MB.3.9.4` (tests) `done`: the five failure modes are named tests — missing
  `php-fpm` binary; a pool that exits immediately (diagnostic names the generated config
  path and the child's last stderr lines); two routes → two pools with distinct sockets;
  `SIGTERM` leaves no `php-fpm` process, no socket and no run directory; and a
  security-relevant subset in the protected tier.
- [ ] `make check` passes at every commit; `make docs-check` passes; the page's own
  commands are what the runnable-docs fences now assert.

# Notes

- The manifest grammar is the owned-socket shape (`MB.3.9.2`): `entry` plus an optional,
  budgeted `workers`. `listen` does not exist — the socket is blastit's, derived per route
  inside the private run dir — and there is still no `[route.fastcgi.env]`. The generated
  conf is derived from `workers` and `entry` and blastit's worker budget.
- The generated pool config goes to a private, per-run location (e.g.
  `<root>/.blastit/run/<route>/fpm.conf`, `0700`), is never written to `$HOME`, never
  needs `sudo`, and never overwrites a user file.
- `MB.3.5.2` and `MB.3.6.3` implement the accepted §4.8.1 text ("dev never spawns") as
  written; this epic supersedes that sentence and must say so in its amendment rather
  than leaving two truths in one document.
