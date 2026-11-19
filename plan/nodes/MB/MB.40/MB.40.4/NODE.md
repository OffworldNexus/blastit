---
id: MB.40.4
title: "Split god-files (scheduler.rs, config.rs, sqlpolicy/analysis.rs, stage0/1) into cohesive modules & serde wire structs"
kind: task
status: review
owner: implementer
priority: 1
depends_on: [MB.40.3]
paths:
  - "crates/blastit/**"
  - "crates/blastit-scheduler/**"
  - "crates/blastit-sqlpolicy/**"
  - Cargo.toml
  - Cargo.lock
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T00:16"
updated: "2026-11-19T03:04"
---

# Goal
Break up the core >1,500-LOC god-files into cohesive submodules (<500–600 prod LOC each), replace `config.rs`'s 16 hand-rolled `json_*` accessors with `#[derive(Serialize, Deserialize)]` structs, and make `sqlpolicy`'s parse cache constructor-injectable.

# Exact Targets (`file:line` pre-diagnosed)
1. **`crates/blastit-scheduler/src/scheduler.rs` (`3,433` prod LOC, single `impl Scheduler` lines `955->5055`):**
   - Split cleanly into `crates/blastit-scheduler/src/scheduler/` (`mod.rs`, `admission.rs`, `request.rs`, `mounts.rs`, `reload.rs`, `learn.rs`) with cohesive helper structs/methods, preserving exact public crate exports and DRR semantics.
2. **`crates/blastit/src/config.rs` (`2,081` prod LOC, `61` free fns, 16 `json_*` accessors):**
   - Split into `crates/blastit/src/config/` (`mod.rs`, `preset.rs`, `parse.rs`, `manifest_io.rs`, `wire.rs`), replacing the 16 hand-rolled `json_string`/`json_u64`/... free functions (`:2772-2959`) with `#[derive(Serialize, Deserialize)]` wire structs via `serde_json`!
3. **`crates/blastit-sqlpolicy/src/analysis.rs` (`1,899` prod LOC, `113` fns, `OnceLock<Mutex<Lru>>` at `:744`):**
   - Split into `crates/blastit-sqlpolicy/src/analysis/` (`mod.rs`, `types.rs`, `classify.rs`, `walk.rs`, `cache.rs`), exposing an explicit `AnalysisCache` struct for constructor injection (keep thin delegating fn only if needed by pure manifest callers, sized explicitly).
4. **`crates/blastit/src/stage0.rs` (`1,601` prod LOC, `58` free fns) & `stage1.rs` (`1,990` prod LOC, `40` free fns):**
   - Split into cohesive submodules (`stage0/{mod,gates,fork_exec,dev}.rs` and `stage1/{mod,adopt,spawn,python_env}.rs`) and group >6-arg functions (`fork_scheduler` 11 args, `fork_http` 8 args, `exec_stage1_dev` 8 args) into cohesive parameter structs.

# Acceptance
- `make fast-check`
- `make rs-no-python rs-no-default`
