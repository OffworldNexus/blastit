---
id: MB.36.1
title: "Design the view-crate graph and write ADR-0016: blastit-view transport, blastit-view-{python,php,static}, a thin blastit, and the exact file-by-file module boundaries"
kind: task
status: done
owner: designer
priority: 1
depends_on: []
paths:
  - "docs/design/view-crates-architecture*.md"
  - "docs/decisions/ADR-0016-view-crates-over-wire.md"
  - "docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md"
  - docs/decisions/README.md
attempts: 2
max_attempts: 3
escalation: architect
created: "2026-11-17T14:07"
updated: "2026-11-17T15:56"
---

# Goal

Write the architecture of record for the crate split, **before any file moves**:
`docs/design/view-crates-architecture.md` and `ADR-0016-view-crates-over-wire.md`.

The design fixes, file by file and crate by crate, which module ends up where, which crate
may depend on which, what each Cargo feature compiles, and how every protected test is
relocated. It supersedes `ADR-0009`'s `WorkerRuntime`/`Upstream` **trait** requirement while
keeping its security, pinning, sandbox and distribution invariants intact.

# Context

- Owner mandate is quoted in `MB.36`; this node is the architect's answer to it. It must be
  schedulable on its own: **`depends_on: []`**, so it runs in parallel with `MB.33` and
  `MB.23.5.2` and touches no crate.
- Read first: `docs/VISION.md`, `ARCHITECTURE.md` §2.5/§2.6/§3.1/§4/§7.3/§9.3,
  `ADR-0009` (decisions 1, 2, 3, 5), `ADR-0010` §1/§3/§4, `ADR-0014` §4.4,
  `docs/design/backend-runtimes.md` §3, `docs/design/parallel-views.md`, and
  `docs/knowledge/LESSONS.md` (PyO3/`libpython`, feature unification, `PYTHONHOME`).

## The graph the design must land (`MB.36.1` decides the details, not the shape)

| Crate | Owns | Process | Compiled when |
|---|---|---|---|
| `blastit-frame` | raw frames, `ReqHeadView`, `Limits` | — | always |
| `blastit-view` | `wire.rs`, `flow.rs`, `loaded.rs`, `error.rs` (+ the shared view config/sync helpers) — **transport primitives only** | — | always |
| `blastit-view-python` | `environ`, `input`, `request`, `response`, the `serve` worker pool, `immortal` preload, `zygote` fork | view | `blastit` feature `python` |
| `blastit-view-php` | the `view.chan` worker loop, `blastit-php-sapi` integration, `libphp` static-ELF discovery, `(st_dev, st_ino)` library + entry pinning, the probe child | view | `blastit` feature `php` |
| `blastit-view-static` | `openat2` root resolution, MIME/`ETag`/`immutable`/SPA-answer responder | **none** (library, in the scheduler) | `blastit` feature `static` |
| `blastit` | CLI, stage 0, stage 1, supervisor, role wiring | init/sched/http/dbproxy | always; `default = ["php", "static"]`; wheel adds `python` |

`crates/blastit-view` must end with **zero `pyo3`** (including `[dev-dependencies]`), **zero
`blastit-php-sapi`**, and **no `runtime.rs`** — the trait plus its `OutputStream`,
`RequestIo`, `RequestEnd`, `OUTPUT_LINE_CAP` and Python `LoadedTracker` are split: transport
types to `flow.rs`/`wire.rs`, the tracker to `blastit-view-python`, the trait deleted.

## Decisions the design MUST record (each with the alternative it rejects)

1. **The child sequence's home.** `crates/blastit/src/child.rs` (`ChildIo`,
   `install_filter`, `child_exit`, `Deadline`) is used by the CPython fork and the PHP fork.
   If `zygote`/`php_view` move into view crates and `child` stays in `blastit`, both crates
   depend on `blastit` — a cycle. Rule out options explicitly (move into `blastit-sys` beside
   the sequence it implements; a non-language `blastit-view` module; leaving the forks in
   `blastit`) and pick one, preserving the exact order
   `ChildIo::prepare → drop_privileges → harden_child → install_filter → set_dumpable(false)
   → saved signal mask → runtime load → READY`.
2. **The spawn-context seam.** `zygote.rs` takes `RouteInfo` + `Config` (6306-line `stage0`
   types) and `immortal.rs` takes `config::Immortalize`. Define the minimal struct(s) the view
   crates accept instead, field by field, and where they live. `blastit-view-python` must not
   depend on `blastit`; decide whether it may depend on `blastit-scheduler` (`RouteInfo`) or
   whether the spawn side gets a narrower type.
