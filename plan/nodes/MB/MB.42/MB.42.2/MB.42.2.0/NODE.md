---
id: MB.42.2.0
title: "Pin fork/zygote/GIL ownership invariants in universal-distribution §1.2.1 (design addendum)"
kind: task
status: active
owner: architect
priority: 1
depends_on: []
paths: ["docs/design/**", "docs/decisions/**"]
attempts: 1
max_attempts: 3
escalation: architect
created: "2026-11-19T17:20"
updated: "2026-11-19T17:20"
---

# Goal
Pin the **fork / zygote / GIL ownership invariants** that `docs/design/universal-distribution.md` §1.2 leaves under-specified, as a 10–25 line addendum, so that `MB.42.2.1.3` (stage-0 handoff + `PyForkHooks`) and `MB.42.2.1.4` (stage-1 `dlopen` + supervisor cut-over) implement **one** invariant instead of inventing two.

# Why this exists
`MB.42.2.1` reported `needs_split` with measured facts:
- `crates/blastit` has **59** `Python<'_>`/`pyo3`/`blastit_view_python` sites across 9 files (19 direct `pyo3::` calls), including `supervise.rs` (~3,700 lines, `Python::initialize()` in 4 test modules) and `visibility.rs` (1,899 lines).
- Today the supervisor owns the interpreter **object**, not a descriptor: `supervise.rs:1679` is `zygote::fork_view(py, ViewSpawn { app: &self.app, … })` with `app: Py<PyAny>`, and `supervise.rs:1076` calls `PyCode::compile` directly on reload.
- §1.2 states `serve_view` is the "post-`blastit_sys::child::enter(plan)` child entry" (i.e. the fork happens *outside* the shim), but `LoadedTracker` must be installed *before* the fork, the `Py<PyAny>` zygote must survive it, and `Adoption` must carry `py_fds` across a `dev` re-exec. That ordering is not pinned anywhere.

# Exact Deliverables
1. `docs/design/universal-distribution.md` §1.2 gains a short **`### 1.2.1 Fork, zygote & GIL ownership`** subsection (10–25 lines, terse, no prose padding) that answers, unambiguously:
   - **Who forks.** Exactly which process forks view workers, and whether that fork happens before or after the stage-1 `dlopen` of `libpython` + shim. Name the ordering: `fork(http)` -> `fork(scheduler)` -> `dlopen(libpython)` -> `dlopen(shim)` -> `preload(zygote)` -> per-view `fork` via the `blastit-view` `spawn` seam.
   - **Where the zygote lives.** The `Py<PyAny>` app object, `immortal` state and `LoadedTracker` live **only** inside the shim's opaque `PyZygoteOpaque`; `crates/blastit` holds at most `*mut PyZygoteOpaque` + the three `PyForkHooks` fn pointers, and **no pyo3 type may appear in any `crates/blastit` signature, struct field, or `cfg`**. State how `Supervisor`/`supervise.rs` and `visibility.rs` shed their `Python<'_>` parameters (e.g. a small `PyRuntime` handle of `(zygote_ptr, hooks, vtable)` that is `Copy`).
   - **Where `LoadedTracker` is installed** (inside `preload`, pre-fork, with `report_fd >= 0`) and what happens to it across a `dev` `exec_cycle` re-exec (re-`preload` vs reuse — pick one).
   - **Reload syntax check.** `create_syntax`/`check_syntax` on `dev` reload goes through the vtable (never `PyCode::compile` from `crates/blastit`), and states whether it runs in the parent or a child.
   - **GIL discipline.** Whether every vtable entry takes `PyGILState_Ensure`/`Release` itself, or requires the caller to hold the GIL; and in which order `before_fork` -> `fork` -> `after_fork_parent` / `after_fork_child` run relative to `blastit_sys::child::enter(plan)` in the child.
   - **`Adoption.py_fds`** semantics across `dev` `exec_cycle`: which fds survive, which are re-pinned, and the `(st_dev, st_ino)` re-verify point.
2. `docs/decisions/ADR-0017-universal-distribution.md` Decision 2 gains **one sentence** pointing at `§1.2.1` (the ADR stays 1 page).
3. If (and only if) the answer requires it, one clarifying sentence in §1.1's `blastit-py-sapi` / `blastit-py-shim` crate-boundary bullets (e.g. that the `PyRuntime` handle type lives in `blastit-py-sapi::abi`).

# Constraints
- **Do not redesign** the C-ABI vtable, the discovery order, the handoff, or the packaging — only pin the ownership/ordering question above. If you find a genuine contradiction in §1.2 while answering, fix that one line and say so in `RESULT.md`.
- Keep the design doc under 165 lines total; this is an addendum, not a new chapter.
- `docs/**` only.

# Acceptance
- `docs/design/universal-distribution.md` §1.2.1 present, <= 25 lines, every bullet above answered with a concrete ordering or ownership statement.
- `uv run --no-project tools/plan.py validate`
