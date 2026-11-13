---
id: MB.1.13
title: "Protect the add_group oracle and correct the design-doc credit"
kind: task
status: active
owner: implementer
priority: 2
depends_on: []
paths:
  - crates/blastit/tests/critical_check_engine.rs
  - crates/blastit/tests/audit_group_hint.rs
  - "docs/design/ma-lean-dx.md"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T18:42"
updated: "2026-11-13T18:42"
---

# Goal

Make the protected suite own the `add_group` fix-action oracle that `MB.1.6` introduced, and stop `docs/design/ma-lean-dx.md` crediting a suite that never asserts it.

# Context

- `docs/testing/AUDIT-MB.1.md` **F3 (MAJOR)**: `crates/blastit/tests/audit_group_hint.rs` is the only oracle for `{"action":"add_group","to":"[group.connection.<proxy>].queries"}`, but it is in the unprotected standard tier, and `docs/design/ma-lean-dx.md:392` credits `crates/blastit/tests/critical_check_engine.rs`, which contains **0** `add_group` occurrences.
- `blastit check --json` is the AI-ergonomics contract: an agent applies the `fix` verbatim. If the group hint silently regresses to per-route `add_query`, an agent still edits one route and the connection-setup statement fails on whichever route connects first (`LESSONS.md` last bullet, `MA.8`).
- Test-tier promotion + a truthful design note: no production behaviour change.

# Acceptance

```console
# 1. The protected suite asserts the group hint, in both directions.
$ grep -c 'add_group' crates/blastit/tests/critical_check_engine.rs      # >= 2
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_check_engine --locked    # green

# 2. It would fail if the hint regressed (probe on a /tmp copy, never in the repo):
#    make `audit_v1` always emit `add_query` for connection-setup SQL in the copy, show the protected
#    suite failing and naming the group hint, then discard the copy.

# 3. The standard-tier oracle keeps its cases (no assertion deleted), and the design doc tells the truth.
$ uv run --no-project python -c 'import pathlib;t=pathlib.Path("docs/design/ma-lean-dx.md").read_text();import re;print([l for l in t.splitlines() if "add_group" in l][:4])'
# every line naming the add_group oracle points at the protected file that now owns it

# 4. Guard + gate.
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node <ID>
$ make rs-lint
$ uv run --no-project tools/plan.py validate
```

# Notes

- `crates/blastit/tests/critical_check_engine.rs` is protected: it is in this node's `paths`, so extend it — never relax or delete an existing assertion (it is also the `SELECT 1 → add_query` contract, which must stay).
- `python/tests/critical/test_check_contract.py` asserts the per-route twin and is **not** in `paths`: leave it untouched and green.
- `make check` is run by the lead at the epic gate; run `make rs-lint`/`make rs-doc` here.
