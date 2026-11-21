---
id: MB.30
title: One VIEW_PROCESS_MARGIN constant and a truthful view_gone paragraph in the design
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths: [crates/blastit/src/supervise.rs, crates/blastit/src/php_view.rs, "docs/design/parallel-views.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T08:41"
updated: "2026-11-18T20:36"
---

# Goal

Two follow-ups from `MB.24.3`'s review, both small and both about one fact living twice.

1. `VIEW_PROCESS_MARGIN` is `16` in `crates/blastit/src/supervise.rs` (init's own fork ceiling,
   `view_worker_cap() + 16`) and again as a private twin in `crates/blastit/src/php_view.rs`
   (`RLIMIT_NPROC`). They are the same number for the same reason (§7.2: gVisor leaves out the main
   thread), so they belong in one place with one sentence.
2. `docs/design/parallel-views.md` §4.6/§4.7 must say what `view_gone` actually does: it fails and
   releases **whatever is still keyed** to the departed `ViewKey`, whether or not the pool still
   holds the worker (the entries are idempotent through `complete`), because returning early on a
   worker the pool no longer holds would leak the scheduler slot. Today's pseudo-code implies the
   early return is harmless.

# Context

- `crates/blastit/src/supervise.rs` (`VIEW_PROCESS_MARGIN`, `may_fork`, the `spawn` ceiling),
  `crates/blastit/src/php_view.rs` (its private twin and the `RLIMIT_NPROC` it installs),
  `crates/blastit-scheduler/src/scheduler.rs::view_gone` (its doc comment already says why),
  `docs/design/parallel-views.md` §4.6/§4.7.
- `MB.24.3`'s review finding 5 and follow-up 8; `plan/nodes/MB/MB.24/MB.24.3/RESULT.md`.

# Acceptance

```console
$ uv run --no-project tools/plan.py validate
$ cargo test -p blastit --features python --lib supervise
$ cargo test -p blastit --features python --test dev_signal_reap
$ make docs-check
$ make check
```

- one `const VIEW_PROCESS_MARGIN: u32 = 16` with one comment, referenced by both call sites; the
  `RLIMIT_NPROC` value it yields is unchanged (assert it in a unit test);
- the design's §4.6/§4.7 text matches `view_gone` (fail and release whatever is still keyed), and a
  reader cannot "restore" an early return that leaks a slot;
- no behaviour change; no new knob.

# Notes

- If sharing the constant would need a new public item across a `php`-gated boundary, prefer a
  `pub(crate)` const over a trait or a second crate.
