---
id: MB.1.12
title: "Promote the normalize and table policy-surface oracles to the protected tier"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths:
  - "crates/blastit-sqlpolicy/tests/critical_policy_surface.rs"
  - "crates/blastit-sqlpolicy/tests/policy_module_edges.rs"
  - "crates/blastit-sqlpolicy/tests/policy_module_tables.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T18:42"
updated: "2026-11-13T18:45"
---

# Goal

Put the two policy-surface oracles that `MB.1.4`/`MB.1.5` added into the protected tier, so a widening mutant that makes a granted `IN ($?)` accept `IN ('x')` cannot be silenced by deleting a non-critical file.

# Context

- `docs/testing/AUDIT-MB.1.md` **F1 (MAJOR)** and **F2 (MAJOR)**: `crates/blastit-sqlpolicy/tests/policy_module_edges.rs` (normalize shaping bypass pairs) and `policy_module_tables.rs` (join alias / subquery / `INSERT … SELECT` / star / lock extents) pin security behaviour in the unprotected standard tier; `crates/blastit-sqlpolicy/tests/critical_learn_policy.rs` has **0** join/subquery lines.
- The audit **reproduced** the escape: mutating `is_placeholder_list -> true` (a grant for `IN ($?)` then allows `IN ('x')`) leaves `critical_literals` 25/25 and `critical_learn_policy` 10/10 green and fails only `policy_module_edges`.
- `tools/critical_guard.py` protects `crates/*/tests/critical_*.rs`; adding a new protected file is always allowed, editing an existing one needs it listed in `paths`.
- This is a test-tier promotion: no production behaviour may change.

# Acceptance

```console
# 1. A protected file owns the oracles, and the escape is caught by it.
$ ls crates/blastit-sqlpolicy/tests/critical_policy_surface.rs
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked   # all suites green

# 2. The reproduced escape now fails a PROTECTED suite (probe on a /tmp copy, never in the repo).
#    Apply `is_placeholder_list -> true` (or the equivalent widening hack) in the copy and show:
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --test critical_policy_surface 2>&1 | tail -5   # FAILED, naming the widening
#    then revert the copy.

# 3. The standard-tier files keep their cases (no test deleted), or the cases moved with a pointer; the
#    protected file covers, at minimum: `IN ($?)` granted must DENY `IN ('x')`; join alias extents;
#    subquery extents; INSERT…SELECT; `SELECT *`; FOR UPDATE/lock extents.
$ grep -c '#\[test\]' crates/blastit-sqlpolicy/tests/critical_policy_surface.rs   # >= 12

# 4. Guard + gate.
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node <ID>
$ make rs-lint
$ uv run --no-project tools/plan.py validate
```

# Notes

- Never weaken or delete an existing assertion; move/copy cases into the protected file and leave a one-line comment where they came from.
- `RESULT.md` must state which behaviours the protected file now owns and paste the failing probe output.
- `make check` is run by the lead at the epic gate (it builds a wheel); `make rs-lint`/`make rs-doc` here.
