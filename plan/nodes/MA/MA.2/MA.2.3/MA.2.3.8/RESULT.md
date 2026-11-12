# MA.2.3.8 — Clean `SIGTERM` to `blastit dev` exits `0`

Status: **done** (attempt 1). Changed exactly one source file
(`crates/blastit/src/supervise.rs`); `crates/blastit/tests/critical_dev_and_fail_closed.rs` is
byte-identical to its committed state.

## What the bug actually was

`docs/testing/AUDIT-MA.2.3.md` F4 blamed a late `ctl.chan` **write** (`EPIPE`) in
`Supervisor::send_ctl`. That is one half. Reproducing it under load exposed the other half, which
is what most runs hit:

```text
INFO  shutdown    drain
INFO  process.exited sql  pid=… status=0
INFO  process.exited _learn_fallback  pid=… status=0
ERROR app.output  stderr  ctl.chan: Connection reset by peer (os error 104)
```

`ctl.chan` is an `AF_UNIX` `SOCK_STREAM` pair. When the scheduler exits while messages are still
queued for it (a `DRAIN` it never read, or an `EXITED` written after it left), init's next `read(2)`
returns **`ECONNRESET`**, not `0`. `read_ctl` only special-cased `read == 0`, so the reset took the
fatal arm — exit `1` from a clean `SIGTERM`, with the message emitted as an `app.output`/stderr
event (which is why the F4 trace looks like a write failure).

## The fix (`crates/blastit/src/supervise.rs`)

1. `ctl_peer_gone(raw)` — one classifier for "the scheduler's end is gone": `EPIPE | ECONNRESET`.
2. Read side: `read_ctl` sends a lost-peer error to `ctl_closed()` (the existing EOF arm), which is
   fatal only while serving and is the normal end of a drain.
3. Write side: the write loop is extracted (`write_ctl`, `wait_for_room`) and returns
   `CtlWriteError`; `ctl_failure_is_fatal(shutting_down, error)` decides. A gone peer after
   `SIGTERM`/`SIGINT` is the closed-fd arm by another name; **everything else stays fatal**, and
   `SEND_CTL_TIMEOUT` + "init must not pretend a dead component is alive" are untouched:
   - `Wedged` (the scheduler took nothing for `SEND_CTL_TIMEOUT`) is fatal **always**, drain or not.
   - Any other errno is fatal always.
   - A gone peer **before** shutdown is still fatal (dead scheduler while serving).
4. `send_spawned` (the `SCM_RIGHTS` writer) uses the same rule: a `SPAWN` that raced the `SIGTERM`
   no longer fails the clean exit; its view is killed in `Terminate` anyway.

No `#[ignore]`, no sleeps added, no timeouts changed, no test weakened.

## Deterministic regressions (unit tier, `crates/blastit/src/supervise.rs`)

Three tests, all socketpair-based (peer closed before/after the message, so no scheduling race and
nothing to tune):

| test | pins |
|---|---|
| `a_late_ctl_write_to_a_gone_scheduler_is_best_effort` | a real `EPIPE` through `write_ctl` is best effort **only** after shutdown; before shutdown it is fatal |
| `a_scheduler_that_closes_with_data_queued_resets_the_channel` | a real kernel `ECONNRESET` (asserted as `libc::ECONNRESET`) is classified as a lost peer by the exact predicate `read_ctl` branches on, while `EINVAL` is not |
| `every_other_ctl_failure_stays_fatal` | `Wedged` and non-peer-gone write errors stay fatal in **both** phases (§8 not weakened) |

### Proof they fail without the fix (scratch mutations, reverted, never committed)

1. `ctl_failure_is_fatal` → `true` (the pre-fix "always fatal" write rule):

```console
test supervise::tests::a_late_ctl_write_to_a_gone_scheduler_is_best_effort ... FAILED
panicked at crates/blastit/src/supervise.rs:1048:
a late ctl.chan write during the drain must be best effort, or a clean SIGTERM exits 1 (finding F4)
test result: FAILED. 2 passed; 1 failed
```

2. `ctl_read_lost_peer` → `false` (the pre-fix read path, which fell into `self.fail`):

```console
test supervise::tests::a_scheduler_that_closes_with_data_queued_resets_the_channel ... FAILED
panicked at crates/blastit/src/supervise.rs:1107:
a reset means the scheduler's end is gone, so `read_ctl` must go through `ctl_closed` …
test result: FAILED. 2 passed; 1 failed
```

Both mutations restored; `grep -rn "SCRATCH MUTATION" crates/` prints nothing and
`git diff crates/blastit/tests/` is empty.

