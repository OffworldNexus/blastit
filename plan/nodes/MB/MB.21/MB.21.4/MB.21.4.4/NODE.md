---
id: MB.21.4.4
title: "Migrate the Python package, Makefile and every e2e fixture to Blastfile, delete the v1 reader and drop the toml dependency"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.3]
paths: ["crates/**", "python/**", "e2e/**", "scripts/**", Makefile, Cargo.toml, Cargo.lock]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T01:49"
---

# Goal

One manifest format in the repository. Every fixture, package default, script and Makefile stage is a
`Blastfile`; the v1 reader and patcher are deleted; the `toml` crate is gone from
`crates/blastit-manifest` and from `Cargo.lock`. After this leaf no Rust or Python file in the
repository contains a TOML parse path, and no fixture or script names the retired file.

# Context

- Fixtures to migrate (fifteen, all currently hold a v1 manifest): `e2e/demo/`,
  `e2e/php/fixture/`, `e2e/sqlalchemy/`, `e2e/static_site/`, `e2e/static_site/nospa/`,
  `e2e/venv-smoke/`, `python/tests/project/`, `python/tests/packaged_project/`,
  `python/tests/live_project/`, and `python/tests/runnable_docs/fixtures/{hello,shop,learn,denial,
  findings,static-site}/`. Rename the file in place to `Blastfile` and translate its body
  (`docs/reference/blastfile.md` is the truth; the planner's mapping table in `MB.21` is the checklist).
- Package surface: `python/blastit/fold.py`, `python/blastit/pytest_plugin.py` (the
  `--blastit-manifest` path default and its help text), `python/blastit/blastit_build.py`.
- Makefile: the `DOCS_SERVE_MANIFEST` variable and the stage that writes it (line 85 and its recipe);
  keep `docs-serve` and `docs-serve-check` working with `Blastfile`.
- Scripts: `scripts/standalone-runtime-check.sh` writes its fixtures with a heredoc tagged by the
  retired format's name (lines 67, 92, 165, 186) — retag it `BLASTFILE`; also check
  `scripts/wheel-smoke.sh`, `scripts/release-notes.sh` and `scripts/build-wheel.sh`.
- **Delete the v1 path last, in this leaf:** `Manifest::from_toml_str`, `parse.rs`'s v1 reader,
  `edit.rs`'s v1 patcher, any `toml::` call site, and the `toml` dependency at
  `crates/blastit-manifest/Cargo.toml:19` (workspace `Cargo.toml` carries no `toml`; `toml_edit` does
  not exist anywhere — confirm, do not assume). `Cargo.toml`/`Cargo.lock` are in `paths` because this
  is the one serialized commit that drops the crate.
- Rename the `manifest_toml` parameter names to `manifest_text` and fix their docstrings:
  `crates/blastit-py/src/lib.rs:156,179,302,326-333` (a positional PyO3 signature — keep it
  positional so `python/blastit` call sites keep working), `crates/blastit-py/src/audit.rs:412-414`,
  `crates/blastit-py/src/check.rs:25,39`, `crates/blastit/src/audit_v1.rs:545,1447`.
- One-line comment fixes outside the main crates: `crates/blastit-env/tests/environment.rs:469`
  (the `BLASTIT_MANIFEST` default it asserts is `/app/<file>`), `crates/blastit-sqlpolicy/src/learn.rs:4`
  (module doc), `crates/blastit-scheduler/src/config.rs:279` (field doc).
- `crates/blastit-manifest/tests/docs_fences.rs`: it still recognises the old fence opening until
  `MB.21.5.1` rewrites the pages, so build its opening constant from two fragments (the literal
  `"blastit"` and the literal `".toml"`, joined with `concat!`) so the tree never carries the
  contiguous name; leave the strict flip to `MB.21.5.2`.
- Build residue is not source: `__pycache__/*.pyc`, `python/blastit/_blastit*.so` and `target/` contain
  the name and must never be edited; every grep below excludes them.
- Docs still show the old fences until `MB.21.5.1`: keep them green by renaming the file the harness
  stages, never by editing a page.

# Acceptance

```console
$ make check
$ make e2e
$ grep -rnE 'toml::|from_toml_str|toml_edit' crates/ python/ --exclude-dir=__pycache__ --exclude-dir=target
$ grep -rnE 'blastit[.]toml' crates/ python/ scripts/ e2e/ Makefile --exclude-dir=__pycache__ --exclude='*.so' --exclude-dir=target
$ cargo tree -p blastit-manifest --locked --edges normal | grep -ci toml
$ find e2e python/tests -name 'Blastfile' | wc -l
$ find e2e python/tests -name '*.toml' ! -name 'Cargo.toml' ! -name 'inventory.toml' | wc -l
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.4
```

1. `make check` exit 0 (`test-critical`, `rs-test`, `rs-no-python`, `docs-unit`, `docs-check`,
   `plan-validate`) and `make e2e` exit 0 with every scenario loading a `Blastfile`.
2. The first grep prints nothing (no `toml::` path, no v1 entry point); `cargo tree … | grep -ci toml`
   prints `0`; the second grep prints nothing (no fixture, script or Makefile line names the retired
   file).
3. The two `find` counts are `≥ 15` and `0`: every fixture is a `Blastfile`, and nothing under `e2e/`
   or `python/tests/` is a `.toml` file apart from the runnable-docs inventory and the seccomp
   fixture's own `Cargo.toml`.
4. `blastit fmt --check ./Blastfile` passes in every migrated fixture; `blastit-manifest fold` reads
   and writes `Blastfile` atomically and comment-preserving with `--help` naming the new default;
   `pytest --blastit-manifest` defaults to `Blastfile`; `make docs-serve-check` passes writing its own
   `Blastfile`.
5. `git diff --stat Cargo.toml Cargo.lock` is explained in `RESULT.md` (one serialized commit, only
   the dependency removal, no version bumps) and `critical_guard.py` is clean for every protected test
   this leaf rewrote.

# Notes

- `crates/blastit/tests/**`, `crates/blastit-manifest/tests/**` and `python/tests/critical/**` are
  protected: this leaf rewrites fixtures and assertions inside them, so they are listed in `paths`
  and the `critical_guard.py` run above is mandatory.
- Do not add a compatibility shim, a fallback reader, or a `--manifest`-format switch: the point is
  that there is exactly one reader. A file that does not parse is a file that does not parse.
- `make e2e` needs Docker; if the host cannot run it, say exactly which scenarios were skipped and
  why, and leave every scenario runnable for the epic gate.
- If a crate outside this leaf's behaviour still names the retired file after the purge, do not leave
  it: this node owns `crates/**`, so fix it here and say so in `RESULT.md`.
