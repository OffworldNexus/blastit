# Result MA.2.2.5

**Status: done**

Pinned the socket-derived identity check (`blastit_dbproxy::state::credentials_match`) in the
protected critical tier. Test-only change: one file, no production code, no `#[ignore]`, no new
skips. The file is owned by this node's `paths`, so `tools/critical_guard.py` accepts it.

## What changed

`crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs` (+156/-10):

- **New critical test** `a_listener_for_another_uid_refuses_the_client_before_the_upstream`:
  - a listener declaring `another_uid()` (`current_uid() + 1`, never 0) refuses a client whose
    kernel `SCM_CREDENTIALS` carry `current_uid()`, with an `ErrorResponse` SQLSTATE `28000`
    (`invalid_authorization_specification`, `docs/troubleshooting.md`'s `FATAL 28000`);
  - the refusal ends the session before any `ReadyForQuery`, names neither the upstream host nor
    a credential, and a `Query` written behind the refusal becomes nothing;
  - the `MockPgServer` receives **no startup packet and no statement**;
  - **no** `obs_fd` byte and no `sql.denied` event is produced; exactly one `pg.refused` with
    `PgRefuseReason::CredentialsMismatch` on proxy `db` is emitted;
  - **accepted twin in the same test**: the same configuration declaring `current_uid()` serves
    the client (trusted startup, `SELECT 1`) and is the only one that reaches the upstream
    (`startups() == [real-user/app]`, `statements() == ["SELECT 1"]`) — so the oracle cannot be
    satisfied by "the proxy refuses everything".
- Harness, no semantics changed: `RawClient::connect` split into `RawClient::begin` (returns the
  frames, asserts nothing) + `connect` (asserts the trust handshake, as before); `start` split
  into `start_as(name, config, route, uid)` + `start` (= `start_as(.., current_uid())`), so a test
  can declare another uid on the listener it still binds itself. `another_uid()` added next to
  `current_uid()`. Module docs list the new negative twin.

## Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-dbproxy --test critical_learn_and_redaction --locked
running 9 tests
test a_root_listener_without_the_opt_in_is_rejected ... ok
test an_oversized_control_line_is_refused_without_buffering_it ... ok
test a_malformed_control_line_is_answered_and_closed ... ok
test enforce_still_answers_42501_and_redacts_every_literal ... ok
test an_unrouted_request_carries_its_own_target ... ok
test set_db_switches_the_database_the_next_session_requests ... ok
test a_listener_for_another_uid_refuses_the_client_before_the_upstream ... ok
test learning_ignores_fixture_sql_and_replays_setup_once_on_the_first_route ... ok
test a_stalled_control_line_times_out_instead_of_hanging ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.04s
EXIT=0
```

Baseline before the change: `8 passed; 0 failed; 0 ignored` (the file's 8 existing tests).

## Mutation proof (scratch copy, working tree never mutated)

`rsync` of the tree (no `target/`, no `.git/`, no `plan/`) to
`/tmp/opencode/blastit-ma225-mutant`, separate `CARGO_TARGET_DIR`, one anchor-checked
replacement of `state.rs`, suite re-run.

**1. The audit's exact survivor `db.credentials_any_uid`** —
`root_allowed && credentials.uid == spec_uid` -> `root_allowed` (any non-root uid admitted):

```console
$ grep -n "let root_allowed" -A2 crates/blastit-dbproxy/src/state.rs   # scratch copy
449:            let root_allowed = credentials.uid != 0 || (allow_root && spec_uid == 0);
450-            root_allowed
$ CARGO_TARGET_DIR=... cargo test -p blastit-dbproxy --test critical_learn_and_redaction --locked
test a_listener_for_another_uid_refuses_the_client_before_the_upstream ... FAILED
test enforce_still_answers_42501_and_redacts_every_literal ... ok
test learning_ignores_fixture_sql_and_replays_setup_once_on_the_first_route ... ok
test set_db_switches_the_database_the_next_session_requests ... ok
...
---- a_listener_for_another_uid_refuses_the_client_before_the_upstream stdout ----
thread '...' panicked at .../critical_learn_and_redaction.rs:1010:28:
a stranger uid is refused: [Frame { tag: 82, body: [0, 0, 0, 0] }, Frame { tag: 83, ... },
 Frame { tag: 75, body: [0, 15, 66, 64, 145, 32, 25, 184] }, Frame { tag: 90, body: [73] }]

test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

Exactly the surviving mutant is now killed by the protected suite, by this one test alone (the
other 8 critical tests stay green), and the client is shown a fully accepted session.

**2. The requested inverted comparison** — `==` -> `!=`:

```console
$ sed -i 's/root_allowed && credentials.uid == spec_uid/root_allowed \&\& credentials.uid != spec_uid/' state.rs
$ cargo test -p blastit-dbproxy --test critical_learn_and_redaction --locked
failures:
    a_listener_for_another_uid_refuses_the_client_before_the_upstream
    an_unrouted_request_carries_its_own_target
    enforce_still_answers_42501_and_redacts_every_literal
    learning_ignores_fixture_sql_and_replays_setup_once_on_the_first_route
    set_db_switches_the_database_the_next_session_requests
test result: FAILED. 4 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out
```

(The inversion also refuses the legitimate uid, which is why the accepted twin and four existing
tests go red as well — the new test fails on its own oracle, not on a harness error.)

## Gates

```console
$ rustfmt --edition 2024 --check crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs
FMT-OK
$ CARGO_NET_OFFLINE=true cargo clippy -p blastit-dbproxy --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.44s
$ CARGO_NET_OFFLINE=true cargo test -p blastit-dbproxy --all-targets --locked
test result: ok. 65 passed; 0 failed; 0 ignored   (src/lib.rs unit)
test result: ok. 5 passed; 0 failed; 0 ignored    (src/main.rs unit)
test result: ok. 9 passed; 0 failed; 0 ignored    (critical_learn_and_redaction)
test result: ok. 24 passed; 0 failed; 0 ignored   (postgres.rs)
$ uv run --no-project tools/critical_guard.py --base f3f26fb4cf928d24f25d1a807e9a3dc82883db82 --node MA.2.2.5
  M  crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs  [authorized by ...]
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-scheduler/tests/critical_ingress_enforcement.rs  [VIOLATION: not covered by any node path]
```

Those three violations are the **parallel pins MA.2.2.6/7/8** editing their own files in this same
working tree; my file is authorized. The union-path gate used by `make check` accepts all four, as
the full run below shows.

```console
$ make check
... test result: ok. 25 passed; 0 failed; 0 ignored
no Python critical tests yet (python/tests/critical/)
uvx --offline ruff check tools
All checks passed!
uvx --offline ruff format --check tools
3 files already formatted
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.21s
uv run --no-project tools/plan.py validate
plan OK
```

Exit status 0 (the last `plan-validate` prerequisite ran, so every target in the chain passed).
Note: this ran while the sibling pins' in-flight critical-test edits were present in the tree, so
it also covers their current state.

## Scope

Touched only:

- `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs`
- `plan/nodes/MA/MA.2/MA.2.2/MA.2.2.5/RESULT.md`

No production code, no `Cargo.toml`/`Cargo.lock`, no `docs/**`, no plan status mutation, no
commit/push/tag.

## Follow-ups

- None for this node. `docs/testing/AUDIT-MA.2.2.md` G1 is closed by this test; the audit's
  finish note (install `cargo-mutants` for the MA gate) still stands and is out of scope here.
- `RawClient::begin` now exists as the unasserting half of `connect`; a future refusal test can
  reuse it instead of duplicating the startup packet.
