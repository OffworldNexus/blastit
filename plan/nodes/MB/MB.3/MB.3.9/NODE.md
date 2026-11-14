---
id: MB.3.9
title: "blastit dev runs, supervises and cleans up the embedded PHP workers itself"
kind: epic
status: ready
priority: 1
depends_on: [MB.3.6]
paths:
  - docs/frameworks/php.md
  - "docs/design/backend-runtimes.md"
  - "docs/decisions/**"
  - "crates/blastit/src/**"
  - "crates/blastit/tests/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T13:17"
updated: "2026-11-14T22:18"
---

# Goal

Make `blastit dev` own the PHP worker set end to end, so a PHP developer runs **two
commands**: write `blastit.toml`, run `blastit dev`. `blastit dev` discovers the host NTS
`libphp`, forks, recycles, supervises, reaps, group-kills and cleans up the per-route
`EmbeddedView` workers — exactly as it already does for Python view workers, with no
external PHP process. Nothing is hand-written, nothing needs `sudo`, nothing lands in
`$HOME`.

Owner correction (2026-11-14): the then-accepted design's §4.8.1 (deleted with `ADR-0010`)
— "the developer writes the PHP server config, runs the PHP server by hand, and
`blastit dev` never spawns one" — was **rejected as developer experience**. "Everything
needs to be automatic."

This epic does **not** re-decide the runtime-provider architecture: epic `MB.12`
(`docs/design/backend-runtimes.md` + ADR) owns "who owns the app process, who strips
secrets, how a per-route uid maps onto a runtime". This epic is the first real consumer
of that contract for `dev`, and must implement the dev lifecycle as **the thing a
provider does**, so the same code serves production with privileges dropped.

# Context

- The whole design change is one sentence: **`blastit dev` runs the embedded PHP workers
  through the production spawn/supervise path**, minus the privilege drop, the cgroup and the
  capability steps (ADR-0010 §1). `check` keeps its app-less branch (it must not spawn a
  second PHP worker set inside a child `dev` that the suite will kill).
- That has a second-order consequence the later leaves must not forget: `DevProbe::check`
  stops being "a socket must already exist, be `S_ISSOCK` and be connectable" and starts
  being "`libphp` and `entry` are ours to check and the worker is absent or stale". There
  is no `listen` key in `[route.php]` — ADR-0010 §2 rejects it as an unknown route key.
- `docs/frameworks/php.md` is the only page that teaches the by-hand server. Its Local
  section must shrink to the manifest plus `blastit dev`; the hand-written config block, the
  `mkdir` + start-the-server commands and the `uv init --bare`/`uv add` ceremony go. The
  distribution story (which package a host installs) belongs to `MB.12` — do not invent
  npm/standalone commands here.
- Ordering is DX-first: the **page** is rewritten first (target DX), then the design/ADR
  amendment, then the code, then the failure-mode tests.
- Constraints that do not move: protected `critical_*` suites are never edited to pass;
  no backend-kind `match` in dispatch; PHP stays an implementation of the injected
  `Upstream` (ADR-0009's `ViewUpstream` over `WorkerRuntime`); `make check` per commit; local
  commits on `release/0.2.0` only.

# Acceptance

- [ ] `MB.3.9.1` (page) `done`: `docs/frameworks/php.md` Local is two steps, the by-hand
  pool is gone, CI/Deploy stay consistent, and the revised page passes fresh `dx` and
  `ai-ergonomics` critics. The test is "a PHP developer with a Symfony app runs two
  commands and sees their app".
- [ ] `MB.3.9.2` (design) `done`: `ADR-0008` carries an explicit amendment banner covering
  the dev lifecycle, provider reuse, the library search order, the cleanup contract and the
  error contract — written against `MB.12`'s runtime-provider model, not against a one-off in
  the scheduler. That amendment and the design it amended are both superseded by `ADR-0010`.
- [ ] `MB.3.9.3`–`MB.3.9.5` are `superseded` (their pool-spawn design went with the external
  path, `MB.3.17`); the lifecycle they described is delivered by the embedded worker
  (`MB.3.13.1`/`MB.3.13.3`) and proven end to end by `MB.3.7`: `blastit dev` runs one PHP
  worker set per `[route.php]` route through the **same** code path production uses
  (privilege drop, cgroup and capability steps skipped), and `check` keeps its app-less
  branch.
- [ ] The five failure modes are named tests — missing NTS `libphp` (boot refusal naming
  the distro package and `BLASTIT_LIBPHP`); a worker that exits immediately (diagnostic names
  `entry` and the worker's last stderr lines); two `[route.php]` routes → two worker sets
  with distinct uids; `SIGTERM` leaves no PHP process and no run directory; and a
  security-relevant subset in the protected tier.
- [ ] `make check` passes at every commit; `make docs-check` passes; the page's own
  commands are what the runnable-docs fences now assert.

# Notes

- The manifest grammar is ADR-0010 §2: `entry` plus an optional, budgeted `workers`.
  `listen`/`socket`/`port`/`env` do not exist (all rejected as unknown route keys), and
  there is no external server config: the worker count is derived from `workers` and
  blastit's worker budget.
- The per-route private run directory (e.g. `<root>/.blastit/run/<route>`, `0700`) holds
  the view tmp/session paths, is never written to `$HOME`, never needs `sudo`, and never
  overwrites a user file.
- `MB.3.17` removed the external path; the dev lifecycle must be written against
  `ADR-0010` rather than leaving two truths in one document, and the historical briefs
  (`MB.3.5.2`, `MB.3.6.3.*`) are records, not instructions.
