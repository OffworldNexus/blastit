---
id: MA.3.8
title: "Wire the docs gate and critical-test guards into CI"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [.github/workflows/ci.yml, .github/workflows/docs.yml]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-13T01:19"
updated: "2026-11-13T02:05"
---

# Goal

Run the `MA.3` verification tier in CI: the runnable-docs gate and the two critical-test guards
(`F6` in `docs/testing/AUDIT-MA.3.md`).

# Context

- No workflow calls `make` (`grep -rn "make " .github/workflows` → nothing). Consequences:
  - `make docs-test` (the snippet gate) runs only on a developer's machine;
  - `make critical-guard` (`tools/critical_guard.py`) is not enforced on pull requests, so a PR can
    weaken a protected `critical_*.rs` / `python/tests/critical/` file and stay green;
  - `make critical-skip-check` (`R42`: no `#[ignore]`, no `pytest.mark.skip`, …) is local-only.
- The critical *tests* themselves do run in CI: `test` uses `cargo test --workspace --all-targets`
  and `pytest-plugin` uses `pytest python/tests` (which collects `python/tests/critical/`).
- `make docs-test` needs `cargo`, `uv`, a CPython the wheel's tag matches, and a `pytest==9.2.0`
  entry in the uv cache (`F7`); CI must install that cache entry explicitly before the gate.

# Acceptance

```console
$ cat .github/workflows/ci.yml          # a `docs-gate` job runs: uv pip install pytest==9.2.0; make docs-test
$ cat .github/workflows/ci.yml          # a `guards` job runs: make critical-skip-check; make critical-guard BASE=origin/<base>
```

- A new job `docs-gate` (ubuntu-latest, CPython 3.13 + `dtolnay/rust-toolchain`, `Swatinem/rust-cache`)
  runs `make docs-test` on every PR and push to `develop`, with `UV_CACHE_DIR`/home cached and
  `uv pip install pytest==9.2.0` before it. It uploads the verdict table on failure.
- A new job `guards` runs `make critical-skip-check` and
  `uv run --no-project tools/critical_guard.py --base "origin/${GITHUB_BASE_REF:-develop}"`
  (fetch-depth 0) and fails the PR on a violation. It must run on pull requests and on `develop`.
- A red `docs-gate` names the fence (the harness already does) in the job log; the workflow does not
  swallow the exit code.
- `docs.yml` keeps its `--strict` build; it additionally asserts the `site/orphan` class cannot
  regress by running the same `docs-test` inventory check first (or by depending on `docs-gate`).
- `RESULT.md` pastes a green run of both jobs (a `workflow_dispatch` run on the branch is enough if
  the PR cannot be opened from here) and the `guards` failure text for a deliberately weakened
  protected test on a scratch branch.

# Notes

- Never `git push`/open a PR from the agent session; if a live run is impossible offline, paste the
  `act`-free dry evidence (`make critical-guard BASE=<sha>` locally with a weakened file restored
  afterwards) and say so in `RESULT.md`.
- Do not touch `Makefile` targets here: `docs-test`/`critical-guard`/`critical-skip-check` already
  exist and work locally (verified in `AUDIT-MA.3.md`); this node is wiring only.
