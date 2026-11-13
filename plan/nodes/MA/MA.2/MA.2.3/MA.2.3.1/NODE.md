---
id: MA.2.3.1
title: "Leaf 8: python ruff hygiene, Makefile gates and fold literal/atomic hardening"
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MA.2.1.3]
paths: [Makefile, "python/blastit/**", "python/tests/**"]
attempts: 3
max_attempts: 4
escalation: expert
created: "2026-11-12T13:12"
updated: "2026-11-12T16:21"
---

# Goal

Make the Python tree lint-clean and the critical tier real: fix every `ruff check`/`ruff format` issue under `python/`, gate `python tools` in `make py-lint`/`py-format`, wire the zero-skip guard + `_blastit<EXT_SUFFIX>`/`blastit-dbproxy` staging + offline `unittest discover` into `make test-critical`, prove `dependencies == []`, and harden `fold.py` for literal refusal plus atomic `.tmp` writes.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §7.1 (offline recipe, skip regex), §7.2 rows `R25 (fold)`, `R39..R42`, §8 Leaf 8; requirements `R25` (fold half), `R29` (temp-file pattern), `R40`, `R41`, `R42`.
- `Makefile` gains the skip-attribute/call regex (`#\[\s*ignore`, `\bneeds_root\s*\(`, `pytest\.mark\.skip`, `pytest\.skip\s*\(`, `importorskip\s*\(`, `unittest\.skip`) so **zero** critical test can skip, plus the `cargo build --locked -p blastit-py -p blastit-dbproxy` staging recipe with `RUST_ENV` (`PYTHONHOME`, `LD_LIBRARY_PATH`) and `CARGO_NET_OFFLINE=1 UV_OFFLINE=1`.
- `python/blastit/fold.py` keeps documented exit codes `0`/`1`, refuses inline literals/`…`/multi-statement/non-DML/unsafe functions, and writes with `.{name}.{pid}.tmp` + `fsync` + `os.replace`.
- Load skills `python-standards`, `testing`, `security-invariants`.

# Acceptance

```console
$ uvx --offline ruff check python tools
$ uvx --offline ruff format --check python tools
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make test-critical
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
```

- `python/tests/critical/test_fold_literals_and_atomic.py` is **new**: `fold` refuses `'secret'` and `…` statements, writes a parsable manifest diff on the happy path, exits `0` on readable input even with refusals, `1` on unreadable input, and leaves no `.tmp` file on error.
- `python/tests/critical/` also proves `pyproject.toml` `dependencies == []` via `tomllib` and that `python/blastit/*.py` imports only stdlib or `blastit`.
- `make test-critical` fails loudly if any critical test file contains a skip construct.

# Notes

- ~350 changed lines; may run parallel with `MA.2.2.3` and `MA.2.2.4`. Do **not** touch `crates/**` (except reading), `e2e/**`, or `docs/**`.
- `python/tests/test_check.py` and `_check_pytest.py` belong to Leaf `MA.2.3.5`; leave them out of this leaf.
