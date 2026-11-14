---
id: MB.3.9.3
title: "Amend design 4.8.1 + ADR-0008: dev spawns the pool via the runtime provider (discovery, cleanup, error contract)"
kind: task
status: ready
owner: designer
priority: 1
depends_on: [MB.12, MB.3.9.2]
paths: ["docs/design/php-fastcgi.md", "docs/decisions/**", "docs/design/php-fastcgi.review-*.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T13:18"
updated: "2026-11-14T13:18"
---
# Goal

Amend `docs/design/php-fastcgi.md` §4.8.1 and `ADR-0008` for the **automatic dev pool**:
`blastit dev` spawns, configures, supervises, reaps and cleans up the php-fpm pool, as the
thing a runtime provider does — the same code path production uses, minus the privilege
drop, the cgroup and the capability steps — and the **runtime binary is resolved once, at
boot, by a provider-declared resolver**.

# Context

Owner correction (2026-11-14): §4.8.1's "the developer writes `php-fpm.conf`, runs
`php-fpm -F -y`, and `blastit dev` never spawns a pool" is rejected as developer
experience — "everything needs to be automatic". The developer writes a manifest and runs
`blastit dev`.

Authority for app-process ownership is `MB.12`'s `docs/design/backend-runtimes.md` + ADR
(the `RuntimeProvider` contract). Write this amendment **against that seam**; do not invent
a competing abstraction, and do not leave a second pool lifecycle in the scheduler. If
`MB.12` has not landed, escalate rather than write a design that will be rewritten.

Pin, at implementable precision:

1. **Generated config, derivable.** Every line is a function of the manifest + blastit's
   budget: `pm = static`, `pm.max_children` = the resolved worker value from `MB.3.9.2`'s
   budget mapping, `listen` = the derived per-route socket inside the private run dir
   (`<run>/<route>/fpm.sock`), `listen.mode = 0600`, `security.limit_extensions = .php`,
   `catch_workers_output = yes`, `clear_env = yes`, `error_log` to blastit's stream, and
   the same `php_admin_value` backstops §4.8.2 lists (a `.user.ini` must not re-open the
   chain in `dev` either). Location: `<root>/.blastit/run/<route>/fpm.conf`, parent
   `0700`, never `$HOME`, never `sudo`, never overwriting a user file, cleaned on exit.
2. **Runtime discovery — one generic resolver, provider-declared.** The provider owns its
   search names and its override variable; the resolver is shared, so Node later declares
   its own names instead of inventing a second mechanism:
   - Search order: `php-fpm`, then versioned (`php-fpm8.4`, `php-fpm8.3`, …) **in `PATH`**;
     the first executable wins. Document the exact order and the behaviour with two
     candidates (first wins, deterministic; log the resolved absolute path).
   - **One override, environment only:** `BLASTIT_PHP_FPM=/opt/php/bin/php-fpm`, in the
     same `BLASTIT_*` family as every other setting. It is **not** a manifest key — the
     manifest stays about the app.
   - **Validated at boot, never at first request:** the override is resolved and checked
     (`is a file`, executable, `--version` runs) before any pool spawns. A bad value fails
     naming the variable, the value and the reason. An absent runtime fails naming the
     install package for the host's distro family **and** the override as the escape hatch
     for a non-standard install. Never a stack trace, never a silent fallback.
   - **One resolution for `dev`, `check` and production**, so the three cannot disagree
     about which binary runs. The resolved absolute path goes in the boot line and the
     `--json` document, so debugging needs no guessing.
   - **Env-read ordering is part of the contract:** `BLASTIT_*` is read in the stage-0
     configuration composition root, *before* the stage-1 `execve` swaps/strips the
     environment (the `Config::for_dev()` rule in §4.8.1). The resolved path travels in the
     typed config, never as a re-read after the barrier.
3. **Lifecycle.** Spawn per route at start or lazily per the accepted `prefork` decision;
   `setpgid`; reap on exit; `SIGTERM`/`SIGINT`/`dev`-return group-kill; socket and run dir
   removed; child stderr surfaced through blastit's log tagged with the route name. State
   exactly which steps are shared with production and which are skipped, and why skipping
   is safe in `dev` (single uid, `dev` already refuses non-loopback binds).
4. **Failure diagnostics.** A pool that dies at startup prints the generated config path
   and the child's last stderr lines (bounded; no param/header/secret values — §4.8.2's
   log rule still applies).
5. **`check` and `e2e`.** Which branch `check` keeps (it must not spawn a pool inside the
   child `dev` it will kill) so `PoolSpawn::External` survives for `check`, not for `dev`;
   and what `MB.3.7` must now replay instead of "start php-fpm by hand".
6. **Probe consequences.** `DevProbe::check` stops being "a socket must already exist, is
   `S_ISSOCK` and is connectable" and becomes "the run dir is blastit's to create and the
   leaf is absent or stale". Rewrite the §4.1 `listen` boot-fatal rows accordingly — the
   rows `MB.3.5.2` was given are otherwise stale, so name that leaf explicitly.
7. **Who limits, and what a request costs.** One sentence each, so the two rules cannot
   drift: the scheduler is the only limiter (the pool's `pm.max_children`/`RLIMIT_NPROC`
   are ceilings ≥ the route's slot allocation, and an under-provisioned provider is a
   boot-time refusal naming the mismatch); and the per-request cost travels on the
   existing fair-queuing path (`Cost` + the `RESP_END_COST` clamp), with an external pool
   reporting `cpu_ns: null` and being charged observed wall time. The **generic**
   measurement interface belongs to `MB.12`'s provider contract — say so in the same
   ADR bullet, alongside the generic resolver, so a runtime blastit does not own yields a
   cost by default rather than being a special case.

# Acceptance

```console
$ make docs-check
$ make check
```
- §4.8.1 rewritten; `grep -n 'never spawn a pool' docs/design/php-fastcgi.md` matches only
  the amendment's own history note.
- The discovery subsection names the provider-declared shape (`search names` + `override
  variable`), so `MB.12` can adopt it as the contract rather than invent a second resolver;
  `ADR-0008`'s bullet says so in one sentence.
- `ADR-0008` carries a second dated amendment bullet (same "Amends." style) naming this
  node, `MB.12` and the provider dependency.
- The amended §4.1 probe rows and §8.1 checklist for `MB.3.5.2` are consistent with the
  derived-socket grammar from `MB.3.9.2` — no sentence in the file has a user-authored
  socket.
- Fresh goldfish critics PASS: `implementability`, `security`, `testability`,
  `consistency`; every `BLOCKER`/`MAJOR` resolved in the document.

# Notes

- Scoped design, not a core-architecture rewrite: `MB.12` owns the provider contract and
  gets `architect` acceptance there. A **gap in `MB.12`'s** contract (not a detail of this
  feature) is surfaced to the lead, not designed around.
- Keep §4.8.2/§4.8.3's fd and socket lifecycle normative: the derived socket still goes
  through the same spawn transition and the same `PoolReady` gate, and `dev` still
  connects only via `/proc/self/fd/<parentfd>/<name>`.
