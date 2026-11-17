---
lens: security
document: docs/design/parallel-views.md
adr: docs/decisions/ADR-0015-multi-worker-views.md
round: 4 (closeout)
---

# Security review, closeout — `parallel-views.md` / ADR-0015

Closeout pass. The seven findings of the previous round are re-checked against
`docs/design/parallel-views.md` as it now stands (1400 lines); each is resolved in the text, and
no design defect remains.

| # | Finding | Status | Where it is resolved |
|---|---|---|---|
| **F1** | `pending_ready` never released by `retire`, so the pre-measurement gate could stick shut | **RESOLVED** | §4.6:849-854 states that `retire` decrements `pending_ready` when the worker it removes was not `READY`, and names the three removal paths (reload sweep, stale-at-birth, `view_gone`'s early return) and the consequence; §8.2:1254-1255 lists it in `retire`'s rewrite. R4's duplicate leg (§7:1154) now asserts the release. No double decrement: `retire` and `view_gone` both act only on the removal that succeeds, so exactly one of them owns the decrement. |
| **F2** | `--learn` fallback asked `grow` for zero processes | **RESOLVED** | §4.3:545-550: "`pump_learn_fallback` (`:1816`) passes `1`: its request waits in `learn_fallback_queue`, not in `queue`, so `needed_processes` would be 0 and the single fallback slot would never get a process" — the same `grow(route, 1)` shape §4.3:556 gives `start()`. |
| **F3** | Pre-measurement gate never lifted when the cap was operator-owned | **RESOLVED** | §4.3:488-495 adds `&& self.cfg.max_view_processes.is_none()` to the gate, with the reason inline: "with `BLASTIT_MAX_VIEW_PROCESSES` set there is nothing to measure, so the gate would only serialise forks forever." Correct: `measure_view_memory` returns early when the cap is explicit (`scheduler.rs:3307-3309`), so without this the chain stayed one-fork-per-`READY` for the process's life. |
| **F4** | §4.5's `spawn` named a private constant in another module | **RESOLVED** | §4.5:776-786 defines a local `const VIEW_PROCESS_MARGIN: u32 = 16;` in `supervise.rs`, states that `php_view`'s private twin is the same number, and records unifying the two as a follow-up (no cross-module reference, so no `E0603`). The ceiling and the rlimit still agree numerically; drift is a recorded request, not an unimplemented ambiguity. |
| **F5** | `emit_close_spawn` vs `emit_warning` | **RESOLVED** | One name: `emit_warning`, called at §4.7:939 and defined in §8.2:1247; no other spelling remains in the document. |
| **F6** | MB.24.4's wiring snippet could not be placed where §8 said | **RESOLVED** | §8:1303-1321 places the loop **after** `boot_bindings` returns (`:356`) and **before** `probe_bindings` (`:370`), uses the real local (`php.get(index)`), and carries `#[cfg(feature = "php")]`. That matches `stage1.rs:356-376`, and the datum is the same slot `spawn_now` forks on. |
| **F7** | Duplicate-pid removal skipped `fail_request`/`release` | **RESOLVED** | §4.3:577-587: the branch drops the newcomer's fd **first** (before any reader task exists — the plumbing now follows the check at `:588-596`), then calls `retire(key, Other("duplicate_pid"))` on the *held* key. `retire` fails that key's requests `502`, releases them, drops its queue, decrements `pending_ready` if it was not `READY`, sends the one `Ctl::Retire` that kills the newcomer (`may_retire` passes — init did fork it for this route) and emits the one `process.retired`. No stale reader is bound to the key, and `unexpected_spawn` still goes through `retire_unheld`. |

**Non-blocking housekeeping** (no grade): `retire` still does not call `forget_if_idle`, so an
empty `ViewSet` can outlive the last worker after a reload sweep, while §4.1:227-229 says the
entry exists only while a worker or a `SPAWN` is present. Every reader is length-based
(`len`/`is_empty`/`live_processes`), so there is no behavioural consequence; the callee already
exists and can be added while touching `retire`.

## Verified as preserved on the final text

- The one-fork-before-the-first-sample chain is intact: `pending_ready` (§4.1:231-240), the
  gate (§4.3:488-495), `len()` counting `views + slot` only (§4.1:241-248), and the cap
  re-derived at `READY` before that pump's next `grow`.
- A refused or failed fork is never fatal: warn + `Ctl::Exited { pid: 0 }` → `spawn_failed`
  (§4.7:919-942), init's own `count_for` ceiling (§4.5:788-806), and pid 0 unrepresentable as a
  `Pid` (`crates/blastit-sys/src/pid.rs:13-19`), so it cannot collide with a child.
- `ViewKey { route, pid }` is still the only dispatch key, `view_out` keeps its key check, the
  wire gains only the reserved empty `Ctl::Reload` `0x45`, and the stage-0 `execve` barrier,
  per-route uid/grants, sockets, seccomp and the `php2shell`/`node2shell`/smuggling posture are
  untouched — a pool multiplies processes, not parsers (§5:1093-1117, `php2shell` row at :1109).
- No new capability, no new panic path, and no request/header/body value in the new logs: the
  new values are pids, uids, fixed reason strings and one fixed sentence naming the route.

## Verdict

Each finding from the last round is now answered by a sentence or a code block the implementer
can follow without guessing, and the two remaining residues (a duplicated `16`, the empty-pool
tidy-up) are either harmless or recorded as requests. Nothing in the change weakens an invariant:
the memory cap is still charged one measured fork at a time, a refused fork still cannot take
another route's availability, the pid-0 sentinel is still unforgeable, and `ViewKey` uniqueness
holds on every insert and removal path.

VERDICT: PASS
