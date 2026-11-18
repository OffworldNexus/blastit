---
name: Git workflow
description: Blastit autonomous git conventions. Covers working on `release/<version>` branches, staging explicit node paths, leaf `make fast-check` vs epic-gate `make check`, commit message format, and Release PRs into `develop`.
---

# Git workflow

- **Working branch (`release/<version>`):** Autonomous development happens on `release/<version>` branches (e.g. `release/0.2.0`) branched from `origin/develop` — **never** forced onto `develop`. If a session begins on `develop` with local commits ahead of `origin/develop`, `conductor` switches onto `release/<version>` (`git switch -c release/<version>`) without rewriting history.
- **Release PRs (`conductor` only) & human-only merge:**
  - Pushing directly to `develop`, creating git tags (`git tag`), force-pushing, and merging PRs (`gh pr merge`) are **strictly forbidden** for every agent.
  - Subagents (`lead`, `implementer`, `writer`, `strategist`, `architect`, `expert`, etc.) never run `git push` or `gh pr` at all.
  - Whenever a milestone or sub-release gate passes (`RELEASE_READY: <version>`), **only `conductor`** runs full `make check`, pushes `release/<version>` to `origin`, opens/updates a GitHub Pull Request into `develop`, records a `plan/HUMAN.md` merge item, and pauses `autopilot`.
  - **Merging that PR on GitHub is done by a human**, triggering `CI`, `Docs`, `Live (DigitalOcean)`, and automated PyPI + GitHub Releases (`.github/workflows/publish-pypi.yml`).
- **Never rewrite history:** Never run `git reset --hard`, `git rebase`, `git commit --amend`, `git checkout --`, or `git clean`.
- **Who commits:** Only `lead` and `conductor`, once per reviewed & verified slice. Leaf workers (`implementer`, `writer`, `researcher`, `architect`, `expert`) **never** run `git add` or `git commit`.
- **What to stage:**
  - Explicit paths matching the completed node's `paths`;
  - `docs/knowledge/LESSONS.md` or `docs/research/INDEX.md` if updated;
  - Its plan folder (`plan/nodes/.../<ID>/`), `plan/JOURNAL.md`, `plan/STATE.md` (and `plan/ESCALATIONS.md` / `plan/HUMAN.md` if touched).
  - Always check `git status --short` first and run `git add -- <explicit paths>` — **never** blind `git add -A`.
- **Pre-commit gates (`lead`):**
  - **Per leaf slice:**
    1. `make fast-check` (plus `make docs-check` when `docs/` or `README.md` changed)
    2. `uv run --no-project tools/critical_guard.py --base <base> --node <id>`
    3. `uv run --no-project tools/guard.py`
    4. `uv run --no-project tools/plan.py validate`
  - **At epic/release gates (`gate <epic>` & Release PRs):**
    Run full `make check` **once** (covering `rs-doc`, Python critical staging, wheel build + `docs-test`, `rs-no-python` / `rs-no-default`, packaging/workflow/Zed gates, and `zensical build --strict`).
- **Commit message format:**

  ```text
  [MB.40.2] Fix store-less check crash, $? grant check & --learn reload bugs

  <2-4 lines explaining what changed and how it improves DX / security>

  Plan-Node: MB.40.2
  Reviewed-by: review-arch, review-ux
  ```

- **Index lock:** If `.git/index.lock` exists because another process is mid-operation, wait 5 seconds and retry (up to 5 times). Never delete `.git/index.lock` while a `git` process is running.
