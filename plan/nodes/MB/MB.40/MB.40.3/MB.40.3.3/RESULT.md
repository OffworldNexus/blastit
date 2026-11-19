# MB.40.3.3 — real TCP PostgreSQL-upstream harness for both critical suites

## Result

`crates/blastit/tests/common/mock_pg.rs` (282 LOC) is deleted, together with the
inline duplicate at the top of
`crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs`. Both critical
suites now drive one named, real-upstream harness — `PgUpstream` — a real
`std::net::TcpListener` on `127.0.0.1:0` speaking the PostgreSQL v3 subset the
suites need. The dbproxy, its per-route Unix sockets, the policy
(`blastit-sqlpolicy`) and the wire framing under test stay the real crates; only
the *external* PostgreSQL peer is a fixture, and it is named after what it is.
`grep -rni "mock_pg\|MockPg" .` (excluding `target/`, `plan/`, `.git/`) returns
nothing **in any code path this node owns or links**: `crates/` is clean. The
repo-wide grep still matches three places outside this node's `paths`, none of
them a fixture the critical Rust suites load (details under
"Residual mentions" below).

## Files

| Change | Path |
|---|---|
| deleted | `crates/blastit/tests/common/mock_pg.rs` |
| added | `crates/blastit/tests/common/pg_upstream.rs` (`PgUpstream`, 346 LOC) |
| added | `crates/blastit-dbproxy/tests/common/mod.rs` (`pub mod pg_upstream;`) |
| added | `crates/blastit-dbproxy/tests/common/pg_upstream.rs` — byte-identical copy (`cmp` clean), because a crate cannot `mod` another crate's `tests/` tree |
| added | `crates/blastit/tests/common_harness_parity.rs` — the divergence guard (`include_str!` parity test, no new target in the dbproxy crate) |
| edited | `crates/blastit/tests/common/mod.rs` (`mock_pg` → `pg_upstream`) |
| edited | `crates/blastit/tests/critical_dev_and_fail_closed.rs` (fixture swap only) |
| edited | `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs` (fixture removed, swap only) |

No production dependency and no `[dev-dependencies]` change: the harness uses
only `std` (the suite's existing `serde_json`/`rustix`/`tokio` imports are
untouched), so `Cargo.toml`/`Cargo.lock` are unchanged.

## Harness shape (`crates/*/tests/common/pg_upstream.rs`)

* `PgUpstream::start()` binds `127.0.0.1:0` (ephemeral — never 5432), sets the
  listener non-blocking, and serves each accepted connection on its own `std`
  thread. `Drop` flips a shutdown flag, connects once to wake the accept loop and
  joins it, so no port or thread outlives a case.
* Startup: length-prefixed packet; `SSLRequest` is answered with `N` (loopback,
  cleartext), then protocol 3.0 is answered with `AuthenticationOk` (`R`),
  `client_encoding`/`standard_conforming_strings`/`server_version`
  `ParameterStatus` frames (`S`), `BackendKeyData` (`K`) and `ReadyForQuery`
  (`Z`).
* Statements: simple query (`Q`) answers `RowDescription` (`T`), `DataRow` (`D`)
  with the single text value `0`, `CommandComplete` (`C`) and `ReadyForQuery`
  (`Z`); the extended sequence answers `ParseComplete` (`1`), `BindComplete`
  (`2`), `RowDescription` to `Describe` (`D`), and `DataRow` + `CommandComplete`
  to `Execute` (`E`), with `ReadyForQuery` at `Sync` (`S`). `Close`/`Flush`/
  `Terminate` are handled.
* Recording: `startups() -> Vec<StartupRequest>` (public `user`/`database`) and
  `statements() -> Vec<String>`, shared with the connection threads through a
  `Recorded` struct of two `Mutex`es — one cohesive object instead of a flat bag
  of `Arc`s.
* `frame(tag, body)` is `pub` in the harness: the dbproxy suite's hand-written
  raw client writes its own startup/query frames with the same encoder, so there
  is one framer rather than a second copy in the test file.
* Zero `unwrap()` on untrusted bytes: startup/message headers use
  `try_into().map_err(...)`, length bounds are checked before allocation, and
  the two `expect`s are on harness setup (bind a loopback listener), which is a
  broken host, not a test input.

## Call-site swap (assertions untouched)

