# MA.2.2.4 — Leaf 7: dbproxy learn, dynamic-check and literal redaction

## Summary

`blastit-dbproxy` is now the wire observation source of truth for `0.1.1`:

- **`ListenerMode::{Enforce, Learn, DynamicCheck}`** on `DbproxyConfig` (serde
  `enforce` / `learn` / `dynamic_check`). `Enforce` keeps today's decisions and
  SQLSTATEs (`42501`, `sql.denied`, root refused) with one intended change: the
  `sql` a denial carries is now redacted (R24). Both learning modes answer
  `QueryDecision::Allow` for *every* message and never emit `sql.denied`
  (R26, ADR-0003, ADR-0004).
- **`ObsTarget` / `WireSqlObs`** (`ObsTarget` is `#[serde(tag = "scope",
  rename_all = "snake_case")]`, so `{"scope":"routed","route":…}` vs
  `{"scope":"unrouted","method":…,"path":…}` needs no delimiter convention).
  `WireSqlObs.sql` is **always** `redact_literals(normalized)`; `verdict` is the
  wire form of `LearnVerdict` (`query`/`refused`, same variant and field names);
  `kind` serialises through `SqlKind::as_str()`, so the pipe keeps one
  vocabulary. One JSON object per line, only ever on `obs_fd` — the public event
  stream gains no kind.
- **JSON-line `check_ctl_fd`**: `push_routed`, `push_unrouted`, `pop`, `set_db`
  → `{"ok":true}` / `{"ok":false,"error":…}`. 4096 B per line (a longer line is
  refused *before* another byte is buffered), one 5 s deadline per read and per
  write. A malformed or oversized line is answered and the channel closed; a
  line that stops mid-way is cut off at 5 s, so no peer can hang dbproxy.
- **One-shot session-setup replay**: `SET`/`SHOW`/`KNOWN_STATIC_SETUP_SQL`/
  read-only catalogue introspection seen under an empty target stack is recorded
  per session (≤ 8 distinct statements) and replayed **once**, against the first
  target the session reports (routed or unrouted — either is a request whose
  route needs the same grants). Anything else under an empty stack is
  fixture/migration SQL and produces zero `obs_fd` lines.
- **`set_db`** steers the database a proxy's *next* sessions request (the
  auth hook reads the override); an open session keeps what it opened with.
- **Literal redaction (S1, R24)** in every `SqlDenied` (all three construction
  paths go through one `denial()`), in `WireSqlObs.sql`, and in
  `ObsVerdict::Refused.redacted`. No user-derived literal byte reaches an event
  or the obs pipe.
- **Root listeners**: a `uid == 0` listener is refused unless
  `allow_root_listener` is set (ADR-0001); when it is, only uid 0 is admitted,
  and only on a listener that declares 0.
- **Wildcard route**: `ListenerSpec.route == "*"` is accepted iff the mode is a
  learning one, at most once per proxy. A wildcard listener evaluates a
  statement against `route_policies[(target route, proxy)]` (derived solely from
  `Manifest::effective_grants`), and an unknown route against an **empty**
  allowlist (`allowed: false`).
- `obs_fd` / `check_ctl_fd` are only valid outside `Enforce` (a descriptor that
  would be silently ignored is a configuration error instead).

New: `crates/blastit-dbproxy/src/learn.rs`,
`crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs` (8 tests, no
`#[ignore]`, `MockPgServer` on `127.0.0.1:0`, no root, no Docker). Existing
`tests/postgres.rs` and the unit tests only gained the new config fields
(`..DbproxyConfig::default()`); no assertion was weakened.

## Changes (paths from `NODE.md` only)

```
crates/blastit-dbproxy/src/config.rs      ListenerMode, RoutePolicySpec, WILDCARD_ROUTE,
                                          new DbproxyConfig fields + validation split
crates/blastit-dbproxy/src/learn.rs       NEW: ObsTarget, WireSqlObs, ObsVerdict, Learning,
                                          control protocol, setup replay, redaction
crates/blastit-dbproxy/src/hooks.rs       learning branch, one denial() with redact_literals,
                                          allow_root_listener, set_db database
crates/blastit-dbproxy/src/state.rs       Learning on Shared, allowlist_for, credentials_match
                                          (+allow_root), build() validates served specs
crates/blastit-dbproxy/src/server.rs      control task adoption, session forget, root rule
crates/blastit-dbproxy/src/lib.rs         exports
crates/blastit-dbproxy/tests/postgres.rs  new config fields only
crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs   NEW (8 tests)
```

One file outside `paths`: `docs/knowledge/LESSONS.md` got the single bullet the
implementer role asks for (`obs_fd` writers block their session when the reader
stops draining). It is agent knowledge, excluded from the public Zensical site.

## Evidence

