---
name: Git workflow
description: Blastit autonomous git conventions. Covers local commits on `develop`, staging only a node's paths, pre-commit gates (`make check`), commit message format, and why pushing/tagging is reserved for the human owner.
---

# Git workflow

- **Branch:** `develop` locally.
- **No autonomous push or tag:** Pushing `develop` to `origin` immediately deploys the public documentation site to GitHub Pages (`docs.yml`) and triggers paid cloud provisioning on DigitalOcean (`live.yml`), and pushing `v*` tags publishes to PyPI (`publish-pypi.yml`). Therefore `git push`, `git tag`, and `gh pr`/`gh release` are **strictly forbidden** for agents. When a milestone release is ready, `strategist` opens a `plan/HUMAN.md` item with the exact release checklist from `RELEASING.md` for the owner.
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
  3. `make check` (runs Rust fmt/clippy/doc/tests, Python ruff/tests, critical guard, and `zensical build --strict`)
- **Commit message format:**

  ```text
  [MA.2] Add unprivileged `blastit dev` simulation mode

  <2-5 lines explaining what changed and how it improves DX / security>

  Plan-Node: MA.2
  Reviewed-by: reviewer (attempt 1)
  ```

- **Index lock:** If `.git/index.lock` exists because another process is mid-operation, wait 5 seconds and retry (up to 5 times). Never delete `.git/index.lock` while a `git` process is running.
