# RESULT — MB.36.4

Status: **done** (every acceptance command below was run in this worktree)

Move of the CPython preload and the view fork out of `blastit` into
`blastit-view-python`, behind the design's `SpawnContext`/`Forker` seam, with the
composition root keeping only what it owns (`forking.rs`, the `Config →
SpawnContext` mapping, the `py` token).

## What moved, and what stayed

| Before | After |
|---|---|
| `crates/blastit/src/immortal.rs` (1400 lines, 17 unit cases) | `crates/blastit-view-python/src/immortal.rs` (**17 unit cases**, same assertions), with a local `Setting { Auto, On, Off }`; `Config::Immortalize` maps into it in `crates/blastit/src/config.rs` |
| `crates/blastit/src/zygote.rs` (282) | `crates/blastit-view-python/src/zygote.rs`, taking `SpawnContext` + `Forker`; `unsafe { UnixStream::from_raw_fd(..) }` replaced by the safe `UnixStream::from(OwnedFd)` |
| `crates/blastit/src/child.rs` (297) | `crates/blastit-sys/src/child.rs` **plus** `Privileges`, `Filter`, `ChildPlan`, `enter`, `set_process_group`, `set_env` and the `open(O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC)` + `fchdir` change; `crates/blastit/src/child.rs` is a 19-line shim (`pub(crate) use blastit_sys::child::*;`), so every `crate::child::…` call site compiles untouched |
| — | `crates/blastit-view/src/spawn.rs`: `SpawnContext`, `ForkProtocol`, `Forker` (1 unit case) |
| — | `crates/blastit-sys/tests/critical_child_sequence.rs`: **5 unprivileged cases** |
| `crates/blastit/src/forking.rs` (85) | unchanged text **plus** `fork_for(protocol, interpreter_live)`, its mismatch refusal and its own unit cases |
| `crates/blastit/src/py.rs` | code unchanged; the docs now say it is the composition root's token and that `blastit-view-python` re-exports the interpreter's own `Python` |

`cargo tree -p blastit-view-python -e normal` contains no `blastit` package, and
neither does `blastit-view`'s: the arrow is one-way. `Config → SpawnContext` is
mapped at the call site in `blastit` (`Supervisor::fork_view`), and
`Immortalize → Setting` in `config.rs`.

## The D1 pin: the boot mode is a tested decision

`view_child_plans(routes, egress, dev, seccomp)` in `crates/blastit/src/stage1.rs`
is the one place the boot mode becomes a view child's identity and filter, and
its `child_plans` cases pin the three rows the design names:

| case | boot | assertion |
|---|---|---|
| `a_production_boot_drops_each_view_to_its_own_uid_and_filters_it` | `dev: None` | two fixture routes with **different** uids (`20_001`, `20_002`) → `DropTo(20_001)`, `DropTo(20_002)` **and** `Seccomp(View { egress: false })`, `Seccomp(View { egress: true })` — one identity and one reach per route, in routing order |
| `a_dev_boot_shares_the_uid_and_still_filters` | `dev: Some(..)` | `[Shared, Shared]` + the same per-route `Seccomp` (dev is filtered today) |
| `an_explicit_off_switch_installs_no_filter_but_keeps_the_identity` | `seccomp: false` | `[DropTo(..), DropTo(..)]` + `[Off, Off]` — `BLASTIT_SECCOMP=0` is not an identity switch |

The module is `#[cfg(test)]` **without** a feature gate and sits at the end of
`stage1.rs`, so the cases run in `cargo test -p blastit --lib --locked` (the
standalone configuration) and therefore in `make check`; they are 3 of the 249
lib cases there (266 with `--features python`).

**Proven to fail.** Inverting the branch (`if dev.is_none()` → `if dev.is_some()`,
the exact mutation the review named):

```console
$ cargo test -p blastit --lib --locked child_plans      # mutated
test stage1::child_plans::a_production_boot_drops_each_view_to_its_own_uid_and_filters_it ... FAILED
test stage1::child_plans::an_explicit_off_switch_installs_no_filter_but_keeps_the_identity ... FAILED
test stage1::child_plans::a_dev_boot_shares_the_uid_and_still_filters ... FAILED
assertion `left == right` failed: each route drops to its own uid, in routing order
dev shares the caller's uid, whatever the layout says: [DropTo(20001), DropTo(20002)]
test result: FAILED. 0 passed; 3 failed; … finished in 0.00s

$ cp /tmp/opencode/stage1.final2.keep crates/blastit/src/stage1.rs
$ cmp -s crates/blastit/src/stage1.rs /tmp/opencode/stage1.final2.keep && echo RESTORED-BYTE-IDENTICAL
RESTORED-BYTE-IDENTICAL
$ cargo test -p blastit --lib --locked child_plans        # restored
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 247 filtered out; finished in 0.00s
```

