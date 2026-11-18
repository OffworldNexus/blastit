---
id: MB.26.25
title: "Protect the view.chan transport: a critical suite for the decoder, the credit window and the frame writer"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-view/tests/**", "crates/blastit/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T18:23"
updated: "2026-11-18T18:23"
---

# Goal

Put the `view.chan` transport under the protected tier: a
`crates/blastit-view/tests/critical_transport.rs` that asserts, as security
invariants, the one request-target decoder (`percent_decode`/`hex`), the credit
window's `take`/`close` semantics and the frame writer's `max_payload` bound —
each with its negative twin (a bad `%` escape refused, a closed window returning
0, a payload over the bound refused).

# Context

`MB.36.7`'s audit (`docs/testing/AUDIT-MB.36.md` §4) ran
`make mutants-rust PKG=blastit-view`: 191 caught / 26 missed (86.0 %) — and the
misses are concentrated in `path.rs` (`percent_decode`'s `<<4 | low`, the
`b'a'..=b'f'` arm, `index += 3` timeouts) and `wire.rs::max_payload -> 1`.
Those are the decoder and the framing bound R7/R10 call security boundaries, yet
their tests live in `blastit-view-python`/`blastit-view-static` suites, so a
per-crate campaign does not run them and a change to `blastit-view` alone can
ship with only downstream coverage.

# Acceptance

```console
$ cargo test -p blastit-view --test critical_transport --locked
$ make mutants-rust PKG=blastit-view
```
- every case in the new file asserts the accepted path **and** its refusal
  (design §7 R7/R10);
- the `path.rs`/`wire.rs`/`flow.rs` survivors listed in the audit are killed, and
  the campaign's `missed.txt` no longer names them;
- `make critical-guard NODE=<this node>` passes.

# Notes

A new `critical_*.rs` file is always allowed by `tools/critical_guard.py`. Do not
edit any existing suite; add the file and, if a helper is needed, a
`tests/common/` module beside it.

# Provenance

Re-filed from `MB.36.8` (the `MB.36` audit) so the deferred gap stays actionable:
priority-3 coverage, not an `MB.36` acceptance claim. The body above is `MB.36.8`'s own, unchanged.
