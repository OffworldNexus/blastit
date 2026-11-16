---
id: MB.21.4.4
title: "Migrate Python package, Makefile and every e2e fixture to Blastfile and delete the TOML reader"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.3]
paths:
  - "python/blastit/**"
  - "python/tests/**"
  - "e2e/**"
  - Makefile
  - "scripts/**"
  - "crates/blastit-manifest/**"
  - "crates/blastit/src/**"
  - Cargo.toml
  - Cargo.lock
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Move the Python package, the Makefile and every `e2e/` fixture onto `Blastfile`, then **delete the TOML
reader and patcher** so the repository has exactly one manifest format.

# Context

- Measured files to migrate (fifteen fixtures + package defaults): `e2e/demo/blastit.toml`,
  `e2e/php/fixture/blastit.toml`, `e2e/sqlalchemy/blastit.toml`,
  `e2e/static_site/{blastit.toml,nospa/blastit.toml}`, `e2e/venv-smoke/blastit.toml`,
  `python/tests/{project,packaged_project,live_project}/blastit.toml`,
  `python/tests/runnable_docs/fixtures/{hello,shop,learn,denial,findings,static-site}/blastit.toml`.
- Package surface: `python/blastit/fold.py` (reads and writes the manifest through the editor,
  `--manifest blastit.toml` default in docstrings/argparse), `python/blastit/pytest_plugin.py`
  (`--blastit-manifest` default and its help text, plus the "a `blastit.toml` in the project" wording),
  and `blastit_build.py` if it names the file.
- Makefile: `DOCS_SERVE_MANIFEST := $(DOCS_SERVE_STAGE)/blastit.toml` and the stage that writes it;
  rename to `Blastfile` and keep `docs-serve`/`docs-serve-check` working.
- `scripts/**`: anything that stages or greps a manifest (`scripts/build-wheel.sh`,
  `scripts/release-notes.sh`, the docs harness fixtures reader) moves with the format.
- **Delete the TOML path last, in this leaf:** `Manifest::from_toml_str`, `parse.rs`'s TOML reader and
  `edit.rs`'s TOML patcher exist only until here. `crates/blastit-manifest/{src,tests}` and
  `Cargo.toml`/`Cargo.lock` are in `paths` for that removal; a dependency that only the TOML reader used
  is dropped in this one serialized commit.

# Acceptance

```console
$ make check
$ make e2e
$ grep -rn 'from_toml_str\|\[\[route\]\]' crates/blastit-manifest/src crates/blastit/src | wc -l
$ find e2e python/tests scripts -name 'blastit.toml' | wc -l
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests -q
```

1. `make check` exit 0 (`docs-unit`, `docs-test`, `test-critical`, `rs-no-python`, `plan-validate`) and
   `make e2e` exit 0 with every scenario loading a `Blastfile`.
2. Both counts above are `0`: no TOML reader symbol and no `blastit.toml` fixture remains.
3. `blastit-manifest fold` reads and writes `Blastfile` (atomic, comment-preserving) and its `--help`
   names the new default; `pytest --blastit-manifest` defaults to `Blastfile`.
4. `make docs-serve-check` still passes (it writes its own `Blastfile` in the stage directory).
5. `git diff --stat Cargo.toml Cargo.lock` is either empty or explained in `RESULT.md` (one serialized
   commit, no version bumps beyond what removal requires).

# Notes

- Keep the docs' fences green: `docs/` still shows `blastit.toml` until `MB.21.5.1`, so a docs fixture
  that the harness stages must be migrated by renaming its staged file, not by editing the page.
- `make e2e` needs Docker; if the host cannot run it, say exactly which scenarios were skipped and why,
  and leave the scenario runnable for the epic gate.
