---
id: MA.7
title: "Leaf 14: green the release CI (sudo-rs smoke job, pytest fixture collection)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [.github/workflows/ci.yml, "python/tests/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T11:02"
updated: "2026-11-13T11:08"
---

# Goal

Make the Release PR's CI fully green. Two jobs are red on `release/0.1.1` (PR #62, run 43404219253) for reasons unrelated to the 0.1.1 product code; both are workflow/harness defects that would block the release.

# Context

Diagnosis from the failed runs (logs quoted, no guessing):

1. **`smoke (root)`** — fails in ~5 s:
   ```text
   sudo: preserving the entire environment is not supported, '-E' is ignored
   error: rustup could not choose a version of cargo to run, because one wasn't
   specified explicitly, and no default is configured.
   ```
   The step is `sudo -E env "PATH=$PATH" cargo test -p blastit --locked -- --test-threads=1` (`.github/workflows/ci.yml`, `smoke (root)` job). The runner image now ships **sudo-rs**, which ignores `-E`, so root inherits neither `RUSTUP_HOME` nor `CARGO_HOME` and the rustup shim has no toolchain. This line dates from `C1b` (#16) and only broke through runner-image drift.
   Fix direction: stop relying on `-E`. Pass what root actually needs explicitly, e.g. `sudo env "PATH=$PATH" "RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}" "CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}" cargo test ...` (plus `PYO3_PYTHON`/`LD_LIBRARY_PATH` if the job needs them), or invoke the resolved toolchain binary directly. Whatever you pick must keep the job's meaning: the root/privileged smoke test still runs as root.

2. **`pytest plugin (CPython 3.14, PostgreSQL)`** — fails collecting the runnable-docs fixtures:
   ```text
   ERROR python/tests/runnable_docs/fixtures/{findings,learn,shop}/tests/test_app.py
   E   ModuleNotFoundError: No module named 'app'
   !!! Interrupted: 3 errors during collection !!!
   ```
   The job runs `/tmp/blastit-tests/bin/pytest python/tests -q`, which recurses into fixture apps that are meant to be driven by the runnable-docs harness from their own directory, not collected at repo root.
   Fix direction: narrow that job's collection the way `make docs-unit` already does (explicit files, or an `--ignore` for `python/tests/runnable_docs/fixtures`). **Do not** add a global `norecursedirs`/`collect_ignore` that could change what the docs harness itself collects — the fence tests run pytest *inside* those fixture directories on purpose, and all 38 fences must stay green.

# Acceptance

1. `pytest python/tests -q --collect-only` from the repo root no longer errors on the fixture apps under the same conditions the CI job uses (reproduce first, then prove the fix). Paste before/after output.
2. `make check` exit 0 (all 38 doc fences: 26 executed / 12 accounted / 0 failed, `zensical --strict`, `plan OK`) — the docs harness is untouched in behaviour.
3. The `smoke (root)` step is rewritten to a sudo-rs-compatible form and the workflow still runs `cargo test -p blastit` **as root**; paste the new step lines and explain why root can now find a toolchain.
4. `uv run --no-project tools/plan.py validate` ok; the change is limited to `.github/workflows/ci.yml` (and a test file only if genuinely needed).
5. **CI is the real proof:** the Release PR's checks go 100 % green (all previously failing jobs pass). If a workflow-only fix cannot be proven locally, say exactly which job needs the next CI run to confirm — the release is not presented to the owner until then.

# Notes

- No product-code change is expected here; do not "fix" the fixtures by adding sys.path hacks to them, and do not weaken any guard or skip a job.
- Keep the diff minimal and commented where the sudo-rs workaround is subtle.
- Do not push; the conductor pushes `release/0.1.1` and lets CI re-run.
