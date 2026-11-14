---
id: MB.3.9.2
title: "Amend ADR-0008 + design 4.1: drop listen, derive the pool socket, optional budgeted workers"
kind: task
status: done
owner: designer
priority: 1
depends_on: []
paths: ["docs/design/php-fastcgi.md", "docs/decisions/**", "docs/design/php-fastcgi.review-*.md"]
attempts: 4
max_attempts: 4
escalation: none
created: "2026-11-14T13:17"
updated: "2026-11-14T14:19"
---
# Goal

Amend `ADR-0008` §4.1 and `docs/design/php-fastcgi.md` §4.1 to the **owned-socket** grammar:
`[route.fastcgi]` has `entry` and an optional `workers`. `listen` is gone from the
manifest, because blastit spawns and supervises the pool and therefore owns the socket.

# Context

Owner decision (2026-11-14), verbatim intent: "blastit spawns php-fpm, right? and should
be controlling the workers and the budget like it does with WSGI? so what is this even
doing here?" A knob a developer cannot act on correctly is a defect: a user-authored
`listen` lets them point two routes at one pool, mis-set `listen.mode`, or place a
world-writable parent — all of which blastit would then have to police. Remove it and the
class disappears. `ADR-0008` was accepted, so this is a **documented amendment**, never a
silent edit.

What the amendment must pin:

1. **No `listen` key.** The socket path is derived per route inside blastit's private run
   directory (`<run>/<route>/fpm.sock`, parent `0700`, created by blastit). `listen.mode`
   is blastit's `0600` + the §4.8.3 owner discipline; it is no longer a thing a manifest
   can influence. A manifest that still writes `listen = "…"` is an **unknown key**,
   refused with the key named. State that the derived path is not relative to
   `blastit.toml` and never user-visible on success.
2. **`workers` optional, derived from blastit's budget.** Absent = derived. The mapping
   must be the *same* budget WSGI already uses, and must be stated numerically with the
   code it comes from: `Config::view_worker_cap()` (`crates/blastit/src/config.rs:1407`)
   = `BLASTIT_VIEW_MAX_WORKERS`, defaulting to `BLASTIT_MAX_WORKERS` (`:673-677`,
   default 16); the pool's `pm.max_children` = that route's share, and the pool is a
   view-role process (decision 6), so the share is that same per-view cap. The scheduler's
   global `slots < max_workers` gate remains the admission truth for every backend; the
   pool is sized so the pool is never the bottleneck. `RLIMIT_NPROC = workers + 2`
   (`config.rs:1313`) follows the resolved value, not the manifest's.
3. **What happens when the resolved value exceeds the budget.** Choose one and justify it
   in the ADR with evidence: refuse at boot naming both numbers (fail closed, no silent
   clamp), or clamp with a boot log line. State the interaction with the explicit override
   (`workers = 32` while the budget is 16) and with two FastCGI routes sharing one budget.
4. **Keep `workers` only if a real deployment constraint needs it.** If it must stay, the
   ADR says why, and the key is documented as "usually omit" with blastit's derived value
   shown in the boot line/log. If it cannot be justified, delete it and say so — but
   `listen` is gone either way.
5. **Bookkeeping.** `ADR-0008` gains a dated **Amends** bullet in its existing "Amends."
   style naming this node; the design's §4.1 grammar block, key table, boot-fatal list and
   error-variant table, and §8.1/§8.2 checklists lose every `listen` row and stop pinning
   `workers = 4` as a default; §4.8.1/§4.8.2/§4.8.3 and §4.1's probe text are re-read so no
   sentence still has a caller-supplied socket path.

# Acceptance

```console
$ make docs-check
$ make check
```
- `grep -n 'listen' docs/design/php-fastcgi.md` shows only derived-socket prose (no
  manifest key, no error variant naming a user `listen`), and `ADR-0008` carries the
  dated amendment bullet.
- §8.1's executable checklist for `MB.3.5.1`/`MB.3.5.2` matches the new grammar exactly,
  so the leaf rework has one spec.
- Fresh goldfish critics PASS on the amendment: `implementability` (can a coder implement
  it with no remaining question), `consistency` (no surviving `listen`/`workers = 4`
  sentence anywhere in the design), `security` (a derived socket cannot be redirected,
  shared or re-permissioned by a manifest), `testability` (every rule above is a named
  test). Resolve every `BLOCKER`/`MAJOR` in the document.

# Notes

- The dev-lifecycle half of the amendment (who spawns in `dev`, discovery order, cleanup,
  error contract) is a **separate node**, `MB.3.9.3`, because it depends on `MB.12`'s
  runtime-provider contract. Do not write it here; do not contradict it either.
- `MB.3.5.1`'s implementer is already applying the new grammar against this decision, so
  land this quickly and make the document match the code that is being written — the
  reviewer will read both.
