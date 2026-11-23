---
id: MB.68
title: "Fix build-wheel action ordering: uv must exist before the build step"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [".github/actions/build-wheel/action.yml", .github/workflows/wheels.yml]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T09:22"
updated: "2026-11-23T09:26"
---

# Goal

`.github/workflows/wheels.yml` is red on every `develop` push, with the same defect I reported at 0.2.0 and which `MB.65` did not cover (`.github/actions/**` was outside its `paths`):

```
the README rewrite needs uv or PYO3_PYTHON: install uv, or point PYO3_PYTHON at an interpreter
##[error]Process completed with exit code 1.
```

`.github/actions/build-wheel/action.yml` runs `scripts/build-wheel.sh` **before** its `astral-sh/setup-uv` step, and `build-wheel.sh` needs `uv` (or `PYO3_PYTHON`) for the README-link rewrite. `publish-pypi.yml`'s wheel job happens to call `astral-sh/setup-uv` before the composite action (line ~532), which is the only reason release wheels build; `wheels.yml` does not.

# Change

- Move `astral-sh/setup-uv` in `.github/actions/build-wheel/action.yml` to **before** the "Build the universal wheel" step (it is needed by the build, not only by the later `e2e/run_packaging.py` step), and drop the now-duplicate later step.
- Make the requirement explicit in the action's `description`/comment: the build step needs `uv` or `PYO3_PYTHON`, and the action provides `uv` itself so no caller has to know.
- Do **not** change `scripts/build-wheel.sh`'s failure message into a silent fallback; failing closed when neither is present is correct.

# Acceptance

- `gh run` equivalent locally is not possible, but: `actionlint` (if available) or a YAML parse of the action passes; `git diff` shows only the step move + comment.
- Prove the ordering matters and is fixed: run the exact composite-action build step locally **with `uv` on `PATH`** and show `scripts/build-wheel.sh` completing its README rewrite (or at least passing that check), and show that with `uv` absent and `PYO3_PYTHON` unset it still fails closed with the same sentence.
- `make fast-check` green (this touches no Rust/Python).
