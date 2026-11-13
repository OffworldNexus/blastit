---
name: Git workflow
description: Blastit autonomous git conventions. Covers working on `release/<version>` branches, staging only a node's paths, pre-commit gates (`make check`), commit message format, and how `conductor` opens Release PRs into `develop` for human merge.
---

# Git workflow

- **Working branch (`release/<version>`):** Autonomous development happens on `release/<version>` branches (e.g. `release/0.1.1`, `release/0.2.0`) branched from `origin/develop` — **never** forced onto `develop`. If a session begins on `develop` with local commits ahead of `origin/develop`, `conductor` switches onto `release/<version>` (`git switch -c release/<version>`) so all commits live on the release branch without rewriting history.
- **Release PRs (`conductor` only) & human-only merge:**
  - Pushing directly to `develop`, creating git tags (`git tag`), force-pushing, and merging PRs (`gh pr merge`) are **strictly forbidden** for every agent.
  - Subagents (`lead`, `implementer`, `doc-writer`, `designer`, `strategist`, etc.) never run `git push` or `gh pr` at all.
  - Whenever a milestone or sub-release gate passes (`RELEASE_READY: <version>`), **only `conductor`** pushes `release/<version>` to `origin` and opens/updates a GitHub Pull Request into `develop` (`gh pr create --base develop --head release/<version> ...`), then records a `plan/HUMAN.md` merge item and pauses `autopilot`.
  - **Merging that PR on GitHub is done by a human**, which validates `CI`, `Docs`, and `Live (DigitalOcean)` on `develop` and automatically publishes the wheels & sdist to PyPI, creates the annotated `v<version>` git tag, and creates the GitHub Release (`.github/workflows/publish-pypi.yml`).
  - Once the PR is squash-merged into `develop`, `conductor`'s next Boot fetches `origin` and cuts the next `release/<next-version>` branch fresh from `origin/develop`.
- **Never rewrite history:** Never run `git reset --hard`, `git rebase`, `git commit --amend`, `git checkout --`, or `git clean`.
- **Who commits:** Only `lead` and `conductor`, once per reviewed & passed node. Leaf workers (`implementer`, `doc-writer`, `designer`, `researcher`, `expert`) **never** run `git add` or `git commit`.
- **What to stage:**
  - The completed node's `paths` globs;
  - `docs/knowledge/LESSONS.md` or `docs/research/INDEX.md` if updated;
  - Its plan folder (`plan/nodes/.../<ID>/`), `plan/JOURNAL.md`, `plan/STATE.md` (and `plan/ESCALATIONS.md` / `plan/HUMAN.md` if touched).
  - Always check `git status --short` first and run `git add -- <explicit paths>`, never blind `git add -A`, so parallel nodes' files stay untouched.
- **Mandatory pre-commit checks (`lead`):**
  1. `uv run --no-project tools/plan.py validate`
  2. `uv run --no-project tools/guard.py`
  3. `make check` (runs Rust fmt/clippy/doc/tests, Python ruff/tests, critical guard, runnable docs, and `zensical build --strict`)
- **Commit message format:**

  ```text
  [MA.2] Add unprivileged `blastit dev` simulation mode

  <2-5 lines explaining what changed and how it improves DX / security>

  Plan-Node: MA.2
  Reviewed-by: reviewer (attempt 1)
  ```

- **Index lock:** If `.git/index.lock` exists because another process is mid-operation, wait 5 seconds and retry (up to 5 times). Never delete `.git/index.lock` while a `git` process is running.
