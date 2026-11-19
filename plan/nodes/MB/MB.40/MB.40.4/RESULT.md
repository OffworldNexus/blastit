# MB.40.4 — Split god-files into cohesive modules

`STATUS: needs_split` — file 1 of 4 is complete and verified; the tree is green at
this boundary. Files 2–4 were not started rather than half-split.

## Per-file status

| # | File | Before (lines / prod) | After | State |
|---|------|----------------------|-------|-------|
| 1 | `crates/blastit-scheduler/src/scheduler.rs` | 5,650 / ~3,433 | 9 modules, all < 610 prod LOC | **done, verified** |
| 2 | `crates/blastit/src/config.rs` | 4,463 / ~2,081 | — | **not started** (`needs_split`) |
| 3 | `crates/blastit-sqlpolicy/src/analysis.rs` | 2,749 / ~1,899 | — | **not started** (`needs_split`) |
| 4 | `crates/blastit/src/stage0.rs` + `stage1.rs` | 5,043 + 3,003 | — | **not started** (`needs_split`) |

## 1. `blastit-scheduler` — done

### Module layout (`src/scheduler/`)

`lib.rs` is **unchanged**: `mod scheduler;` now resolves to `scheduler/mod.rs`,
and `pub use crate::scheduler::{Emit, run};` re-exports exactly what it did.

| Module | Contents | Lines | Prod* |
|---|---|---|---|
| `mod.rs` | module docs, imports, peer-queue constants, `Emit`/`HttpIn`/`ViewKey`/`ViewIn`/`CtlIn`/`StaticRead`, `Scheduler` state, `new`/`start`/`pump`/`on_http`/`on_view`/`on_ctl`/push helpers, module wiring | 836 | 482 |
| `admission.rs` | `View`, `ViewSet`, `Admission` (+ fair queue delegation), admission gates, per-process sizing, `dispatch_queued{,_fair}`, `dispatch`, `reserve`, `view_out` | 872 | 542 |
| `request.rs` | `RequestHead`/`Request`, `begin_raw` (head-before-body), `resolve`, `enqueue`, `request_body`/`request_end`, cancel/window/credit, header helpers | 881 | 603 |
| `mounts.rs` | `Resolution`/`MountHead`/`StaticHead`, `static_head`, `plan_request_body`, `dispatch_static`, `mount_missed`/`mount_learn_or_404`, `serve_static`/`forward_static`/`pump_static`/`on_static_read`/`finish_static`/`abort_static`, static-off stubs | 766 | 491 |
| `reload.rs` | `MemSample`, `expire_queue`/`expire_requests`, `retire_finished_views`, memory watermark, derived view cap, `grow`/`make_room`/`evictable_view`/`retire`, `reload` | 761 | 520 |
| `response.rs` | `respond{,_with}`, `reject`/`refuse_body`/`fail_request`, `release`, `charge`/`charge_ns`, `emit_request`/`emit_retired`, `view_done`/head/body/end/abort/window, outcome helpers, unit tests | 1,138 | 688 (558 prod + 130 test lines) |
| `views.rs` | `view_spawned`/`view_ready`/`view_exited`/`view_retiring`/`expire_exited_views`/`view_gone` | 276 | 186 |
| `run.rs` | `sleep_until`, `ctl_task`, `pub async fn run` (re-exported from `mod.rs`) | 181 | 124 |
| `learn.rs` | `learn_route`, `pump_learn_fallback` (`--learn` single fallback slot) | 64 | 45 |

\* prod = non-blank, non-comment lines, so tests are counted; `response.rs` is the
only module holding tests (they test `Charge`/`charge_ns`/`completed_outcome`,
which live in that module).