* `MockPgServer::start()` → `PgUpstream::start()` (blastit: 17 sites; dbproxy: 9
  sites, which also lose the `.await` because the harness is a `std` thread — it
  is still called from inside the `#[tokio::test]` bodies).
* `&MockPgServer` → `&PgUpstream` in `database_url`, `proxy_spec` and
  `dynamic_check`; `server.port` → `server.port()` in `proxy_spec`.
* `use common::mock_pg::MockPgServer` → `use common::pg_upstream::PgUpstream`;
  dbproxy additionally imports the shared `StartupRequest` and `frame`.
* Wording only: the doc headers, the `DatabaseUrl` comment, and the two
  "mock"/"the mock saw them" assertion *messages* now say "upstream". No test
  name, assertion, count or `#[ignore]` changed.

## Divergence guard (the "no two divergent copies" clause)

`crates/blastit/tests/common_harness_parity.rs` is one `#[test]` that compares
`include_str!("common/pg_upstream.rs")` with
`include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../blastit-dbproxy/tests/common/pg_upstream.rs"))`
and fails with a message telling the next editor to copy the edited file
verbatim into the other crate in the same change.

* It compares the bytes the compiler was handed, in the same spirit as the suites
  using real sockets, so it fails on the edit rather than on a regex.
* It is a test in its own target, not a block added to the harness module, so
  **both protected suites keep exactly their pre-change test counts** (20 and 9);
  the guard adds no test to a protected binary and no `scripts/check_*.py`.
* It is not gated on the `python` feature: `make fast-check` runs it on every
  slice, so an edit to either copy is caught by the workspace gate even when the
  dbproxy suite is the one being run alone.
* Packaging is unaffected: both crates are `publish = false`, and the path
  resolves from either crate at compile time (`CARGO_MANIFEST_DIR` + `../`).

## Test counts (base `84ff143` → this change)

| Suite | at `84ff143` | now |
|---|---|---|
| `blastit --test critical_dev_and_fail_closed` | 20 `#[test]` | 20 passed |
| `blastit-dbproxy --test critical_learn_and_redaction` | 9 `#[tokio::test]` | 9 passed |

## Acceptance evidence

```
$ cargo test -p blastit-dbproxy --locked --test critical_learn_and_redaction
running 9 tests
...
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.01s

$ cargo test -p blastit --locked --features python --test critical_dev_and_fail_closed
running 20 tests
...
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.75s

$ cargo test -p blastit --locked --test common_harness_parity
running 1 test
test both_crates_carry_the_same_upstream_harness ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ cmp crates/blastit/tests/common/pg_upstream.rs \
      crates/blastit-dbproxy/tests/common/pg_upstream.rs
copies identical
```

## Residual mentions (out of this node's `paths`, nothing loads them)

`grep -rni "mock_pg\|MockPg" . --exclude-dir=target --exclude-dir=plan --exclude-dir=.git`

| Where | What | Why it is untouched |
|---|---|---|
| `.opencode/agents/*.md`, `.opencode/skills/testing/SKILL.md`, `AGENTS.md` | agent policy prose naming the banned symbol | instruction text, not code |
| `docs/design/ma-lean-dx*.md` | frozen DX design/review documents describing the historical fixture and its leaf | history belongs to the design record; `docs/` is outside this node |
| `python/tests/critical/test_check_contract.py` (`class MockPgServer`, docstring "see `mock_pg.rs`") | an *independent* Python-side PG stub for the `check` contract mini-runner | **protected** critical test (`tools/critical_guard.py`); renaming it needs its own authorized node, and it is unrelated to the two Rust suites this node swaps |

`grep -rni "mock_pg\|MockPg" crates/` → no matches.

## Follow-ups

* The harness is duplicated byte-for-byte across the two crates because a test
  file can only `mod` modules inside its own crate's `tests/` tree; a shared
  `tests/support` crate would remove the copy but needs a new workspace member
  (and therefore `Cargo.toml`/`Cargo.lock`), which is out of this node's paths.
  Until then `tests/common_harness_parity.rs` makes any fork fail the workspace
  gate; if the copy is ever edited, `cp` the file and re-run `make fast-check`.
* Rename `python/tests/critical/test_check_contract.py`'s own `MockPgServer`
  class (and its "see `mock_pg.rs`" docstring) so the banned symbol disappears
  repo-wide: it is a protected critical test, so it needs a node that lists it
  in `paths` and authorizes the edit.
