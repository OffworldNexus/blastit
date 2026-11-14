# Testability review — `php-fastcgi.md` + ADR-0008 (Round 7)

Lens: **testability**. Critic: goldfish, fresh context. Scope: design `§7.1`, `§7.2`, `§8`
(`MB.3.6.2.2` / `MB.3.6.3.1` / `MB.3.6.4.2`), `§9` Round-6 table, ADR-0008 once, plus
`smoke.rs`/`critical_dev_and_fail_closed.rs` `children()` and the `Makefile` critical gate.

## What discriminates (no finding)

- `external_mode_never_calls_ensure_pool` is a real two-sided oracle: “recording `PoolPort`
  fake with `Arc<AtomicUsize>` + panic-on-call; `external_executor = true`, one request →
  `200` from the scripted fake peer, counter `== 0`; the `false` twin → counter `== 1` and
  the request waits for `PoolReady`” (§7.2 `R9`/`R20`). Counter `0` vs `1`, a fake peer that
  must answer `200`, and panic-on-call make it fail if `ensure_pool` is reached in External
  mode. Owned by `MB.3.6.2.2` (`crates/blastit-scheduler/tests/critical_fastcgi_dispatch.rs`);
  its acceptance `cargo test -p blastit-scheduler --test critical_fastcgi_dispatch --locked`
  is unprivileged and `make rs-test-critical` picks up `crates/*/tests/critical_*.rs`.
- The view-fd oracle states its preconditions and a positive control: “preconditions asserted
  — WSGI response `200`, view pid found via `/proc/<init_pid>/task/<init_pid>/children`, fd
  set non-empty and containing `view.chan` + stdio; the predicate is **inode identity**: the
  inode read from init's `ctl.chan` entry before the request is absent from the view's table,
  while the view's `view.chan` inode **is** present” (§7.2 `R2`/`R3`/`R20`). The `view.chan`
  present assertion is the anti-vacuity control. Owned by `MB.3.6.3.1`
  (`crates/blastit/tests/critical_fastcgi_parent_fd.rs`), acceptance unprivileged.
- No existing protected suite is edited: `critical_fastcgi_dispatch.rs`,
  `critical_fastcgi_parent_fd.rs`, `critical_fastcgi_learn.rs` and `critical_fastcgi_pool.rs`
  are all new files created by their leaves. `MB.3.6.2.2`/`MB.3.6.4.2`/`MB.3.6.3.1` all run
  inside `make check` (`rs-test` + `rs-test-critical`), so no MAJOR gate is tripped.

## Findings

### MINOR-T1 — `external_pool_never_spawns_php_fpm` is absence-only and can pass vacuously

§7.2 `R17`/`R18`: “**`external_pool_never_spawns_php_fpm`** — after one request to the
FastCGI route under `dev`, `/proc/<init_pid>/task/<init_pid>/children` contains no pid whose
`/proc/<pid>/comm` is `php-fpm` (the `children()` helper in `smoke.rs`).” ADR-0008 restates
it as “no `php-fpm` child of the `dev` init after a request”. Three gaps:

1. **No request-success precondition.** The sibling oracle explicitly requires “one request →
   `200` from the scripted fake peer”; this one only says “after one request”. If the fixture
   never binds a FastCGI peer at `listen`, the request is `503` and the absence assertion
   holds trivially, so a conditional spawn (only when serving) is never exercised.
2. **Direct-children-only scan.** `smoke.rs::children()` reads only
   `/proc/<pid>/task/<pid>/children` and documents “init is single-threaded, so its main
   thread's entry has them all.” A spawn from a tokio worker thread, or a
   `forking::fork(true)` double-fork that reparents the `php-fpm` master, is invisible to
   that path.
3. **No positive control.** Nothing proves the scanner can see a `php-fpm` child at all; a
   wrong path, a `comm` mismatch, or the single-thread assumption silently degrades the
   oracle to a no-op.

**Fix:** state the fixture binds a `UnixListener` (or fake `php-fpm` helper) at `listen` and
asserts the request returns `200`; add a positive twin that spawns a `php-fpm`-named child
(or runs the managed-mode path) and asserts the scanner finds it; scan descendants
recursively (`/proc/*/stat` ppid walk) and assert `/proc/<init_pid>/task` has exactly one
task before scanning. Grade stays MINOR because the counter oracle already fails on the
primary regression; this one is the belt-and-braces check.

### MINOR-T2 — §7.2's view-oracle predicate omits the pinned-parent inode half

§7.1 says the oracle proves “`/proc/<view_pid>/fd` holds neither the pinned parent inode nor
init's `ctl.chan` inode”, and §8 names it
`view_child_holds_neither_parentfd_nor_ctl_chan`. But the authoritative §7.2 predicate
sentence only names “the inode read from init's `ctl.chan` entry … absent … while the view's
`view.chan` inode **is** present”. The pinned parent directory's `(st_dev, st_ino)` (read
from init's `/proc/<init_pid>/fd` entry for `parentfd`) is never asserted absent, so a
regression that leaks `parentfd` into the view passes the stated oracle.

**Fix:** append “and the pinned parent directory's `(st_dev, st_ino)`, read from init's
`parentfd` entry before the request, is absent from the view's table” to the §7.2 predicate.

### MINOR-T3 — §7.2 `R2`/`R3`/`R20` Suite column omits `critical_fastcgi_parent_fd.rs`

The row's Suite column is “`critical_fastcgi_pool.rs` + `e2e/rootless/run.py`”, yet the view
oracle it describes lives in `critical_fastcgi_parent_fd.rs` (`MB.3.6.3.1` per §7.1 and §8).
§7.1 and §8 agree with each other; only the §7.2 owner column disagrees.

**Fix:** add `critical_fastcgi_parent_fd.rs` to the Suite column, or split the view oracle
into its own row whose Suite column names it.

### MINOR-T4 — “reuses `smoke.rs`” helpers are not reachable from the owning leaves' `paths`

§7.2 says the `dev` oracle uses “the `children()` helper in `smoke.rs`”, and §9 MINOR-T5
says the view oracle “reuses `smoke.rs`'s `Fixture`/`fds()`/`children()`”. But `MB.3.6.4.2`
paths are `crates/blastit/src/{cli,check}.rs` + `crates/blastit/tests/critical_fastcgi_learn.rs`,
and `MB.3.6.3.1` paths are `crates/blastit/src/{stage0,stage1,config,cli,dev_learn}.rs` +
`crates/blastit/tests/critical_fastcgi_parent_fd.rs`; neither includes `smoke.rs` or a shared
support module. The existing `critical_dev_and_fail_closed.rs` already duplicates
`children()` rather than sharing it, so “reuses” is not a mechanism the implementer can apply
without a path edit.

**Fix:** add a shared `crates/blastit/tests/support/proc.rs` (or `tests/common/`) to both
leaves' `paths` and move `Fixture`/`fds()`/`children()` there, or change the prose to “copies
the 15-line helper from `smoke.rs`”.

## Accepted follow-ups (MINORs)

- T1: add a `200` fake-peer precondition, a positive control, and a recursive descendant scan
  to `external_pool_never_spawns_php_fpm`.
- T2: name the pinned-parent inode assertion in the §7.2 view-oracle predicate.
- T3: add `critical_fastcgi_parent_fd.rs` to the §7.2 `R2`/`R3`/`R20` Suite column.
- T4: add a shared test-support module to the leaf `paths`, or say “copies” instead of
  “reuses”.

Counts: BLOCKER 0 · MAJOR 0 · MINOR 4.

VERDICT: PASS
