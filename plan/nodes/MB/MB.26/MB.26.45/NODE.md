---
id: MB.26.45
title: "critical_learn_and_redaction.rs:518 BrokenPipe under the parallel workspace run is a race, not a flake"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-manifest/tests/critical_learn_and_redaction.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-19T00:36"
---

# Goal

Make `crates/blastit-manifest/tests/critical_learn_and_redaction.rs:518` deterministic under the
parallel workspace run.

Observed while `MB.32.1` ran `make check`: a `BrokenPipe` panic at that line when the workspace
suites run in parallel; the same file passes 3/3 standalone. That is a real race, not a host
flake: a protected suite that only passes in isolation cannot protect anything, because
`make check` is the gate that runs it in parallel.

1. Reproduce it without a "sometimes" — a bounded loop of the full `cargo test --workspace` (or
   the smallest target set that shows it), pasted, with the run count and the failure text.
2. Name the mechanism from the code: which writer/reader pair loses whose end of the pipe (a
   child that exits before the parent's write, an `obs_fd` reader dropped early, a `Drop` that
   closes a sink another task still writes to). `LESSONS.md` already records the `ECONNRESET`
   twin of this class.
3. Fix it so that the same command is green N times in a row, and keep the assertion that the
   suite exists to make — deleting or weakening the case is a failed leaf (the file is protected;
   this node's `paths` lists it).
4. Add one leg that would go red on the old code (the mutation you can show: restore the old
   ordering and watch it fail).

# Context

- The failure was seen once during `MB.32.1`'s gate run and never standalone; that asymmetry is
  the whole defect. Do not close this node on "cannot reproduce" without the loop in item 1.
- `crates/blastit-manifest/tests/critical_learn_and_redaction.rs` is protected: this node owns
  that path on purpose, and `tools/critical_guard.py --base <sha> --node <id>` must pass.
- `docs/knowledge/LESSONS.md` may be updated with the mechanism (it is not in `paths`; add it to
  the brief of whoever owns this if the finding is general).

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-manifest --locked
$ for i in 1 2 3 4 5; do cargo test -p blastit-manifest --locked || break; done
$ make check
```

and in `RESULT.md`: the reproduced failure text, the mechanism, the five-run tail, and the proof
that the new guard goes red on the old ordering.

# Notes

- If the race is in a *shipped* writer (not the harness), the fix belongs there and this node's
  `paths` grows to that crate in the same change.

# Provenance

Re-filed from `MB.37` at the `v0.2.0` gate: rare parallel-test `BrokenPipe` race in `critical_learn_and_redaction.rs:518`.
