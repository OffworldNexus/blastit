---
id: MB.1.14
title: "Close the sqlpolicy lexer numeric-extent survivors, re-measure learn.rs, and protect the lexer boundary oracle"
kind: task
status: done
owner: implementer
priority: 3
depends_on: []
paths:
  - "crates/blastit-sqlpolicy/src/lexer.rs"
  - "crates/blastit-sqlpolicy/src/learn.rs"
  - "crates/blastit-sqlpolicy/tests/lexer_progress.rs"
  - "crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T18:44"
updated: "2026-11-13T19:42"
---

# Goal

Close the `blastit-sqlpolicy` coverage debt the `MB.1` gate audit found: protect the lexer boundary oracle, kill the 30 real numeric-literal mutants in `lexer.rs`, and re-measure `learn.rs` as a whole module on the shipped tree.

# Context

- `docs/testing/AUDIT-MB1.md` (`G3`). `crates/blastit-sqlpolicy/tests/lexer_progress.rs` (6 tests) pins lexer region boundaries and 200 KB hostile input — a parser-differential/DoS surface (`security-invariants` §2/§5) — but sits in the unprotected standard tier.
- `lexer.rs` is at **173/203 = 85.2 %** with **30 real survivors** (`/tmp/opencode/mb1-sql-lexer3`), not the 8 the plan expected. Distribution, verified from `missed.txt`: **21 in `number_end`**, 4 in `radix_end`, 4 in `grouped_digits`, 1 in `lex`. They are numeric-literal extent mutants (`0x1_2`, `0b1_0`, `$1E'a'`, `1..2`, `_` separators), not timeouts — `timeout.txt` is empty.
- `MB.1.3`'s RESULT says the largest cluster is `grouped_digits`; that is wrong (4 of 30) and its "8 tests" is the `fn` count (there are 6 `#[test]`s). Correct both in your RESULT.
- `learn.rs` lists **96** mutants; only the 4 in the refactored `parse_form` region were re-measured (`/tmp/opencode/mb1-sql-learn`, 4/4). The other 92 rest on the `MA` baseline (`70a4357`, pre-refactor 90/98), so "every mutant measured" is not literally true yet.

# Acceptance

```console
# 1. The boundary oracle is protected (a new critical file is always allowed; deleting the
#    non-critical original is fine, but keep every case).
$ ls crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs
$ grep -c '#\[test\]' crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs   # >= 6
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked                     # exit 0

# 2. Numeric-literal extent is pinned with allowed AND denied twins (exact text, not just "parses"):
#    hex/oct/bin with `_`, `$1E'a'` (placeholder then operator then literal), `1..2` vs `1.2`,
#    `1.2e+3`, `_`-separated runs, and the `0x_`/`0b_` fail form. Show the new tests failing on a
#    /tmp copy with one `number_end` mutant applied and green on the shipped tree.

# 3. The module is re-measured (one file-scoped run, disk scratch):
$ TMPDIR=$(make -s print-mutants-tmp)/mb114 CARGO_NET_OFFLINE=true \
    cargo mutants -p blastit-sqlpolicy -f '**/src/lexer.rs' -j 2 --timeout 90 \
    -o /tmp/opencode/mb114-lexer
$ test ! -s /tmp/opencode/mb114-lexer/mutants.out/timeout.txt && echo "0 timeouts"
$ for f in caught missed timeout unviable; do printf '%-9s %s\n' "$f" \
    "$(wc -l < /tmp/opencode/mb114-lexer/mutants.out/$f.txt)"; done
# killed >= 95.0 % (baseline 85.2 %), 0 timeouts. Any survivor left must be named and proved
# equivalent in RESULT.md, not merely "above the bar".

# 4. `learn.rs` as a whole module, on this tree:
$ TMPDIR=$(make -s print-mutants-tmp)/mb114-learn CARGO_NET_OFFLINE=true \
    cargo mutants -p blastit-sqlpolicy -f '**/src/learn.rs' -j 2 --timeout 90 \
    -o /tmp/opencode/mb114-learn
$ test ! -s /tmp/opencode/mb114-learn/mutants.out/timeout.txt && echo "0 timeouts"
$ for f in caught missed timeout unviable; do printf '%-9s %s\n' "$f" \
    "$(wc -l < /tmp/opencode/mb114-learn/mutants.out/$f.txt)"; done
# counts add to 96 and killed >= 75.0 %, 0 timeouts.

# 5. Gates.
$ make rs-lint && make rs-doc
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node <ID>
$ uv run --no-project tools/plan.py validate
```

# Notes

- Never weaken or delete a case while promoting it; no `#[ignore]`, no raised `--timeout`, no `#[mutants::skip]`.
- A scan loop may be restructured only if a mutation of its step could not then stop it advancing; measure after any production change and state the before/after mutant total.
- Do not touch `policy_module_edges.rs`/`policy_module_tables.rs` (MB.1.12 owns their promotion). If MB.1.12 lands a `critical_policy_surface.rs`, leave it alone.
- `make check` builds a wheel; the lead runs it at the epic gate.
- `RESULT.md` must paste the two `for f in …` blocks, the two `timeout.txt` checks, and the corrected survivor distribution (21/4/4/1).

- If `make -s print-mutants-tmp` is not available yet (`MB.1.11` adds it), use a private scratch
  directory **on disk** (never `/tmp`, which is tmpfs on this host) and say which one in `RESULT.md`.