The restore is a `cp` from a backup taken before the mutation, never a
`git checkout`; `cmp` proves the bytes are back.

The rows keep the census clean: `Privileges` and `Filter` have two variants
each, so row 2 asserts "no route got a per-route drop" and row 3 "no route kept
a `Seccomp`", and the literal `Privileges::Shared` / `Filter::Off` spellings
exist **only** inside `view_child_plans` — `grep -rn -e 'Privileges::Shared' -e
'Filter::Off' crates/blastit/src crates/blastit-view*/src` returns exactly those
two lines.

## Doc corrections (review round 1), each verified

* **A symlinked working directory is refused with `ENOTDIR`, not `ELOOP`.**
  `child.rs`'s `ChildIo::prepare` doc now says so and states why (the
  `O_DIRECTORY` check fires before the no-follow check); the critical suite's
  case 4 accepts both spellings and names the reason. Witnessed: the child's
  stderr reads `test child: Not a directory (os error 20)`.
* **`enter` returns on success.** It returns `Ok(())` and the caller's next step
  is its own — for the WSGI fork, `set_view_environment` then `serve_reporting`
  — while `Err` is the one sentence the caller passes to `child_exit`. The doc
  said "never returns", which was false; fixed, together with a "step 0 is the
  caller's" paragraph.
* **Only the WSGI fork calls `enter` today.** `child.rs`'s module doc,
  `blastit-sys/src/lib.rs`'s sequence paragraph and `blastit/src/child.rs`'s shim
  doc now say "the WSGI fork today; the two PHP callers land in `MB.36.5`"
  instead of describing code that does not exist yet.
* **`php_view.rs`'s remaining sites are named exactly** (see follow-up 1):
  `own_process_group` `:875`, its parent half `join_group` `:888` (not
  `adopt_process_group`, and not `:884`), `set_view_environment` `:1275`, and the
  two dead doc links at `:10` (`crate::zygote`) and `:1752` (`crate::immortal`).

## The security order preserved

`blastit_sys::child::enter` runs steps 1–7 **in this order**, and nothing after
them:

```text
ChildIo::prepare            dup stdio, close_fds_except, rlimits, umask,
                            open(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC) + fchdir
setpgid(0, 0)               when ChildPlan::pgroup     <- BEFORE the filter
drop_privileges(uid)        when Privileges::DropTo
harden_child(SIGKILL)       after the drop (setresuid clears PDEATHSIG)
install(role)               when Filter::Seccomp
set_dumpable(false)         best effort, error discarded on purpose
restore_signal_mask_in_child(mask)   when Some
```

* **Where it is asserted:** `crates/blastit-sys/tests/critical_child_sequence.rs`,
  `the_child_is_a_group_leader_before_the_embedded_view_filter` (case 3) plans
  `pgroup: true` **and** `Filter::Seccomp(EmbeddedView)`, then requires
  `setpgid` to be refused *after* `enter` and `getpid() == getpgid(0)`. Proven
  to fail on a reorder: with `setpgid` moved after `install`, the case fails with
  `test probe: Operation not permitted (os error 1)`.
* The GIL protocol is untouched: `forking::fork` still runs
  `PyOS_BeforeFork` / `PyOS_AfterFork_Parent` / `PyOS_AfterFork_Child` with the
  GIL held, and `fork_for` is now the only path a view crate can reach it
  through (`ForkProtocol::InterpreterLive`, refused when the image disagrees,
  and refused at all in a `python`-less build). `fork_view` calls the injected
  forker as its last act before `enter`, and
  `blastit_sys::child::enter(plan)` is the child's first act (step 0: nothing
  loads, opens or mutates the environment before it).
* `own_process_group` was **not** copied: `enter` owns `setpgid`, and the WSGI
  fork passes `pgroup: false` (supervision kills by pid). The copy still in
  `crates/blastit/src/php_view.rs` belongs to `MB.36.5` — see `## Follow-ups`.

## Recorded deviations (all measured on this branch)

