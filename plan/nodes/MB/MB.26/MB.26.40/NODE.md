---
id: MB.26.40
title: "Deterministic ctl-write-to-gone-scheduler test (flake at supervise.rs:1045)"
kind: task
status: proposed
owner: implementer
priority: 2
depends_on: []
paths: [crates/blastit/src/supervise.rs, "crates/blastit/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

Make `supervise.rs`'s `a_late_ctl_write_to_a_gone_scheduler_is_best_effort` deterministic. It fails intermittently under full-workspace parallelism and has already cost a gate run.

# Context

Reported at the `MB.3.4` gate: `crates/blastit/src/supervise.rs:1045` fails now and then under `cargo test --workspace` parallelism, passes 3/3 when run alone. The mechanism is a race in the assertion, not the behaviour: the first `EPIPE` on a closed AF_UNIX peer can still succeed (the write is buffered before the peer's close is observed), so a test that asserts "the first write fails" is asserting a kernel timing detail rather than the contract.

The contract worth pinning is: a write to a scheduler that is **known to be gone** is best-effort and never fatal — i.e. the caller does not panic, does not abort, and reports the condition the way the code intends. Retrying until the peer notices is legitimate *in the test*, as long as the bound is explicit.

# Acceptance

1. Reproduce with evidence: run the suite under parallelism (`cargo test -p blastit --locked`) enough times to show the flake (or state honestly how many runs it took, and what the failure output is), then run it alone to show it passes.
2. Fix the **test**, not the behaviour: wait/retry with a bounded deadline until the closed peer is observed (or assert the documented outcome that does not depend on the first-write timing), and add a comment naming the race so the next reader does not "simplify" it back.
3. Prove: the test passes 20 consecutive runs under full-workspace parallelism (`cargo test --workspace --locked`, or the same command that flaked) and still fails if the best-effort handling is removed (a teeth check — mutate the handler temporarily in a scratch copy, or show the assertion that would catch a panic).
4. `make check` exit 0 twice in a row; no behaviour change in `supervise.rs` unless the diagnosis shows the code is genuinely wrong (in which case say why and treat it as a fix, not a test change).
5. If the cleanest answer is a design change (e.g. the ctl writer should not attempt a write after observing a closed peer), say so and propose it as its own node rather than folding it in.

# Notes

- A flaky test in a gate is worse than a missing test: it teaches the loop to re-run instead of read. That is why this is a node, not a "retry and move on".
- Do not mark it `#[ignore]`, do not loosen it to `is_ok()`, and do not add a sleep long enough to hide the race.

# Provenance

Re-filed from `MB.16` at the `v0.2.0` gate: deterministic retry for `supervise.rs::a_late_ctl_write_to_a_gone_scheduler_is_best_effort` under heavy workspace parallelism.