### End-to-end before/after (scratch harness, `/tmp/opencode/egress/run.sh`, not committed)

A bash harness boots the real binary with the `egress = true` fixture, sends the two `/…/inet`
requests, `SIGTERM`s init, and prints init's whole stdout on a non-zero exit. Run as 8 parallel
instances × 800 runs (the same contention the suite has with `--test-threads=4`):

| binary | runs | failures |
|---|---|---|
| write fix only (read path still fatal) | 6400 | **5** — every one `ctl.chan: Connection reset by peer (os error 104)`, exit `1`, after `shutdown drain` |
| both fixes | 6400 | **0** |

The same class surfaced in the real suite while diagnosing: the 400-run
`cargo test … -- --test-threads=4` loop failed at run 114 in
`learn_writes_routes_and_queries_on_sigterm` with that exact `ECONNRESET` line.

## Acceptance

### 1. `for i in $(seq 30); do cargo test -p blastit --test critical_dev_and_fail_closed -- --test-threads=4; done`

```console
GREEN=30 RED=0
30
```

(`30` = count of `test result: ok. 11 passed; 0 failed; 0 ignored` blocks; last run:)

```console
test only_a_granted_route_gets_a_pg_socket ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.57s
```

Extra confidence beyond the required 30 (same command, 300 more runs): `GREEN=300 RED=0`.

### 2. `cargo test -p blastit --test critical_dev_and_fail_closed`

```console
test fatal_startup_removes_the_ephemeral_dev_root ... ok
test unprivileged_production_has_no_single_uid_fallback ... ok
test a_non_loopback_host_needs_the_acknowledgement ... ok
test one_route_banner_singularises ... ok
test an_egress_false_view_cannot_open_inet ... ok
test json_mode_emits_privilege_and_never_a_port_key ... ok
test dev_boots_five_unprivileged_roles_and_strips_secrets ... ok
test only_a_granted_route_gets_a_pg_socket ... ok
test learn_writes_routes_and_queries_on_sigterm ... ok
test learn_attributes_queries_per_route_across_concurrent_live_views ... ok
test first_declared_route_keeps_all_queries_when_unrouted_requests_exist ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.41s
```

### 3. `make test-critical`

```console
cargo test -p blastit-dbproxy --test critical_learn_and_redaction      test result: ok. 9 passed …
cargo test -p blastit-http --test critical_early_abort                 test result: ok. 5 passed …
cargo test -p blastit-ingress --test critical_ingress                  test result: ok. 55 passed …
cargo test -p blastit-manifest --test critical_manifest_011            test result: ok. 17 passed …
cargo test -p blastit-manifest --test critical_manifest_edit           test result: ok. 18 passed …
cargo test -p blastit-py --test critical_check_schema                  test result: ok. 6 passed …
cargo test -p blastit-scheduler --test critical_ingress_enforcement    test result: ok. 9 passed …
cargo test -p blastit-sqlpolicy --test critical_literals               test result: ok. 25 passed …
cargo test -p blastit --test critical_check_engine                     test result: ok. 13 passed …
cargo test -p blastit --test critical_config_and_presets               test result: ok. 11 passed …
cargo test -p blastit --test critical_dev_and_fail_closed              test result: ok. 11 passed …
python -m unittest discover -s python/tests/critical
Ran 49 tests in 3.637s
OK
exit=0
```

### 4. `make check` (implementer gate)

```console
test supervise::tests::a_late_ctl_write_to_a_gone_scheduler_is_best_effort ... ok
test supervise::tests::a_scheduler_that_closes_with_data_queued_resets_the_channel ... ok
test supervise::tests::every_other_ctl_failure_stays_fatal ... ok
test result: ok. 167 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.30s
…
Ran 49 tests in 3.190s
OK
uvx --offline ruff check python tools          All checks passed!
uvx --offline ruff format --check python tools  43 files already formatted
uvx --offline zensical build --strict          No issues found
uv run --no-project tools/plan.py validate     plan OK
exit=0
```

## Follow-ups (out of scope, recorded for the lead)

- The audit's other findings (F1/F2/F3 pytest-plugin CI job, F5 `check --learn` exit code, F6..F9
  inference/audit rules) are untouched; sibling nodes cover them.
- The flake only reproduces under parallel load: a single-threaded loop of this suite can run
  hundreds of times clean on a quiet machine, so a green `--test-threads=4` loop is the honest
  gate for this class of bug.
- `docs/knowledge/LESSONS.md` gained one bullet on the `AF_UNIX` reset-instead-of-EOF behaviour
  (it is the non-obvious half of F4).
