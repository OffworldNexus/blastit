---
id: MA.3.10
title: "Harden static deploy checks, wheel provenance and offline/HOME isolation"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: [python/tests/runnable_docs/harness.py, docs/testing/README.md]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-13T01:19"
updated: "2026-11-13T02:51"
---

# Goal

Close the `harness.py` half of the audit gaps: an executing mode must not pass on a fence with no
commands, the `static` deploy checks must reject commands and files that cannot exist, the
YAML/ini/SQL snippets a reader copies must be parsed, and the wheel under test must be the one the
workspace version names (`F2`, `F4`, `F7`, `F11` in `docs/testing/AUDIT-MA.3.md`).

# Context

- `mode_check` / `mode_learn_sequence` / `mode_findings` / `mode_shell`-style modes loop over
  `fence.commands()`; an empty tuple returns PASS. Audit evidence: `console fence whose `$ ` prompt
  was lost` → `mode_check` returned `('check reported a clean suite', True)` while asserting nothing.
- `_static_command` validates executables, `uv run` targets and `blastit`/`blastit-manifest` flags,
  but not the arguments of the tools it only name-checks. All of these pass today:
  `docker compose -f /nope/compose.yaml up -d`, `docker run --frobnicate ghcr.io/x/shop:nope`,
  `blastit-manifest fold --manifest /nope/blastit.toml`, `doctl apps create --spec /nope/app.yaml`,
  `psql --frobnicate postgres://x`.
- `mode_static` parses TOML only, and only on `deploy/` pages. The recipes' `yaml title="compose.yaml"`,
  `yaml title=".github/workflows/check.yml"`, `yaml title="networkpolicy.yaml"`,
  `ini title="/etc/systemd/system/shop.service"` and `sql title="db/grants.sql"` blocks are never read.
- `_static_json_lines` skips any line that does not parse, so a truncated boot line is unchecked.
- `wheel_path()` takes the newest mtime in `dist/`; with `blastit-0.1.0-…whl` (newer mtime) beside
  `blastit-0.1.1-…whl` it selected 0.1.0 and the file-by-file check then "verified" that wheel.
- `install_pytest` reads `uv cache dir`, i.e. the caller's `HOME`. With a scrubbed `HOME` the gate
  dies at `pytest==9.2.0 is not in the uv cache` after the build step downloaded `maturin` and a
  CPython (see `F7`).

# Acceptance

```console
$ make docs-test
$ uv run --no-project python -c 'import sys; sys.path.insert(0, "python/tests/runnable_docs"); import harness; print(harness.wheel_path())'
$ env -i HOME=/tmp/dh PATH="$PATH" make docs-test   # after `uv pip install pytest==9.2.0` into /tmp/dh
```

- Every executing mode raises `Failure` when a fence body has a non-blank line and
  `Fence.commands() == ()`; only an accounted mode may accept an empty body, and it still needs its
  written reason.
- `_static_command` rejects: an unknown flag on `docker`/`docker compose`/`psql`/`doctl`, and a
  `--file`/`-f`/`--spec`/`--manifest` value that is a workspace-relative path which does not exist.
  Absolute paths outside the repository stay unchecked (they belong to the reader's host) and the
  verdict line says so.
- `mode_static` parses every `yaml`, `ini`, `toml` and `sql` block of the page image-agnostically:
  YAML via a stdlib-only structure check (no new dependency — `pip install pyyaml` is forbidden by
  `python-standards`), `ini` via `configparser`, `toml` via `tomllib`, `sql` non-empty and free of
  `…`; a block that does not parse fails the fence naming the block's first line.
- `_static_json_lines` fails on a line that starts with `{` and does not parse, and on a boot line
  missing any of `event`/`stage`/`seccomp`/`privilege`/`bounding_set`.
- `wheel_path()` requires exactly one `dist/blastit-<Cargo.toml version>-*.whl` and names the
  version it expected when it is missing or doubled.
- `install_pytest`'s failure message names the prerequisite in `docs/testing/README.md`, and the
  harness states the offline+HOME contract in its module docstring; either the gate survives a
  scrubbed `HOME` once the prerequisite is documented, or the prerequisite is documented as the one
  command the gate needs. `RESULT.md` pastes both runs.

# Notes

- No new third-party Python dependency: `python-standards` forbids it and the gate must run offline.
  A 20-line YAML *structure* linter (indentation + `key: value`/`- item`) is enough for a compose
  file and a workflow; do not pretend to be a YAML parser.
- `python/tests/runnable_docs/harness.py` only. `fences.py`/`run.py`/`inventory.toml` belong to the
  scanner node — do not touch them here.
