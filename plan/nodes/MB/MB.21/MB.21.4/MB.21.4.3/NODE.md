---
id: MB.21.4.3
title: "Map check, dev --learn, fold and --json fix actions onto Blastfile nodes"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.2]
paths:
  - crates/blastit/src/check.rs
  - crates/blastit/src/dev_learn.rs
  - crates/blastit/src/audit_v1.rs
  - "crates/blastit-manifest/src/blastfile/**"
  - crates/blastit/tests/critical_check_engine.rs
  - crates/blastit/tests/critical_learn_executor_blocks.rs
  - crates/blastit/tests/audit_group_hint.rs
  - python/tests/critical/test_check_contract.py
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Make `blastit check`, `blastit dev --learn`, `blastit-manifest fold` and the `--json` contract write
and read `Blastfile`: the fix actions name Blastfile nodes (`view`, `trait`, `query`, `body`) and the
surgical editor from `MB.21.3.5` is what applies them.

# Context

- Design §9–§10 and `ADR-0003`/`ADR-0004` (the `--json` v1 schema and the literal-redaction rules are
  **unchanged**; only the `to`/`fix` strings that name a node change).
- Code sites: `crates/blastit/src/{check,dev_learn,audit_v1}.rs`, the `learn_line` helper
  (`check.rs:1364-1379`), `read_manifest` (`check.rs:294-300`), and the executor glue that calls the
  manifest editor. `dev --learn` writes grants; `check --learn` proposes new views only — keep that
  asymmetry exactly (`MB.1.6`, `docs/VISION.md`).
- Mapping rules to implement and pin:
  * `missing_grant` → `add_query`, `to` names the view and the store (`view "restaurants" → postgres "db"`),
    inserting `query 'SELECT …'` inside that store's grant block (creating the block when absent);
  * connection-setup SQL → `add_trait` (the `MB.1.6` group hint, now a trait record plus a `use` line on
    every view) instead of `add_group`;
  * `unrouted` → `add_view`, inserted above the method-unrestricted catch-all `view`;
  * a body seen on a route without a codec → `add_body` with `json`/`form`;
  * `raw` is never written and a statement inlining a literal is still refused, not written.
- Tests: `crates/blastit/tests/critical_check_engine.rs` and
  `crates/blastit/tests/critical_learn_executor_blocks.rs` (rewrite fixtures to Blastfile; assertions
  stay or get stronger), `python/tests/critical/test_check_contract.py` (the `--json` document shape),
  and `crates/blastit/tests/audit_group_hint.rs` (now a trait hint).

# Acceptance

```console
$ make test-critical
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/critical/test_check_contract.py -q
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.3
$ make check
```

1. `dev --learn` on a fixture adds exactly one `query` line inside the right store block, keeps every
   comment and blank line, and a second run adds nothing.
2. `check --learn` still proposes views only; `--json` bytes match the documented v1 schema with the new
   node names in `fix.to`, and the literal-refusal case still refuses.
3. The connection-setup finding names a trait and a `use` line, not a group (assert the exact strings).
4. `raw` is never emitted by any learn path (assert on the written text of every fixture).
5. `make test-critical` (zero skips) and `make check` exit 0; `critical_guard.py` clean.

# Notes

- `python/tests/critical/test_check_contract.py` is protected: it is in `paths` for the fixture format
  change only; do not relax a schema assertion.
- Keep the human-readable finding text stable where a docs page quotes it (`docs/ci.md` is rewritten by
  `MB.21.5.1`, so say in `RESULT.md` which sentences moved).
