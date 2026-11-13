---
id: MB.1.5
title: "Measure blastit-sqlpolicy statements and tables mutants"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: []
paths:
  - "crates/blastit-sqlpolicy/src/statements.rs"
  - "crates/blastit-sqlpolicy/src/tables.rs"
  - "crates/blastit-sqlpolicy/tests/policy_module_tables.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T14:44"
updated: "2026-11-13T14:44"
---

# Goal

Measure the last two unreached `blastit-sqlpolicy` modules — `statements.rs` (79 mutants) and `tables.rs` (91) — at >= 75 % killed each, closing the "all 1019 mutants measured" requirement of the epic.

# Context

- Baseline (`plan/nodes/MA/RESULT.md` §2): `statements.rs` (79) and `tables.rs` (91) were never reached inside the 28 min bounded run. With `MB.1.3` (`lexer.rs` 298 + `learn.rs` 103) and `MB.1.4` (`normalize.rs` 114 + `literals.rs` 4 + `lru.rs` 10) plus the already-measured `analysis.rs` 299 and `lib.rs` 21, this leaf completes 1019/1019.
- `statements.rs` classifies statement kind/transaction control; `tables.rs` extracts the tables/columns a statement touches, which is the table-grant surface (`[route.db] tables`, `docs/reference/manifest.md`). A surviving mutant here can under- or over-report a table and widen a grant: every new test must pair the allowed grant with the denied twin (e.g. an `INSERT` reported for `SELECT`-only grants).
- `crates/blastit-sqlpolicy/tests/critical_learn_policy.rs` is protected and not in `paths` — put new cases in the new non-critical `tests/policy_module_tables.rs`.

# Acceptance

```console
$ df -h /tmp | tail -1

# 1. Crate tests green (new table/statement cases included).
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked

# 2. One file-scoped run (170 mutants, ~8-10 min).
$ mkdir -p /tmp/opencode/blastit-mutants
$ TMPDIR=/tmp/opencode/blastit-mutants CARGO_NET_OFFLINE=true \
    cargo mutants -p blastit-sqlpolicy -f '**/src/{statements,tables}.rs' -j 2 --timeout 90 \
    -o /tmp/opencode/mb1-sql-statements
$ grep -c ':' /tmp/opencode/mb1-sql-statements/mutants.out/outcomes.txt   # 170 mutants listed
$ test ! -s /tmp/opencode/mb1-sql-statements/mutants.out/timeout.txt && echo "0 timeouts"

# 3. Per-module killed ratio, each >= 75.0 %.
$ for f in statements tables; do
    awk -v f="$f" '$1 ~ ("/" f "\\.rs:") {n[$NF]++} END {v=n["caught"]+n["missed"]+n["timeout"]; printf "%s: %d/%d = %.1f%%\n", f, n["caught"], v, 100*n["caught"]/v}' \
      /tmp/opencode/mb1-sql-statements/mutants.out/outcomes.txt
  done

# 4. Survivors listed, each either tested or documented as unreachable.
$ awk '$NF=="missed"' /tmp/opencode/mb1-sql-statements/mutants.out/outcomes.txt

# 5. Green gate.
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
```

# Notes

- Verified glob: `-f '**/src/{statements,tables}.rs'` lists exactly 170 mutants in cargo-mutants 27.1.
- Table extraction is a grant surface: assert both the table a statement legitimately touches **and** that a statement touching another table is not matched by the same grant (a mutant that drops a table from the report must fail the test).
- Keep `crates/blastit-sqlpolicy/tests/critical_*.rs` untouched; do not edit `src/lexer.rs`, `src/learn.rs`, `src/normalize.rs`, `src/literals.rs`, `src/lru.rs` (owned by `MB.1.3`/`MB.1.4`).
- In `RESULT.md`, add a one-line tally proving 1019/1019 mutants are now measured across `MB.1.3`–`MB.1.5`.
