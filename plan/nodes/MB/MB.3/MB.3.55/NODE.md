---
id: MB.3.55
title: "MB.3.54 review MINORs: the pretty twin cannot fail on stream, the proof token's honest limit, and two names"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.3.54]
paths:
  - crates/blastit/tests/critical_raw_ingress.rs
  - crates/blastit/src/stage0.rs
  - docs/deploy/index.md
  - docs/knowledge/LESSONS.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T08:46"
updated: "2026-11-18T20:36"
---

# Goal

Close the four MINOR findings from MB.3.54's reviewer. None is a hole in the gate; three are
precision (one assertion that cannot fail in its twin, one overstated doc, one naming issue,
one proof-token weakness) and they exist so the next reader is not misled by a suite that
looks stricter than it is.

# Context

MB.3.54 (commit cfe0185) made `ServeGates` a compile-time proof that `raw_ingress_gate` ran
before `bind_view_sockets`, and repaired the two unfalsifiable assertions in
`critical_raw_ingress.rs`. Its reviewer PASSed and confirmed the proof cannot be bypassed as
shipped (one constructor, private field, no `Default`/`Clone`, no cross-module literal, no
unsafe forging, one production bind site), then left:

1. `crates/blastit/tests/critical_raw_ingress.rs:391-406` — the pretty twin never asserts its
   `stream` column: flipping `Stream::Stderr` to `Stdout` left it green, while `:310-311`
   claims "the pretty-format twin below asserts the *other* stream". Asserts the whitespace
   token `stderr` on the line naming `allow_raw`, or drop the claim.
2. `crates/blastit/src/stage0.rs:373` — "Uninhabited outside this module" overstates: the
   type is a ZST, so `unsafe { mem::zeroed() }` would forge one (nothing does, and no unsafe
   of that kind exists in the crate). Say "not constructible outside this module: private
   field, single constructor".
3. `stage0.rs:377-382` — the token proves *a* gate passed, not that the manifest bound is the
   one gated. One binding exists today so no path exploits it. Optional strengthening: hold
   `manifest: &'a Manifest` in `ServeGates<'a>` so `bind_view_sockets` reads the gated
   manifest from the token; if that fights the borrow checker or the call shape, record why
   and leave the limit documented.
4. `stage0.rs:379`/`:2699` — `ServeGates::clear(&manifest)` reads like a mutator and
   `_serve_gates` reads like an unused binding. `ServeGates::pass(&m)` / `gate_proof` name
   what they are.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_raw_ingress
$ make check
```
- Item 1's twin reddens when the stream flips (paste both runs).
- The doc sentence at `:373` states what is true, and — if item 3 is taken — a second
  manifest cannot be bound under a token minted for the first (say how you proved it, or
  record the limit and why it is acceptable today).
- Renames do not change behaviour; `make check` exit 0; no assertion weakened; no
  `#[ignore]`.

# Notes

- `crates/blastit/tests/critical_raw_ingress.rs` is protected and this node's paths name it:
  strengthen only.
- Do not re-open the proof-type design; the reviewer confirmed it cannot be bypassed.
