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
  - "crates/blastit-py/src/lib.rs"
  - "crates/blastit-py/src/audit.rs"
  - "crates/blastit-py/src/check.rs"
  - python/blastit/fold.py
  - python/blastit/pytest_plugin.py
  - python/blastit/__init__.py
  - python/tests/test_check.py
  - python/tests/test_fold.py
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T02:18"
---

# Goal

Make `blastit check`, `blastit dev --learn`, `blastit-manifest fold` and the `--json` contract write
and read `Blastfile`: `fix.action` keeps the accepted vocabulary (`add_query`, `add_group`,
`remove_query`, `declare_ingress`, `fix_ingress`, `add_route`, `raw_ingress`) and, where `fix.to`
appears at all, it is a Blastfile **node locator** (`view restaurants postgres db`, `trait connection`,
`view upload body`) — never a directive to paste. The surgical editor from `MB.21.3.5` is what applies
the edits.

# Context

- Design §13.3/§14 and `ADR-0003`/`ADR-0004` (the `--json` v1 schema and the literal-redaction rules are
  **unchanged**; only the `to`/`fix` strings that name a node change).
- Code sites: `crates/blastit/src/{check,dev_learn,audit_v1}.rs`, the `learn_line` helper
  (`check.rs:1364-1379`), `read_manifest` (`check.rs:294-300`), and the executor glue that calls the
  manifest editor. `dev --learn` writes grants; `check --learn` proposes new views only — keep that
  asymmetry exactly (`MB.1.6`, `docs/VISION.md`).
- The Python package is this leaf's (`MB.21.4.4` keeps to fixtures and the Makefile): `fold.py` keeps
  its required `--manifest PATH` (now a Blastfile, `ADR-0002`), re-pointed at the new PyO3 binding
  `blastfile_apply_edits(source, edits_json) -> str` over `apply_edits`, so the one editor stays the
  one editor; `pytest_plugin.py`'s default and help text name `Blastfile`; `python/tests/{test_check,
  test_fold}.py` assert the new defaults.
- Rename the `manifest_toml` parameters to `manifest_text` and fix their docstrings:
  `crates/blastit-py/src/lib.rs:156,179,302,326-333` (a positional PyO3 signature — keep it positional
  so `python/blastit`'s call sites keep working), `crates/blastit-py/src/audit.rs:412-414`,
  `crates/blastit-py/src/check.rs:25,39`, `crates/blastit/src/audit_v1.rs:545,1447`. That is why
  `crates/blastit-py/src/check.rs` is in `paths` in addition to §21's `{lib,audit}.rs`.
- Mapping rules to implement and pin:
  * `missing_grant` → `add_query`, `to` is `view restaurants postgres db` (a named store appends its
    name: `view catalog postgres analytics`), and the editor inserts `query 'SELECT …'` inside that
    store's grant block, creating the block when absent;
  * connection-setup SQL → `add_group` with `to` = `trait connection` (the `MB.1.6` group hint keeps its
    group name; the node it writes is a `trait` plus a `use` line on every view);
  * `unrouted` → `add_route` with no `to` — `fix` carries `method`/`path`, and the view is inserted
    above the method-unrestricted catch-all `view`;
  * a body seen on a view without a codec → `declare_ingress`, whose `fix` is `fix_ingress` and whose
    `to` is `view upload body`;
  * `raw` is never written, `unvalidated` never appears in a `fix`, and a statement inlining a literal
    is still refused rather than written.
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
2. `check --learn` still proposes views only; `--json` bytes match the documented v1 schema with the
   page's action names (`add_query`/`add_group`/`declare_ingress`/`add_route`) and the node locators in
   `fix.to`, and the literal-refusal case still refuses.
3. The connection-setup finding is `add_group` with `to` = `trait connection`, and the text it writes is
   a `trait` block plus a `use` line on every view (assert the exact strings).
4. `raw` is never emitted by any learn path, and `unvalidated` appears in no `fix` object (assert on the
   written text of every fixture and on the `--json` document).
5. `make test-critical` (zero skips) and `make check` exit 0; `critical_guard.py` clean.

# Notes

- `python/tests/critical/test_check_contract.py` is protected: it is in `paths` for the fixture format
  change only; do not relax a schema assertion.
- Keep the human-readable finding text stable where a docs page quotes it (`docs/ci.md` is rewritten by
  `MB.21.5.1`, so say in `RESULT.md` which sentences moved).