### `cargo test -p blastit-dbproxy --test critical_learn_and_redaction --locked`

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-dbproxy --test critical_learn_and_redaction --locked
running 8 tests
test a_root_listener_without_the_opt_in_is_rejected ... ok
test a_malformed_control_line_is_answered_and_closed ... ok
test an_oversized_control_line_is_refused_without_buffering_it ... ok
test enforce_still_answers_42501_and_redacts_every_literal ... ok
test an_unrouted_request_carries_its_own_target ... ok
test set_db_switches_the_database_the_next_session_requests ... ok
test learning_ignores_fixture_sql_and_replays_setup_once_on_the_first_route ... ok
test a_stalled_control_line_times_out_instead_of_hanging ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.00s
```

What each twin pins:

- `learning_ignores_fixture_sql_and_replays_setup_once_on_the_first_route`: the
  empty-stack `INSERT` produces **no** obs line; `SET TIME ZONE 'UTC'` and
  `select pg_catalog.version()` produce none either until the first
  `push_routed`, which replays both **before** the routed statement; a second
  routed request replays nothing; the ungranted `SELECT 's3cr3t' FROM orders
  WHERE id = 1` is allowed, observed as `SELECT … FROM orders WHERE id = 1`
  with `allowed: false` and a refused verdict, the mock server saw every
  statement, and the whole obs byte stream contains no `s3cr3t`.
- `an_unrouted_request_carries_its_own_target`: `ObsTarget::Unrouted` attribution
  plus redaction.
- `set_db_switches_the_database_the_next_session_requests`: the mock's startup
  log goes `app` → `test_shop`; an unknown proxy is refused.
- `enforce_still_answers_42501_and_redacts_every_literal`: `42501`, the refusal
  names the route, the denied statement never reaches the database, the event
  carries `SELECT … FROM orders WHERE id = 1`, and neither `encode_json` nor
  `Debug` of any event contains `s3cr3t`.
- `a_root_listener_without_the_opt_in_is_rejected`: `run_with_ready` refuses
  with a message naming `uid 0` and no credential.
- The three ctl tests: malformed line → `{"ok":false,"error":"malformed control
  command"}` then EOF; 8 KiB line → error naming 4096 then EOF; a stalled
  partial line → EOF after ~5 s (asserted `>= 4 s`, `<= 8 s`).

### `cargo test -p blastit-dbproxy --locked`

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-dbproxy --locked
unittests src/lib.rs:    test result: ok. 65 passed; 0 failed; 0 ignored
unittests src/main.rs:   test result: ok.  5 passed; 0 failed; 0 ignored
tests/critical_learn_and_redaction.rs: test result: ok. 8 passed; 0 failed; 0 ignored
tests/postgres.rs:       test result: ok. 24 passed; 0 failed; 0 ignored   (no BLASTIT_TEST_PG_URL: they print their skip note)
Doc-tests:               test result: ok.  1 passed; 0 failed
```

`cargo clippy -p blastit-dbproxy --all-targets --locked -- -D warnings` → clean.
`RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-dbproxy --no-deps --locked` → clean.

### `cargo fmt --check`

```console
$ cargo fmt -p blastit-dbproxy -- --check
(clean)
$ cargo fmt --all -- --check
(clean, whole workspace)
```

(The workspace form was briefly red in `crates/blastit-http/**` and
`crates/blastit-scheduler/**` while the sibling `MA.2.2.3` node was still
writing those files; it is clean now.)

### `CARGO_NET_OFFLINE=true make check`

```console
$ CARGO_NET_OFFLINE=true make check
uv run --no-project tools/guard.py
guard: OK (8 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 27 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
… cargo clippy --workspace --all-targets --locked -- -D warnings
… cargo doc --workspace --no-deps --locked
… cargo test --workspace --all-targets --locked
     Running tests/critical_learn_and_redaction.rs
      test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
… cargo test -p blastit-dbproxy --test critical_learn_and_redaction
      test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
uvx --offline ruff check tools
All checks passed!
uvx --offline ruff format --check tools
3 files already formatted
uvx --offline zensical build --strict
No issues found
uv run --no-project tools/plan.py validate
plan OK
rc=0
```

71 `test result: ok` lines, zero failures.

## Follow-ups (outside this node's `paths`)

1. **Orchestrator wiring (Leaf `MA.2.3.x`, `crates/blastit/src/**`):** the new
   `DbproxyConfig` fields must be set at the two call sites —
   `mode`, `route_policies` (from `Manifest::effective_grants`), `obs_fd`
   (`--learn`/`check`), `check_ctl_fd` (`ctl.sock`), and
   `allow_root_listener = (geteuid() == 0)` (ADR-0001, `blastit dev` in a root
   container). `DbproxyConfig::enforcing(proxies, listeners)` keeps its
   signature and therefore still means "Enforce, no descriptors"; construct with
   `DbproxyConfig { mode, allow_root_listener, obs_fd, check_ctl_fd,
   route_policies, ..DbproxyConfig::enforcing(…) }`.
2. **`blastit-sqlpolicy` derives:** `LearnVerdict`, `RefuseReason` and `SqlKind`
   are not `Serialize`, so `WireSqlObs` carries `ObsVerdict` (same variants, tag
   and field names as `LearnVerdict`, with the reason's one-line `Display`) and
   a `serde(with)` module for `SqlKind`. Adding `Serialize`/`Deserialize` in
   `blastit-sqlpolicy` would let `MA.2.3.x` consume the sqlpolicy types
   directly; that crate is outside this node's paths.
3. `docs/reference/log-events.md` needs no edit: `sql.denied`'s `sql` field is
   still "normalised", and `docs/local.md:114` already promises `…` for quoted
   literals. The event now keeps that promise.
