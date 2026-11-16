---
id: MB.21.4.7
title: "Cut python/blastit over to Blastfile: fold and the pytest plugin through blastfile_apply_edits, _blastit.audit off the retired reader, and the four Python suites translated"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.3]
paths: ["python/blastit/**", "python/tests/**", "crates/blastit-py/src/**", "crates/blastit-py/tests/**"]
attempts: 0
max_attempts: 5
escalation: none
created: "2026-11-16T12:54"
updated: "2026-11-16T12:54"
---

# Goal

Cut the Python package over to `Blastfile`: `python/blastit/fold.py` and `python/blastit/pytest_plugin.py`
read and write a `Blastfile` through the PyO3 editor — `_blastit.blastfile_apply_edits(source,
edits_json) -> str` for every write, never a Python-side splice — and `_blastit.audit`'s `audit()` parses
with `from_blastfile_str` instead of `Manifest::from_toml_str`. `fold` keeps every documented flag
(`--manifest PATH`, `--write`, `--prefer-tables`, `--allow-dangerous`), the `--prefer-tables` and
`functions` grants go through the `ProposedGrant` table/function scope `MB.21.4.3` added, and the pytest
plugin's default and help text name `Blastfile`. The four Python suites that run these modules are
translated to `Blastfile` fixtures and keep every assertion.

**This leaf re-points callers; it does not delete a reader.** `Manifest::from_toml_str`, `parse.rs`'s TOML
reader, `edit.rs`'s retired patcher and the `toml` dependency are `MB.21.4.5`'s. Nothing here edits
`Cargo.toml`/`Cargo.lock`, and no compatibility shim, fallback reader or `--manifest`-format switch is
added: after this leaf no default code path in `python/blastit` reaches the retired reader.

# Context

- **Why this is its own node.** `MB.21.4.3` landed the whole Rust contract (the `--json` locators, the
  §13.2/§10.2/§11.3 re-spellings, §18, the editor's `ProposedGrant` scope and the
  `blastfile_apply_edits` binding) and left the tree green; the Python cutover did not fit that session.
  It is size, not a design defect: ~2 500 lines of Python suites
  (`test_fold.py` alone is 2 014 lines of TOML planning cases) plus ~18 TOML fixtures and ~10
  global-`groups` unit tests inside `crates/blastit-py/src/audit.rs`.
- `python/blastit/fold.py` today parses the retired document with `tomllib` and plans additions from the
  log; `pytest_plugin.py` reads the manifest the same way. `_blastit.audit()` calls
  `Manifest::from_toml_str` (`crates/blastit-py/src/audit.rs`, whose parameter `MB.21.4.3` already renamed
  to `manifest_text`).
- Available after `MB.21.4.3`, already built and tested — do not re-do or re-derive it:
  * `_blastit.blastfile_apply_edits(source, edits_json) -> str` over `blastfile::apply_edits`
    (`BlastfileEdits` in, the proved source out);
  * `Manifest::parse_blastfile` / the Python `from_blastfile_str` reader;
  * `ProposedGrant { view, store, queries, tables, functions }`, whose table/function splice rewrites the
    store block's directives to the merged scope and refuses an unwritable name or an empty column list
    before splicing, so a repeated proposal adds nothing;
  * the audit report's `routes[].proxies[].missing[]` (what is missing per view/store) and
    `_blastit.covering` for `--prefer-tables`.
- `fold`'s write path keeps `_write_atomic` exactly as it is — the `.{name}.{pid}.tmp` sibling, `fsync`,
  `os.replace`, and the mode — because protected
  `python/tests/critical/test_fold_literals_and_atomic.py` pins it. Read the `Blastfile`, plan from the
  log, build the `BlastfileEdits` JSON, call the binding, then re-audit the edited **text** to prove it
  parses and validates before the atomic write.
- **Global `groups` do not exist in a `Blastfile`** (`MB.21` §7.1: `global_groups` is always empty). The
  `audit.rs` unit tests that assert a global group's grants on every route become traits-attached-by-`use`
  on every view; where the concept genuinely has no counterpart, delete the case and say in `RESULT.md`
  what replaced its coverage — a deleted case is never a silent skip.
