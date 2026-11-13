---
id: MB.1.2
title: "Prove blastit-manifest whole-crate mutation kill rate past 75 percent"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.1.1]
paths:
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/tests/mutation_gaps.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T14:44"
updated: "2026-11-13T14:44"
---

# Goal

Lift whole-crate `blastit-manifest` mutation coverage past 75 % killed with zero 90 s timeouts, proving `tools/`-measured `make mutants-rust PKG=blastit-manifest` on the post-`MB.1.1` tree.

# Context

- Baseline: `437/630` viable = 69.4 % whole crate; policy modules 95.0 % (`parse.rs` 98.3 %, `model.rs` 92.4 %, `error.rs` 77.8 %); `edit.rs` 42.9 % with 10 timeouts; `parse.rs:252` 1 timeout.
- `MB.1.1` owns `src/edit.rs` + `src/parse.rs` (loop bounding, `verify()`, patcher branches). This leaf owns the **remaining** survivors: `src/model.rs` (9 missed) and `src/error.rs` (4 missed) plus whatever the whole-crate run still misses, and produces the crate-wide number the epic acceptance asks for.
- This is DEBT-only work: no manifest grammar, no new keys, no behaviour change. Tests may only be added; never weaken or delete one, and never touch `crates/blastit-manifest/tests/critical_*.rs` (protected, not in this node's `paths`).
- Sequential after `MB.1.1`: two whole/file mutation runs must not overlap in the same crate.

# Acceptance

```console
$ df -h /tmp | tail -1          # a full /tmp turns the link into SIGBUS (LESSONS.md)

# 1. The exact command the epic acceptance names (whole crate, ~11-15 min once timeouts are gone).
$ make mutants-rust PKG=blastit-manifest

# 2. Zero cursor-loop timeouts survive.
$ test ! -s mutants.out/timeout.txt && echo "0 timeouts"

# 3. Whole-crate killed ratio, the gate.
# printed ratio >= 75.0 %

# 4. No surviving mutant in the policy/validation modules this crate is judged on.
$ grep -E 'src/(validate|routing|uid)\.rs' mutants.out/missed.txt || echo "policy modules clean"

# 5. Green gate.
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
```

# Notes

- Escape hatch if a survivor is genuinely unreachable/dead code: mark it with a comment-level justification and `#[cfg_attr(test, ...)]`-free, minimal restructuring — but **never** `#[mutants::skip]` without a written reason in the code and in `RESULT.md` (a skip is not a kill).
- New tests go in `crates/blastit-manifest/tests/mutation_gaps.rs` (integration) or inline `#[cfg(test)]` modules inside `src/model.rs` / `src/error.rs` (both listed in `paths`).
- Record the exact per-module ratio table (baseline vs now) in `RESULT.md`; the milestone gate re-reads it.
- If wall time overruns 20 min, stop and report the completed ratio rather than leaving the run incomplete: partial numbers plus a follow-up beats a hung leaf.
