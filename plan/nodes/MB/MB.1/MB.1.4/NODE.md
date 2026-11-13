---
id: MB.1.4
title: "Measure blastit-sqlpolicy normalize, literals and lru mutants"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: []
paths:
  - "crates/blastit-sqlpolicy/src/normalize.rs"
  - "crates/blastit-sqlpolicy/src/literals.rs"
  - "crates/blastit-sqlpolicy/src/lru.rs"
  - "crates/blastit-sqlpolicy/tests/policy_module_edges.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T14:44"
updated: "2026-11-13T14:44"
---

# Goal

Measure the three unreached `blastit-sqlpolicy` policy modules `normalize.rs` (114 mutants), `literals.rs` (4) and `lru.rs` (10) in one file-scoped run, and add the tests each module needs to clear 75 % killed.

# Context

- Baseline (`plan/nodes/MA/RESULT.md` §2): these 128 mutants were **never run** — the 28 min bounded `make mutants-rust PKG=blastit-sqlpolicy` stopped inside `lexer.rs`. Epic acceptance (`plan/nodes/MB/MB.1/NODE.md`) requires **all 1019** mutants measured at >= 75 % per policy module.
- `literals.rs` redacts every PostgreSQL literal form and already has a protected oracle (`tests/critical_literals.rs`, 25 tests) — do not edit it; a survivor there means a new non-critical case in `tests/policy_module_edges.rs`.
- `normalize.rs` (1573 lines) canonicalises statements so a grant cannot be bypassed by reformatting, comments or casing; `lru.rs` (136) is a cache. A surviving mutant in `normalize.rs` is a potential allowlist bypass — the allowed **and** denied twin must be asserted, never only the happy path.
- `crates/blastit-sqlpolicy/tests/critical_*.rs` are protected and not in `paths`.

# Acceptance

```console
$ df -h /tmp | tail -1

# 1. Crate tests green (new non-critical tests included).
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked

# 2. One file-scoped run over the three modules (~7-9 min, no timeouts expected).
$ mkdir -p /tmp/opencode/blastit-mutants
$ TMPDIR=/tmp/opencode/blastit-mutants CARGO_NET_OFFLINE=true \
    cargo mutants -p blastit-sqlpolicy -f '**/src/{normalize,literals,lru}.rs' -j 2 --timeout 90 \
    -o /tmp/opencode/mb1-sql-normalize
$ grep -c ':' /tmp/opencode/mb1-sql-normalize/mutants.out/outcomes.txt   # 128 mutants listed
$ test ! -s /tmp/opencode/mb1-sql-normalize/mutants.out/timeout.txt && echo "0 timeouts"

# 3. Per-module killed ratio, each >= 75.0 %.
$ for f in normalize literals lru; do
    awk -v f="$f" '$1 ~ ("/" f "\\.rs:") {n[$NF]++} END {v=n["caught"]+n["missed"]+n["timeout"]; printf "%s: %d/%d = %.1f%%\n", f, n["caught"], v, 100*n["caught"]/v}' \
      /tmp/opencode/mb1-sql-normalize/mutants.out/outcomes.txt
  done

# 4. Any survivor is either tested or documented as unreachable with a reason.
$ awk '$NF=="missed"' /tmp/opencode/mb1-sql-normalize/mutants.out/outcomes.txt

# 5. Green gate.
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
```

# Notes

- Verified glob semantics (cargo-mutants 27.1): `-f '**/src/{normalize,literals,lru}.rs'` lists exactly 128 mutants; a glob containing `/` matches the whole path, a bare name like `literals.rs` matches the file name.
- Never add `#[mutants::skip]` silently: a skip needs a one-line reason in the code and in `RESULT.md`.
- Do not touch `src/lexer.rs`, `src/learn.rs`, `src/statements.rs`, `src/tables.rs`, `src/analysis*` (`MB.1.3`/`MB.1.5` own them) or `src/lib.rs` (`analysis.rs`/`lib.rs` are already measured).
- `normalize.rs` is the security-relevant one: prefer table-driven cases that pair a reformatted allowed variant with a bypass attempt (comment splice, casing, duplicate whitespace) that must still normalise identically.