3. **One percent-decoder.** `static_asset.rs` uses `crate::path` (`percent_decode`,
   `path_of_target`) and `rewrite.rs` uses it too. If static moves out and path stays in the
   scheduler, the scheduler depends on a crate that depends on the scheduler. Say exactly
   which crate owns the single decoder and prove there is no second one
   (`grep -rn 'percent_decode' crates/`).
4. **`blastit-view-static` is not a process.** It has no `view.chan`, no fork and no seccomp
   role: it is the static *view kind* as a library the scheduler calls. Record the dependency
   direction and why the alternative (a static worker process) is worse.
5. **PHP discovery's split.** `stage0.rs`'s `php_boot_gate` / `PhpSearch` /
   `LIBPHP_PROBES` / `discover_php` / `resolve` / `version_of` run in **init** (root), while
   `PhpEmbedRuntime`, `boot_bindings`, `pin_entry`, `probe_child` and the worker loop run in
   the **view**. Decide the module boundary inside `blastit-view-php` (a `discovery` module
   callable from init, a `worker` module for the child), so `blastit` links that crate for
   stage 0 without dragging a worker or `dlopen` into init's symbol reach.
6. **Feature wiring, exactly.** `blastit`'s `default = ["php", "static"]` (standalone) and
   `blastit-py`'s `features = ["python"]` → wheel `["python", "php", "static"]`. State the
   per-artifact build command (`-p blastit`, never `--workspace`) and the guard that catches
   a regression (`make rs-no-python`, `scripts/check-portable-elf.sh`). Note that
   `blastit-view` currently defaults `python = true`; that default must go.
7. **The critical-test relocation table.** One row per file that changes home, with source,
   destination and the `paths` glob that legalises the move:
   `critical_static_assets.rs`, `critical_static_resolve.rs`, `critical_static_entry.rs`
   (scheduler → `blastit-view-static`), `critical_static_cli.rs` (stays in `blastit`),
   `critical_php_backend.rs`, `critical_php_embed.rs`, `critical_parallel_views.rs` (decide),
   `critical_loaded_files.rs`, `critical_dev_reload.rs`, and the non-protected
   `crates/blastit-view/tests/{serve,environ,body,flow,concurrency,gil,php_native}.rs`.
   No case may be dropped; `tools/critical_guard.py` requires the old glob in `paths`.

## DX invariance, written down

This epic changes no CLI, no manifest key and no documented command. The design therefore
carries a short **DX-invariance list** — `blastit dev`, `blastit dev --no-reload`,
`blastit check`, a static-only manifest, a `[route.php] entry`, a WSGI `app`, and the three
documented environments (local zero-`sudo`, CI completeness, deployment presets) — that
`MB.36.7` replays verbatim. No new user page is needed; a page that says "the internals moved"
would be the bloat `dx-first` forbids.

# Acceptance

```console
$ test -f docs/design/view-crates-architecture.md && echo "design exists"
$ test -f docs/decisions/ADR-0016-view-crates-over-wire.md && echo "ADR exists"
$ grep -q 'Superseded in part by ADR-0016' docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md
$ grep -q 'ADR-0016' docs/decisions/README.md && echo "ADR indexed"
$ for l in security implementability testability consistency; do test -f docs/design/view-crates-architecture.review-$l.md; done
$ grep -q 'blastit-view-python' docs/design/view-crates-architecture.md
$ grep -q 'blastit-view-php' docs/design/view-crates-architecture.md
$ grep -q 'blastit-view-static' docs/design/view-crates-architecture.md
$ grep -q 'critical_static_resolve' docs/design/view-crates-architecture.md && echo "relocation table present"
$ uv run --no-project tools/plan.py validate
```
- Each of the four `review-<lens>.md` files is a fresh-context `critic` review ending in
  `VERDICT: PASS` (`security` must explicitly confirm every §Context invariant is preserved;
  `implementability` must confirm the child-sequence/cycle/seam answers are buildable;
  `testability` must confirm no protected case disappears; `consistency` must confirm
  `ADR-0009`/`ADR-0010`/`ADR-0014` are amended rather than contradicted).
- The design contains the crate graph table, a file-by-file module map with line counts and
  destination, the feature matrix, the seam decisions with rejected alternatives, and the
  critical-test relocation table.
- No file outside `docs/design/view-crates-architecture*`, `docs/decisions/ADR-0016-*`,
  `docs/decisions/ADR-0009-*` and `docs/decisions/README.md` changes in this node.

# Notes

- `ADR-0016` is the next free number (0015 is `MB.24`'s multi-worker views). One page, four
  sections: Context, Decision (the six items), Rejected alternatives, Consequences.
- Amend `ADR-0009`'s header status only — do not rewrite its decisions; the ADR chain is how
  a later reader learns *why* the traits were real once and stopped being real.
- The critic reviews are `goldfish`: they judge the file on disk, not this brief.