1. **Case 3 uses `Privileges::Shared`, not the design's `DropTo(euid)`.** An
   unprivileged caller cannot perform a drop at all: `drop_privileges` starts
   with `setgroups(0, NULL)`, which needs `CAP_SETGID` (measured here: `CapPrm:
   0000000000000000`, `os.setgroups([])` → `Operation not permitted`). With
   `DropTo(euid)` `enter` returns the drop's error and case 3 would assert the
   drop instead of the order. The drop has its own case (5).
2. **Case 5 asserts the drop step, not the literal `setresuid` string.** That
   sentence presumes `CAP_SETGID`; an unprivileged caller fails one step earlier
   (`setgroups(0, NULL)`). The case asserts the crate's own prefix
   (``dropping privileges failed at ` ``) and that the child's line names the
   plan; under root it plans `DropTo(0)` and asserts `refusing to drop
   privileges to uid 0` — the only drop root may not perform, since root may
   `setresuid` to any uid.
3. **The census count of `Privileges::Shared` is one, not two.**
   `grep -rn -e 'Privileges::Shared' -e 'Filter::Off' crates/blastit/src
   crates/blastit-view/src crates/blastit-view-python/src
   crates/blastit-view-static/src` returns exactly `stage1.rs`'s
   `view_child_plans` — one `Privileges::Shared` in stage 1's **dev branch
   only**, one `Filter::Off` — and nothing in a view crate. `blastit dev` is the
   only non-production boot that reaches stage 1: `check` never boots it and
   forks no view, so it constructs neither (`MB.36.6`'s log says the same).
   `blastit-sys::child::ChildPlan::for_test` is the documented fourth spelling
   and is outside the census. The decision is a named, pure function
   (`view_child_plans`) with its own cases, so "one branch" is also "one pinned
   branch".
4. **`fchdir`'s refusal is `ENOTDIR`, not `ELOOP`.** Linux answers
   `O_DIRECTORY|O_NOFOLLOW` on a symlink with `ENOTDIR` (the directory check
   fires first); the test accepts both and says why.
5. **`set_env` is safe because it *enforces* its precondition, not because a
   comment says so.** A safe `pub fn` over `setenv(2)` is only sound when no
   other thread can be inside `getenv`; the function therefore reads
   `/proc/self/status` (the same `thread_count()` `blastit_sys::fork` uses)
   and refuses a process with more than one thread, with its own sentence. It is
   the same **advisory** check `fork` makes — a thread born between the read and
   the write is not caught — and it refuses when the count cannot be read.
   It still has no production caller: its user is `php_view`'s `HOME`/`TMPDIR`
   write, which moves in `MB.36.5` (§8.3 asks for the helper in this leaf).

   The unit case makes the interlock **deterministic**: it spawns its own parked
   helper thread (`proc.rs:141`'s pattern — a `started`/`hold` channel pair), so
   the refusal is exercised on every run, `--test-threads=1` included, instead
   of depending on the harness's incidental thread count; then it asserts the
   sentence, asserts the value was **not** written, and runs the success path (a
   value set, a NUL refused without changing it) in a forked single-threaded
   child. Proven to fail: with the refusal branch removed, the case is red even
   single-threaded —

   ```console
   $ cargo test -p blastit-sys --lib --locked child::tests::the_environment -- --test-threads=1
   # mutated: the `if threads != 1 { … }` branch deleted
   test child::tests::the_environment_helper_refuses_a_multi_threaded_caller_and_sets_a_value_in_a_child ... FAILED
   thread '…' panicked at crates/blastit-sys/src/child.rs:579:31:
   a second thread must make `set_env` refuse: ()
   test result: FAILED. 0 passed; 1 failed; … finished in 0.00s

   $ cp /tmp/opencode/child.final.keep crates/blastit-sys/src/child.rs
   $ cmp -s crates/blastit-sys/src/child.rs /tmp/opencode/child.final.keep && echo RESTORED-BYTE-IDENTICAL
   RESTORED-BYTE-IDENTICAL
   # restored, same single-threaded invocation
   test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 151 filtered out; finished in 0.00s
   ```
6. **`blastit-view` gained a `blastit-sys` normal dependency** (the design's R2
   table lists it) to name `Fork`, `Rlimits`, `SignalMask`, `Privileges` and
   `Filter`; no features, no new third-party crate.
7. **`blastit-sys`'s `rustix` features gained `event`** — an existing
   dependency's feature, for the moved `poll` waits. `Cargo.lock` gained only
   edges (`blastit-sys` into `-view`/`-view-python`, `serde_json` as
   `-view-python`'s dev-dep); `cargo metadata --offline --locked` is clean.
8. **Two `cfg_attr(not(feature = "python"), allow(dead_code))` attributes, and
   why each is the honest shape** (they are the only two this node adds; the
   seven earlier ones collapsed to these when the two per-route vectors became
   one `ViewChildPlans`):
   * `crates/blastit/src/supervise.rs`'s `Supervisor.plans` — the same
     attribute the **adjacent `egress` field** has carried since `MB.3.11`, for
     the same shape of data: a route-parallel vector the supervisor lays out in
     every build and only the `#[cfg(feature = "python")]` fork arm reads. A
     `cfg` does not remove the escape — measured: with `#[cfg(feature =
     "python")]` on the field *and* its initialiser, the `field is never read`
     report simply moves up to `Parts.plans` (`supervise.rs:423`), because the
     only reader of that field is `new()`, which is the initialiser being gated.
     The two carriers, `Serving.plans` and `Parts.plans`, need no attribute at
     all (each is read unconditionally as the data is moved along); measured by
     removing all three and re-checking: only `Supervisor.plans` warned.
   * `crates/blastit/src/forking.rs`'s `fork_for` — its one caller is the WSGI
     fork arm, which a `python`-less build compiles out. It stays compiled
     there on purpose: its refusal of `ForkProtocol::InterpreterLive` in an
     image with no interpreter is a tested security property
     (`a_build_without_the_python_feature_refuses_an_interpreter_live_fork`), and
     gating the function would delete the property from exactly the build it
     protects.

## Evidence

### Rename summary (`-M`) — the five new files are untracked, and a subagent never stages

```console
$ git diff --stat -M -- Cargo.lock crates/
 Cargo.lock                            |    3 +
 crates/blastit-sys/Cargo.toml         |    2 +-
 crates/blastit-sys/src/lib.rs         |    8 +
 crates/blastit-view-python/Cargo.toml |    7 +
 crates/blastit-view-python/src/lib.rs |   27 +-
 crates/blastit-view/Cargo.toml        |    5 +
 crates/blastit-view/src/lib.rs        |    5 +
 crates/blastit/src/child.rs           |  312 +-------
 crates/blastit/src/config.rs          |   28 +-
 crates/blastit/src/forking.rs         |   88 +++
 crates/blastit/src/immortal.rs        | 1400 ---------------------------------
 crates/blastit/src/lib.rs             |   28 +-
 crates/blastit/src/py.rs              |   10 +
 crates/blastit/src/stage1.rs          |   64 +-
 crates/blastit/src/supervise.rs       |   94 ++-
 crates/blastit/src/zygote.rs          |  282 -------
 16 files changed, 349 insertions(+), 2014 deletions(-)

$ git status --short          # the whole tree, unfiltered
 M Cargo.lock
 M README.md
 M crates/blastit-sys/Cargo.toml
 M crates/blastit-sys/src/lib.rs
 M crates/blastit-view-python/Cargo.toml
 M crates/blastit-view-python/src/lib.rs
 M crates/blastit-view/Cargo.toml
 M crates/blastit-view/src/lib.rs
 M crates/blastit/src/child.rs
 M crates/blastit/src/config.rs
 M crates/blastit/src/forking.rs
 D crates/blastit/src/immortal.rs
 M crates/blastit/src/lib.rs
 M crates/blastit/src/py.rs
 M crates/blastit/src/stage1.rs
 M crates/blastit/src/supervise.rs
 D crates/blastit/src/zygote.rs
 M plan/BUDGET.md
 M plan/JOURNAL.md
 M plan/nodes/MB/MB.36/MB.36.4/LOG.md
 M plan/nodes/MB/MB.36/MB.36.4/NODE.md
 M plan/nodes/MB/MB.36/MB.36.6/LOG.md
?? crates/blastit-sys/src/child.rs
?? crates/blastit-sys/tests/critical_child_sequence.rs
?? crates/blastit-view-python/src/immortal.rs
?? crates/blastit-view-python/src/zygote.rs
?? crates/blastit-view/src/spawn.rs
?? docs/design/portable-lsp.review-ai-ergonomics.md
?? docs/design/portable-lsp.review-dx.md
?? plan/nodes/MB/MB.36/MB.36.4/RESULT.md
```

**Nothing here is staged, and nothing outside this node's `paths` was edited.**
Four groups are in that listing and are **not this node's work**, left exactly as
found:

* `README.md` — dirty by design: the release-time link rewrite `scripts/build-wheel.sh`
  owns it. Out of scope, never touched, never staged, never restored.
* `plan/BUDGET.md`, `plan/JOURNAL.md`, `plan/nodes/**/LOG.md`, `plan/nodes/**/NODE.md`
  — the plan system's own managed writes (status, attempts, timestamps), plus
  `MB.36.6`'s log. Mutated only through `tools/plan.py` by the harness, never by
  hand.
* `docs/design/portable-lsp.review-{ai-ergonomics,dx}.md` — untracked files of
  another node's (`MB.35`), present in this worktree before this node started.
* `plan/nodes/MB/MB.36/MB.36.4/RESULT.md` — this report.

The node's own change set is the sixteen `M`/`D` entries under `crates/` and
`Cargo.lock`, plus the five new files listed above.

```console
$ wc -l crates/blastit-sys/src/child.rs crates/blastit-sys/tests/critical_child_sequence.rs \
        crates/blastit-view/src/spawn.rs crates/blastit-view-python/src/immortal.rs \
        crates/blastit-view-python/src/zygote.rs crates/blastit/src/child.rs
   635 crates/blastit-sys/src/child.rs
   390 crates/blastit-sys/tests/critical_child_sequence.rs
   150 crates/blastit-view/src/spawn.rs
  1435 crates/blastit-view-python/src/immortal.rs
   285 crates/blastit-view-python/src/zygote.rs
    19 crates/blastit/src/child.rs

$ grep -c '#\[test\]' crates/blastit-view-python/src/immortal.rs
17            # the same 17 the file had in blastit
```

### The one-way arrow

```console
$ cargo tree -p blastit-view-python -e normal
blastit-view-python v0.2.0 (…/crates/blastit-view-python)
├── blastit-frame v0.2.0 (…/crates/blastit-frame)
│   ├── thiserror v2.0.21
│   └── …
├── blastit-sys v0.2.0 (…/crates/blastit-sys)
│   ├── libc v0.2.191
│   └── rustix v1.1.5
├── blastit-view v0.2.0 (…/crates/blastit-view)
│   ├── blastit-frame v0.2.0 (*)
│   ├── blastit-sys v0.2.0 (*)
│   └── thiserror v2.0.21 (*)
└── pyo3 v0.30.1
    ├── …
# no `blastit` package in the tree

$ cargo tree -p blastit-view -e normal | grep -c '^blastit '
0
```

### Acceptance

```console
$ cargo fmt --all -- --check
(no output)

$ cargo test -p blastit-view-python --locked
     Running unittests src/lib.rs
test result: ok. 59 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.16s
     Running tests/body.rs
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s
     Running tests/concurrency.rs
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s
     Running tests/critical_loaded_files.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
     Running tests/environ.rs
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running tests/flow.rs
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.77s
     Running tests/gil.rs
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/serve.rs
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.03s
   Doc-tests blastit_view_python
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ cargo test -p blastit-sys --locked
     Running unittests src/lib.rs
test result: ok. 152 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
     Running tests/critical_child_sequence.rs
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_embedded_seccomp.rs
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_fanotify.rs
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s
   Doc-tests blastit_sys
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ cargo test -p blastit-sys --lib --locked child::tests -- --test-threads=1
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 149 filtered out; finished in 0.00s
# the set_env refusal is exercised by this test's own parked thread, so it is
# asserted single-threaded too — not skipped.

$ cargo test -p blastit --lib --locked
test result: ok. 249 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.37s
$ cargo test -p blastit --lib --features python --locked
test result: ok. 266 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.88s

$ cargo test -p blastit --test critical_dev_and_fail_closed --locked
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
$ cargo test -p blastit --test critical_dev_and_fail_closed --features python --locked
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.65s
$ cargo test -p blastit --test critical_dev_reload --features python --locked   # §8.3: unchanged
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.26s

$ make critical-guard NODE=MB.36.4
uv run --no-project tools/critical_guard.py --base HEAD --node MB.36.4
critical-guard: HEAD..working tree, 20 changed file(s)
node MB.36.4: paths ['crates/blastit-view-python/**', 'crates/blastit-view/src/spawn.rs',
  'crates/blastit-view/src/lib.rs', 'crates/blastit-view/Cargo.toml', 'crates/blastit-sys/**',
  'crates/blastit/src/child.rs', 'crates/blastit/src/forking.rs', 'crates/blastit/src/immortal.rs',
  'crates/blastit/src/zygote.rs', 'crates/blastit/src/py.rs', 'crates/blastit/src/stage1.rs',
  'crates/blastit/src/supervise.rs', 'crates/blastit/src/config.rs', 'crates/blastit/src/lib.rs',
  'crates/blastit/Cargo.toml', 'Cargo.lock']
  no change to a protected critical test

$ test ! -e crates/blastit/src/immortal.rs && test ! -e crates/blastit/src/zygote.rs && echo "fork left the composition root"
fork left the composition root
$ test -z "$(grep -rn 'mod immortal\|mod zygote' crates/blastit/src)" && echo "no orphan modules"
no orphan modules
$ grep -q 'blastit_view_python::zygote' crates/blastit/src/supervise.rs && echo "supervisor calls the crate"
supervisor calls the crate

$ cargo check -p blastit --no-default-features --features php --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.92s

$ make rs-no-python
clippy      -p blastit{,-view} (standalone)
env -u PYO3_PYTHON -u PYTHONHOME cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.42s
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.72s
default build: 0 pyo3 dependencies (no libpython linked)
```

`cargo test -p blastit --locked` (the node's other acceptance line) is the same
21 test binaries as before this round: every one green, `grep -c '^test result:
ok'` = 21 and `grep -c '^test result: FAILED'` = 0 — and `make check`'s
`cargo test --workspace --all-targets --features python` below is the wider
form of it.

```console
$ make check
uv run --no-project tools/guard.py
guard: OK (8 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 22 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
env … cargo clippy --workspace --all-targets --locked --features python -- -D warnings
env … RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --locked --features python
env … cargo test --workspace --all-targets --locked --features python
env … cargo test --workspace --doc --locked --features python
# 196 `test result: ok` lines, 0 `test result: FAILED`, across the whole workspace
…
cargo build -p blastit --locked            # the default build is the standalone artifact
scripts/check-portable-elf.sh "…/target/debug/blastit"
scripts/check-no-inotify.sh "…/target/debug/blastit"
default build: 0 pyo3 dependencies (no libpython linked)
…
uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — … 60 packages in the normal closure, none of 12 forbidden; one home
UV_OFFLINE=1 uv run --no-project scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 229 assertions over 59 fixtures (43 cargo-checked)
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

## Follow-ups

1. **`crates/blastit/src/php_view.rs` still carries the PHP forks' own spellings of
   what `enter` now owns** — `own_process_group` (`:875`, its parent half
   `join_group` at `:888`), a second `set_view_environment` (`:1275`), and two
   doc links to the modules this node deleted (`:10` `crate::zygote`, `:1752`
   `crate::immortal`). The file is `MB.36.5`'s (`discovery`/`pinning`/`probe`/
   `spawn`), not this node's `paths`, so **all five sites are repointed at
   `MB.36.5`**: it deletes `own_process_group`/`join_group`'s child half (the
   probe passes `pgroup: true` through `ChildPlan`), reuses
   `blastit_view_python`'s/`blastit_sys`'s helpers instead of its own, and
   rewrites the two doc links. Until then the tree has two `setpgid(0, 0)`
   spellings.
2. **`docs/knowledge/LESSONS.md` candidates** (that file is not in this node's
   `paths`, so they are recorded here):
   * **libtest captures `println!` in a thread-local sink, so a forked child's
     `println!` never reaches its own pipe.** A protected test that forks and
     wants the child's output on the pipe `prepare` dup'ed onto fd 1 must write
     with `libc::write(1, …)`; `child_exit`'s raw fd-2 write works, which is why
     the symptom looked like "stdout empty, stderr fine".
   * **A unit test that re-runs its own binary with `--exact` breaks when the
     file changes crate:** libtest names tests by `module_path!`, which spells
     the crate `blastit_view_python`, while `env!("CARGO_PKG_NAME")` spells it
     `blastit-view-python`, so `strip_prefix` silently missed and the child ran
     **0 tests** ("the auto child printed no report (exit 0)"). Strip the
     package name with `-` → `_`.
   * **`O_DIRECTORY|O_NOFOLLOW` on a symlink returns `ENOTDIR` on Linux, not
     `ELOOP`**, so an assertion written for `ELOOP` fails on a correct refusal.
3. **`ARCHITECTURE.md` §9.3 still describes the split in `blastit` terms**; the
   design gives that sweep to `MB.36.6` (§8.5).
4. **`cargo check -p blastit --no-default-features --locked`** (no php, no
   static, no python) still reports two pre-existing `dead_code` warnings about
   `ViewReportEnd`'s fields; the acceptance configuration
   (`--no-default-features --features php`) is clean.