- `python/tests/{project,packaged_project,live_project}/**` and `python/tests/runnable_docs/**` are
  `MB.21.4.4`'s; this leaf translates only the fixtures the four suites below run against and lists in
  `RESULT.md` what it leaves for 4.4.
- Python stays 100 % stdlib (`dependencies = []`); the only new call is into the already-linked `_blastit`
  extension. The wheel/link path is `crates/blastit-py`, so `make check` must still build it.

# Acceptance

```console
$ make test-critical
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/critical/test_check_contract.py python/tests/critical/test_fold_literals_and_atomic.py \
    python/tests/test_fold.py python/tests/test_pytest_plugin.py -q
$ grep -rnE 'tomllib|from_toml_str' python/blastit --include='*.py'
$ uv run --no-project tools/critical_guard.py --base <dispatch base sha> --node MB.21.4.7
$ make check
```

1. `fold` reads a `Blastfile` and writes only through `blastfile_apply_edits`: `--manifest PATH`,
   `--write`, `--prefer-tables` and `--allow-dangerous` behave exactly as `docs/reference/cli.md:95`
   says; the edited file re-parses **and validates**, every comment and blank line survives, and a second
   run adds nothing. `docs/reference/cli.md`'s `git diff` is empty — the flags did not change.
2. `--prefer-tables` writes the smallest `select`/`insert`/`update`/`delete` scope inside the view's
   `postgres [store] { }` block and `--allow-dangerous` writes `functions …`, both through the
   `ProposedGrant` table/function scope `MB.21.4.3` landed; no Python code splits, patches or concatenates
   manifest text to get there.
3. `_blastit.audit()`'s `audit()` parses with `from_blastfile_str`; `crates/blastit-py/src/audit.rs`'s
   ~18 TOML fixtures and its global-`groups` unit tests are translated to `Blastfile` (traits-on-every-view
   where the concept moved, deleted-with-reason where it did not), and every assertion is kept or
   strengthened.
4. The pytest plugin's default and help text name `Blastfile`, it reads a `Blastfile`, and the
   `manifest.warnings` sentence assertions name the new §13.2 spellings.
5. `make test-critical` (zero skips) and `make check` exit 0, the four Python suites are green
   (no new skip, no `xfail`), and `critical_guard.py --base <dispatch base sha> --node MB.21.4.7` is
   clean for the protected suites whose fixtures moved. Every changed assertion is listed in `RESULT.md`
   with the reason it is not weaker.
6. The `grep` prints nothing: no default code path in `python/blastit` names `tomllib` or
   `from_toml_str`. Reachability counts, not just the import — `fold`, the plugin and `_blastit.audit` are
   each exercised by a suite that runs their real parse path.

# Notes

- `max_attempts: 5` is measured, not generous: two suites (~2 500 lines), ~18 Rust-side fixtures, four
  translation surfaces (`fold`, the plugin, `audit()`, the protected atomicity suite) and a PyO3 build
  between each. A `needs_split` here is a signal to escalate, not to widen `paths`.
- **Protected suites, edited never weakened:** `python/tests/critical/test_fold_literals_and_atomic.py`
  (every literal/redaction refusal and the temp-file atomicity assertion stay) and
  `python/tests/critical/test_check_contract.py` (the `--json` document shape and `fix.to` locators stay).
  List each changed assertion and why it is not weaker in `RESULT.md`.
- **`crates/blastit-py/Cargo.toml` is deliberately not in `paths`.** If the cutover looks like it needs a
  dependency, it does not: report it — the lockfile is `MB.21.4.5`'s alone.
- Do not add a compatibility shim, a fallback reader or a `--manifest`-format switch. Exactly one reader
  answers; until `MB.21.4.5` deletes the retired one, it is still compiled but unreachable from Python.
- If a suite's fixture cannot be green until the Rust reader is deleted, say exactly which case and why in
  `RESULT.md` and leave it runnable for the epic gate — no skipped assertion to reach a green run.
- `_write_atomic`'s behaviour is pinned, not rewritten: the cutover changes *what* is written (proved text
  from the editor), never *how* the file lands.
