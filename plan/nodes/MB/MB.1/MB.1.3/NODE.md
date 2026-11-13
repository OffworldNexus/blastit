---
id: MB.1.3
title: "Bound blastit-sqlpolicy lexer and learn loops and measure lexer.rs"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: []
paths:
  - "crates/blastit-sqlpolicy/src/lexer.rs"
  - "crates/blastit-sqlpolicy/src/learn.rs"
  - "crates/blastit-sqlpolicy/tests/lexer_progress.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T14:44"
updated: "2026-11-13T14:44"
---

# Goal

Bound the two remaining `blastit-sqlpolicy` cursor loops (`lexer.rs:179..230` byte scanner, `learn.rs:661` `parse_form`) so their mutants terminate instead of burning 90 s each, and measure the whole `lexer.rs` module (298 mutants) at >= 75 % killed.

# Context

- Baseline (`plan/nodes/MA/RESULT.md` §2): `lexer.rs` only 85/298 mutants reached (`80/82` viable killed, 97.6 %) with **8 timeouts** at `lexer.rs:179..230`; `learn.rs` 103/103 reached (`90/98`, 91.8 %) with **1 timeout** at `learn.rs:661`.
- The timeouts are cursor-advance mutations (`+=` -> `*=`) that never advance: `lexer.rs:661:19` (`parse_form` in `learn.rs`) is `replace += with *=`. Restructure the scans so progress is structural (iterate a byte slice with an explicit index taken from `enumerate`/`get`, or guard `assert!(next > pos)`), never fail them by raising `--timeout`.
- These loops guard SQL text parsing: an unbounded scan is a DoS on hostile SQL and the lexer is a security surface (`security-invariants`). Keep the existing length/budget checks and never narrow accepted syntax.
- `crates/blastit-sqlpolicy/tests/critical_*.rs` are protected and not in `paths`: new coverage goes in the new non-critical `tests/lexer_progress.rs`.

# Acceptance

```console
$ df -h /tmp | tail -1

# 1. Crate tests green, including the new progress-budget tests.
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked
$ grep -c 'fn ' crates/blastit-sqlpolicy/tests/lexer_progress.rs        # >= 4

# 2. Whole lexer.rs module measured, no runaway mutant (one file-scoped run).
$ mkdir -p /tmp/opencode/blastit-mutants
$ TMPDIR=/tmp/opencode/blastit-mutants CARGO_NET_OFFLINE=true \
    cargo mutants -p blastit-sqlpolicy -f '**/src/lexer.rs' -j 2 --timeout 90 -o /tmp/opencode/mb1-sql-lexer
$ test ! -s /tmp/opencode/mb1-sql-lexer/mutants.out/timeout.txt && echo "0 timeouts"
$ for f in caught missed timeout unviable; do printf '%-9s %s\n' "$f" "$(wc -l < /tmp/opencode/mb1-sql-lexer/mutants.out/$f.txt)"; done
# caught/(caught+missed+timeout) is the killed ratio; the four counts must add to 298 and be pasted in RESULT.md

# 3. The learn.rs:661 spinner is gone (11 mutants around the loop, seconds not minutes).
$ CARGO_NET_OFFLINE=true cargo mutants -p blastit-sqlpolicy -F 'learn\.rs:6[56][0-9]' --list | wc -l   # 11
$ TMPDIR=/tmp/opencode/blastit-mutants CARGO_NET_OFFLINE=true \
    cargo mutants -p blastit-sqlpolicy -F 'learn\.rs:6[56][0-9]' -j 2 --timeout 90 -o /tmp/opencode/mb1-sql-learn
$ test ! -s /tmp/opencode/mb1-sql-learn/mutants.out/timeout.txt && echo "0 timeouts"
$ grep -c . /tmp/opencode/mb1-sql-learn/mutants.out/missed.txt >/dev/null && cat /tmp/opencode/mb1-sql-learn/mutants.out/missed.txt || echo "all caught"

# 4. Green gate.
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
```

# Notes

- `-f` is a single glob in cargo-mutants 27.1: a glob containing `/` matches the whole path, hence `**/src/lexer.rs`. Verified: 298 mutants for this glob, 11 for the `-F` regex.
- Two `-o` output dirs keep the two runs' `mutants.out` from overwriting each other; inspect each separately. Both runs are file/regex scoped — no whole-crate run here.
- If a `lexer.rs` survivor is a genuine alternative spelling of a token rule, add the allowed **and** denied case to `tests/lexer_progress.rs`; a killed mutant must not be killed by a test that only asserts `Ok`.
- Report wall time per run in `RESULT.md` so the milestone gate can see the 90 s timeout slots are gone.
