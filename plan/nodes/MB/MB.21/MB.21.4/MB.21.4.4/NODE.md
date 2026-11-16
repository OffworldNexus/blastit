---
id: MB.21.4.4
title: "Rewrite every e2e/ fixture, the remaining python/tests fixtures, the scripts and the Makefile's docs-serve stage to Blastfile, and take make e2e green"
kind: task
status: active
owner: implementer
priority: 1
depends_on: [MB.21.4.7]
paths: ["e2e/**", "python/tests/**", "scripts/**", Makefile]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T14:43"
---

# Goal

One manifest format in the repository's fixtures, scripts and build stages outside the crates: every
remaining `e2e/` fixture, the `python/tests/` fixtures `MB.21.4.1` did not already migrate, every
script's staged manifest and the Makefile's `docs-serve` stage is a `Blastfile` — and **`make e2e` is
green again**, which is this leaf's gate.

**No deletion work.** The reader, `Manifest::from_toml_str`, the TOML synthesis in `stage1`, the
dependency and the last Rust mentions are `MB.21.4.5`'s — it runs alone afterwards because it is the
only leaf that owns `Cargo.toml`/`Cargo.lock`.

# Context

- **`make e2e` is red between `MB.21.4.1` and this leaf, and that is accepted.** `MB.21.4.1` flips every
  entry point to `./Blastfile` and translates what `make check` reaches; the `e2e/` harness fixtures and
  scripts change here, so `make e2e` fails from 4.1's commit until this one. Only the epic gate
  (`MB.21.4`'s acceptance 2) requires `make e2e` exit 0, and **no leaf may weaken a gate to cover the
  gap** — no skipped scenario, no `xfail`, no loosened assertion, no `BLASTIT_MANIFEST` pointed at a
  translation shim. A scenario that cannot be green says exactly that in `RESULT.md` and is left runnable
  for the epic gate.
- Fixtures to rewrite (nine, all currently hold a retired-format manifest): `e2e/demo/`, `e2e/php/fixture/`,
  `e2e/sqlalchemy/`, `e2e/static_site/`, `e2e/static_site/nospa/`, `e2e/venv-smoke/`,
  `python/tests/project/`, `python/tests/packaged_project/`, `python/tests/live_project/`. Rename the file
  in place to `Blastfile` and translate its body (`docs/reference/blastfile.md` is the truth; the planner's
  mapping table in `MB.21` is the checklist).
- **Already migrated, do not re-touch:** `MB.21.4.1` converted the six `python/tests/runnable_docs/fixtures/`
  directories (`hello`, `shop`, `learn`, `denial`, `findings`, `static-site`), the runnable-docs harness
  (staged file name, `READER_FILES`, the page-manifest mode) and every `crates/blastit/tests/` suite.
  They are listed here only so the `find` counts below make sense: a hit there is 4.1's, not yours.
- Package surface: **not this leaf's, and the last word is `MB.21.4.7`'s** — `python/blastit/
  {fold,pytest_plugin}.py`'s readers, the PyO3 `blastfile_apply_edits` call, `_blastit.audit`'s parse, the
  `--blastit-manifest` default and its help text, and the four Python suites' own fixtures belong to 4.7,
  which runs before this leaf. `MB.21.4.3` landed the Rust contract and the binding; `manifest_toml` →
  `manifest_text` and the plugin's help text moved there. **This leaf translates only what 4.7 leaves** —
  the three project fixture directories below — and proves the rewritten `python/tests/` fixtures run
  through the plugin 4.7 cut over. Do not re-translate a suite 4.7 already owns.
- Makefile: the `DOCS_SERVE_MANIFEST` variable and the stage that writes it (line 85 and its recipe);
  keep `docs-serve` and `docs-serve-check` working with `Blastfile`, and drop the retired file name from
  the variable's value (rename the variable if it reads as the retired path).
- Scripts: `scripts/standalone-runtime-check.sh` writes its fixtures with a heredoc tagged by the
  retired format's name (lines 67, 92, 165, 186) — retag it `BLASTFILE`; also check
  `scripts/wheel-smoke.sh`, `scripts/release-notes.sh` and `scripts/build-wheel.sh`.
- **Not this leaf's work:** `Manifest::from_toml_str`, `parse.rs`'s reader, `edit.rs`'s old patcher, any
  `toml::` call site and the `toml` dependency are `MB.21.4.5`'s. Do not delete a reader, drop a
  dependency or edit `Cargo.toml`/`Cargo.lock` here: a second leaf touching the lockfile is exactly what
  the epic forbids. If a fixture cannot be green until the reader is gone, say so in `RESULT.md`.
- One-line comment fixes outside the main crates: `crates/blastit-env/tests/environment.rs:469` is
  `MB.21.4.5`'s (the reader it calls goes there). If a `scripts/` or `Makefile` line needs a non-manifest
  change, it is yours.
- `crates/blastit-manifest/tests/docs_fences.rs` is **not** this leaf's: `MB.21.4.1` moved the one page
  whose block it executes, the remaining eight pages are `MB.21.5.1`'s, and `MB.21.4.5` builds the
  retired opening from two fragments so the tree never carries the contiguous name.
- Build residue is not source: `__pycache__/*.pyc`, `python/blastit/_blastit*.so` and `target/` contain
  the name and must never be edited; every grep below excludes them.
- Docs still show the retired openings until `MB.21.5.1`: keep them green by what the harness stages
  (4.1's work), never by editing a page.

# Acceptance

```console
$ make check
$ make e2e
$ grep -rnE 'blastit[.]toml' python/ scripts/ e2e/ Makefile --exclude-dir=__pycache__ --exclude='*.so' --exclude-dir=target
$ find e2e python/tests -name 'Blastfile' | wc -l
$ find e2e python/tests -name '*.toml' ! -name 'Cargo.toml' ! -name 'inventory.toml' | wc -l
$ git diff --stat Cargo.toml Cargo.lock
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.4
```

1. **`make e2e` exit 0** with every scenario loading a `Blastfile` — this is the repair of the gap opened
   by `MB.21.4.1`, and no scenario is skipped to reach it — and `make check` exit 0 (`test-critical`,
   `rs-test`, `rs-no-python`, `docs-unit`, `docs-check`, `plan-validate`).
2. The `grep` prints nothing: no fixture, script or Makefile stage names the retired file. Every
   remaining Rust mention is `MB.21.4.5`'s and is listed in this leaf's `RESULT.md` with its file, so the
   next dispatch needs no search of its own.
3. The two `find` counts are `≥ 15` and `0`: every fixture is a `Blastfile` (nine here plus the six
   `MB.21.4.1` migrated), and nothing under `e2e/` or `python/tests/` is a `.toml` file apart from the
   runnable-docs inventory and the seccomp fixture's own `Cargo.toml`.
4. `blastit fmt --check ./Blastfile` passes in every rewritten fixture; the rewritten `python/tests/`
   fixtures run through `pytest --blastit-manifest` (whose default `MB.21.4.7` moved) and
   `make docs-serve-check` passes writing its own `Blastfile`.
5. This leaf changes no dependency: the `git diff --stat` above is empty, and `critical_guard.py` is
   clean for every protected test this leaf rewrote.

# Notes

- `python/tests/critical/**` is protected: this leaf rewrites fixtures beside it, so those paths are in
  `paths` and the `critical_guard.py` run above is mandatory. The crate-side protected suites are
  `MB.21.3.7`'s, `MB.21.4.1`'s, `MB.21.4.2`'s and `MB.21.4.3`'s.
- Do not add a compatibility shim, a fallback reader, or a `--manifest`-format switch: the point is that
  there is exactly one reader. A file that does not parse is a file that does not parse.
- `make e2e` needs Docker; if the host cannot run it, say exactly which scenarios were skipped and why,
  and leave every scenario runnable for the epic gate — the gate is not waived, only deferred with a
  named blocker.
- If a crate still names the retired file after this leaf, leave the mention for `MB.21.4.5` and record
  it in `RESULT.md`; rewording a fixture is yours, deleting a reader is not.
- `scripts/**` is in `paths` because the staged-manifest heredocs live there: retag them `BLASTFILE` and
  keep every script's behaviour (`standalone-runtime-check.sh`, `wheel-smoke.sh`, `release-notes.sh`,
  `build-wheel.sh`).