The six node-named modules are present (`mod`, `admission`, `request`, `mounts`,
`reload`, `learn`). Three cohesive sub-modules were added because the single
`impl Scheduler` is 4,097 lines and 6 files would each exceed 600 prod LOC:
`response.rs` (responses/charging/events), `views.rs` (view-process lifecycle,
split from `reload.rs`'s supervision/sizing), `run.rs` (the loop/`run` entry).

### Mechanics (behaviour-preserving)

- Pure line-range moves out of one pristine copy; the only text transformations
  were `pub(super)`/`pub(crate)` visibility prefixes required because an
  inherent `impl Scheduler` is visible only in the module that writes it, and its
  field/type declarations now live in `mod.rs`. No statement, match arm, numeric
  constant, error string or doc paragraph was edited.
- DRR/fair-queue maths, `Admission::remove` funnel, credit accounting, charge
  clamp (`R19`), view lifecycle and every `SchedulerError` sentence are byte
  identical.
- No new globals: the split introduced zero `static`/`OnceLock`; the only
  `static`-like items are the pre-existing `const`s, now in `mod.rs`.
- No new traits, no `unwrap()`/`expect()` on untrusted input (`grep -n
  "unwrap()" src/scheduler/*.rs` → no matches; only `unwrap_or*` fallbacks).

### Public API unchanged

- `git status --porcelain` → only `D src/scheduler.rs` + `?? src/scheduler/`.
  `lib.rs`, `crates/blastit/**`, `Cargo.toml`, `Cargo.lock` untouched.
- `pub use crate::scheduler::{Emit, run};` still resolves: the `lib.rs` doc test
  (`use blastit_scheduler::{..., run, routes_from_manifest}`) compiles and runs.
- All 15 integration/unit test binaries in the crate (including the protected
  `critical_*.rs` and `static_feature_off.rs`) build against the unchanged crate
  API and pass unmodified.
- `Emit` stays `pub type Emit = Arc<dyn Fn(Event) + Send + Sync>`; `run` keeps
  its exact `pub async fn run<C: CtlChannel + Send + 'static>(...)` signature.

### Tails

```
$ cargo test -p blastit-scheduler --locked
running 45 tests ... test result: ok. 45 passed; 0 failed
running 11 tests ... test result: ok. 11 passed; 0 failed
running 10 tests ... test result: ok. 10 passed; 0 failed
running 7 tests  ... test result: ok. 7 passed; 0 failed
running 16 tests ... test result: ok. 16 passed; 0 failed
running 32 tests ... test result: ok. 32 passed; 0 failed
running 9 tests  ... test result: ok. 9 passed; 0 failed
running 17 tests ... test result: ok. 17 passed; 0 failed
running 6 tests  ... test result: ok. 6 passed; 0 failed
running 2 tests  ... test result: ok. 2 passed; 0 failed
Doc-tests blastit_scheduler: test result: ok. 1 passed; 0 failed
passed 239 failed 0

$ cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.31s
```

## 2–4. Remaining work (`needs_split`)

### `crates/blastit/src/config.rs` (4,463 lines)
Split into `config/{mod,preset,parse,manifest_io,wire}.rs` and replace the 16
`json_*` free functions (`field`, `json_string`, `json_opt_string`,
`json_string_list`, `json_optional_string_list`, `json_string_lists`, `json_u64`,
`json_u32`, `json_u8`, `json_opt_u32`, `json_usize`, `json_i32`, `json_bool`,
`json_optional_bool`, `json_duration`, `optional_path`, plus
`php_runtime_from_json`/`preset_from_json`) with `#[derive(Serialize,
Deserialize)]` wire structs consumed by `Config::from_json` at `:1951-2060`.
**Warning for the next implementer:** this `Value` is the stage-0 re-exec
contract (`--config-fd`), so the wire structs' field names, optionality and
number ranges must reproduce the hand-written accessors exactly (the accessors
produce `ConfigError::Document { field, reason }` wording that tests assert).

### `crates/blastit-sqlpolicy/src/analysis.rs` (2,749 lines)
Split into `analysis/{mod,types,classify,walk,cache}.rs`. `src/analysis/tests.rs`
already exists, so `mod tests;` keeps resolving after the move.
**Constraint discovered:** `the_cache_stays_bounded` (`src/analysis/tests.rs:838`)
pins the current semantics — `analyze(sql)` must populate a cache that
`cached_analyses()` reports and that stays `<= CACHE_CAPACITY`. The
`OnceLock<Mutex<Lru>>` at `:743` can therefore only be removed by giving
`Allowlist`/`check` (`lib.rs:274`) an injected `AnalysisCache` **and** keeping the
public `analyze`/`cached_analyses` pair behaviourally identical; a pure
`AnalysisCache`-only rewrite fails that test unmodified. `pitfalls`: the
`Resolver` impl alone is ~1,000 lines (`:1263-2332`), so `walk.rs` needs a second
file (e.g. `walk/` or `resolve.rs`) to stay under 600 prod LOC.

### `crates/blastit/src/stage0.rs` (5,043) & `stage1.rs` (3,003)
`stage0/{mod,gates,fork_exec,dev}.rs` and `stage1/{mod,adopt,spawn,python_env}.rs`,
grouping `fork_scheduler` (11 args), `fork_http` (8) and `exec_stage1_dev` (8)
into parameter structs. These are process-plane files: behaviour-preserving
splitting needs the fd/`OwnedFd` ownership order kept literal.

## Acceptance gates

Both node gates pass at this boundary.

```
$ make fast-check
   ... Doc-tests blastit_view_static ... ok
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
67 files already formatted
uv run --no-project tools/plan.py validate
plan OK
FAST_CHECK_EXIT=0
```

```
$ make rs-no-python rs-no-default
running 2 tests
test a_manifest_with_a_mount_is_refused_before_any_channel_is_served ... ok
test the_same_boot_without_a_mount_is_not_refused ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.30s
RS_GATES_EXIT=0
```

Feature-matrix check for the split (the crate builds with and without `static`;
only the `static`-gated types were split across modules, so two imports had to
carry `#[cfg(feature = "static")]`):

```
$ cargo check -p blastit-scheduler --no-default-features --locked
   Finished `dev` profile ... (no warnings)
$ cargo clippy -p blastit-scheduler --no-default-features --locked -- -D warnings
   Finished `dev` profile ... (no warnings)
$ cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings
   Finished `dev` profile ... (no warnings)
```

## Follow-ups

- `plan/BUDGET.md`, `plan/JOURNAL.md`, `plan/nodes/.../LOG.md` and `NODE.md` were
  already modified in the worktree when this slice started; not touched here.
- `--all-targets --no-default-features` still cannot compile
  `tests/critical_rewrite.rs` (it imports `blastit_scheduler::rewrite`, which
  only exists with the `static` feature). Pre-existing, and outside this node's
  targets; `make rs-no-default` does not build that combination.
- The three remaining god-files (`config.rs`, `sqlpolicy/analysis.rs`,
  `stage0.rs`/`stage1.rs`) are untouched — see their sections above for the exact
  layout expected next and the two constraints found while reading them.

## File-1 review round (both reviewers PASSED; two MINORs closed)

**1. Visibility narrowed to the smallest thing that works.** The split had
promoted 18 type declarations to `pub(crate)` because an inherent `impl
Scheduler` is only visible in the module that writes it. That was wider than
needed: from inside `scheduler/`, `pub(super)` resolves to the `scheduler`
module and is inherited by its submodules, and a plain private item of
`mod.rs` is already visible to its descendants. Now:

- `mod.rs` (the `scheduler` module itself): `Scheduler`, `StaticRead`,
  `StaticReadMsg`, `HttpIn`, `ViewKey`, `ViewIn`, `CtlIn` carry **no** visibility
  qualifier (exactly the base state) and `Scheduler`/`StaticRead` fields carry
  none either.
- The types defined in submodules and used by sibling modules are `pub(super)`
  (`pub(in crate::scheduler)`): `View`, `ViewSet`, `Admission`, `Request`,
  `RequestHead`, `Resolution`, `MountHead`, `StaticHead`, `MemSample`, `Charge`.
- `grep -rn "pub(crate)" src/scheduler/` → **0** matches.
- The only `pub` items in `src/scheduler/` are `pub type Emit`, `pub async fn
  run` and `pub use run::run;` — `lib.rs` still names exactly `{Emit, run}`.

**2. `slot_releasable` moved to the module whose slot it governs.** It (with its
four-row test `a_static_slot_is_releasable_only_when_its_read_is_not_parked`,
byte-identical assertions and doc) now lives in `mounts.rs` immediately after
the `Scheduler::release_static_slot` impl block, as a private `fn`; its only
caller is `mounts.rs:664`. `mounts.rs` no longer imports static slot policy back
from `response.rs` (`use super::response::canned;` only). `response.rs`'s test
module lost the pair and now imports `{Charge, a_done_may_relabel_a_completed_5xx,
answered_a_bailout, charge_ns, completed_outcome}`.

Nothing else was touched: no statement, doc paragraph, import ordering
(`cargo fmt --all -- --check` is clean without reformatting) or test changed.
Net LOC shift: `mounts.rs` +31, `response.rs` −34 (both stay well under 600 prod
LOC: 522 and 524).

Verification after the round:

```
$ cargo test -p blastit-scheduler --locked
passed 239 failed 0

$ cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings
CLIPPY_DEF=0            (no diagnostics)

$ cargo clippy -p blastit-scheduler --no-default-features --locked -- -D warnings
CLIPPY_NOFEAT=0         (no diagnostics)

$ cargo fmt --all -- --check
FMT=0
```
