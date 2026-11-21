---
id: MB.40.1
title: "Install GEPA G4 .opencode bundle, fast-check Makefile split, delete 7 regex guards & retire MB.26 micro-nodes"
kind: task
status: done
owner: expert
priority: 1
depends_on: []
paths: [".opencode/**", AGENTS.md, Makefile, "scripts/**", tools/plan.py, "plan/**", "docs/design/**"]
attempts: 1
max_attempts: 3
escalation: expert
created: "2026-11-19T00:16"
updated: "2026-11-19T00:50"
---

# Goal
Install the GEPA `G4_reflect_pareto_polish` prompt bundle into `.opencode/` and `AGENTS.md` (with production orchestration permissions & step caps), add `writer` to `tools/plan.py`, split `Makefile` into `fast-check` vs `check` (dropping the 52 duplicate `critical_*` re-runs and deleting the 7 pure source-grep regex guard scripts), and supersede the 56 `MB.26` micro-node parking lot items.

# Exact Targets (Zero Re-Research)
1. **Agents & `AGENTS.md` (`/home/remy/.cache/blastit-gepa/results/winning_bundle/`):**
   - Install `implementer.md` (`steps: 120`, keep `git commit *` / `git add *` deny + `explore` allow), `writer.md` (`steps: 60`, deny `**/crates/**`, `**/.opencode/**`, `git commit *`, `git add *`), `review-arch.md` (`steps: 60`, read-only), `review-ux.md` (`steps: 60`, read-only), `planner.md` (`steps: 60`), and `lead.md` (`steps: 200`, full Blastit `plan.py` + `git commit` subtree orchestration loop spawning `implementer`, `writer`, `review-arch` + `review-ux` in parallel, `planner`, `researcher`, `visual-checker`, `expert`, `architect`, `strategist` — running `make fast-check` per slice and full `make check` once at epic/release gates, zero `RESULT.md` line-number nit bounces, 1 sequential Cargo writer per worktree).
   - Remove obsolete `.opencode/agents/{critic,doc-writer,designer,reviewer,test-curator}.md`.
   - Update `.opencode/agents/{conductor,architect,strategist,expert,researcher,visual-checker}.md` and `.opencode/skills/*/SKILL.md` to reference `Blastfile` (zero retired TOML manifest names, retired external PHP daemon/transport names, or retired agent names) and match `G4` principles.
   - Update root `AGENTS.md` combining `winning_bundle/AGENTS.md` with Blastit's mandatory repo rules (`tools/plan.py`, `tools/critical_guard.py`, `release/<version>` git workflow, `uv` execution, confidentiality rule).
2. **`tools/plan.py`:** Add `"writer"` to `OWNERS` (`tools/plan.py:45-57`), and update `MB.40.6` owner to `writer`.
3. **Delete pure source-grep guard scripts & clean `Makefile`:**
   - Delete `scripts/check_doc_tone.py`, `scripts/test_doc_tone.py`, `scripts/check_manifest_purity.py`, `scripts/test_manifest_purity.py`, `scripts/check_lsp_portable.py`, `scripts/test_check_lsp_portable.py`, `scripts/check-no-inotify.sh` (keep `check-no-test-seam.sh`, `check_zed_extension.py`, `platform_matrix.py`, `check_workflows.py`, `test_packaging_gates.py`, `check-portable-elf.sh`, `check-exec-env.sh` intact as CI workflows/packaging scripts call them!). Also remove untracked `docs/design/portable-lsp.review-*.md` if untracked scratch.
   - In `Makefile`: remove deleted targets (`doc-tone`, `manifest-purity`, `lsp-portable`, `no-inotify`, and `scripts/test_doc_tone.py` from `docs-unit`); add `fast-check: guard critical-guard rs-fmt rs-lint rs-test py-lint plan-validate`; remove `rs-test-critical`'s 52-binary duplicate loop from `test-critical` when called inside `check` (since `rs-test` `--workspace --all-targets --features python` already runs every `critical_*.rs` binary; keep `critical-skip-check` + Python `unittest discover -s python/tests/critical` in `test-critical` and keep `rs-test-critical` standalone); trim bloated multi-paragraph Makefile comments to concise 1–2 line comments.
4. **Retire `MB.26` overflow micro-nodes:** Supersede `MB.26` and all open/proposed `MB.26.*` children via `tools/plan.py`.

# Acceptance
- `make fast-check`
- `make docs-unit workflow-check packaging-gates-test zed-check`
- `uv run --no-project tools/plan.py validate`
