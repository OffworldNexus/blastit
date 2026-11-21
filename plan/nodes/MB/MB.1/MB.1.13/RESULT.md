# MB.1.13 — RESULT

Status: **done** — the protected suite now owns the `add_group` oracle in both directions, the
standard-tier case file keeps every assertion plus a pointer, and `docs/design/ma-lean-dx.md`
attributes the oracle to the file that really asserts it. No production change.

## What changed

- `crates/blastit/tests/critical_check_engine.rs` (protected, extended only): new section
  *R33 / `MA.8`: session setup names the `connection` group, never one route* with a `fix_of`
  helper (whole `fix` object compared, so an extra field fails too) and 3 tests:
  - `connection_setup_sql_names_the_connection_group_grant` — `SELECT set_config('TimeZone', $?, false)`,
    `SET search_path = public`, `SHOW standard_conforming_strings` each emit exactly
    `{"action":"add_group","to":"[group.connection.db].queries"}`.
  - `ordinary_statements_keep_the_per_route_add_query` — `SELECT 1` and
    `DELETE FROM "p" WHERE "p"."id" = $?` each emit exactly
    `{"action":"add_query","to":"[route.db].queries"}`.
  - `only_connection_setup_names_a_group_and_never_a_refusal` — one mixed run: exactly the setup
    statement is in the `add_group` set; `SELECT 1`/`DELETE` keep `add_query`; redacted `…` and
    `SAVEPOINT` stay `observe` (never a group hint).
  Module doc updated; **no existing assertion relaxed or deleted** (`SELECT 1 → add_query` contract intact).
- `crates/blastit/tests/audit_group_hint.rs`: module-doc pointer to the protected owner; all 4 tests
  untouched.
- `docs/design/ma-lean-dx.md`: `:392` (§7.2 matrix) now states the oracle lives in
  `crates/blastit/tests/critical_check_engine.rs` and spells out both directions; `:131` (R33)
  carries the same pointer. `:136` is the action enum and `:315` a sample `--json` document —
  neither attributes an oracle to a suite.

## Acceptance evidence

### 1. Protected suite asserts the group hint, both directions

```console
$ grep -c 'add_group' crates/blastit/tests/critical_check_engine.rs
6
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_check_engine --locked
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test audit_group_hint --locked
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 2. It fails if the hint regresses (probe on a `/tmp` copy, since discarded)

Copied the tree to `/tmp/opencode/mb113-probe` (`.git`/`target` excluded), made `audit_v1` always
emit `add_query` for grantable denials there (replacing the `if is_connection_setup(..)`
`Fix::add_group` branch), and ran the protected suite with `CARGO_TARGET_DIR=/tmp/opencode/mb113-probe-target`:

```console
thread 'connection_setup_sql_names_the_connection_group_grant' panicked at crates/blastit/tests/critical_check_engine.rs:612:9:
assertion `left == right` failed: SELECT set_config('TimeZone', $?, false) runs on whichever route opens the connection
  left: Object {"action": String("add_query"), "to": String("[route.db].queries")}
 right: Object {"action": String("add_group"), "to": String("[group.connection.db].queries")}

thread 'only_connection_setup_names_a_group_and_never_a_refusal' panicked at crates/blastit/tests/critical_check_engine.rs:673:5:
assertion `left == right` failed: only connection setup names a group: Report { ... "sql": "SELECT set_config('TimeZone', $?, false)", "fix": Fix { action: "add_query", to: Some("[route.db].queries"), ... } ... }
  left: []
 right: ["SELECT set_config('TimeZone', $?, false)"]

test result: FAILED. 17 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
error: test failed, to rerun pass `-p blastit --test critical_check_engine`
```

Full log: `/tmp/opencode/mb113-probe-evidence.log`. Copy discarded (`ls -d` → both
`/tmp/opencode/mb113-probe{,-target}`: No such file or directory); `git status crates/blastit/src/`
is clean, so nothing leaked into the repo.

### 3. Standard-tier oracle intact, design doc truthful

```console
$ git diff --stat crates/blastit/tests/audit_group_hint.rs
 crates/blastit/tests/audit_group_hint.rs | 4 ++++     # doc lines only; 0 assertions touched
$ git diff crates/blastit/tests/critical_check_engine.rs | grep '^-' | grep -v '^---'
-//! one; an unrouted request that proposes a route, against an unrouted SQL
-//! statement that must never produce a route-less `missing_grant`.   # header rewrite only
$ uv run --no-project python -c '... "add_group" in l ...'
[four lines: R33 bullet (now names critical_check_engine.rs), R38 enum, §6 sample JSON, §7.2 matrix
 (now names critical_check_engine.rs and both directions)]
$ cargo fmt --all -- --check   # my two files: no diff (remaining diffs are MB.1.12's in-flight file)
```

### 4. Guard + gates

```console
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node MB.1.13
critical-guard: 51dced9..working tree, 12 changed file(s)
  M  crates/blastit/tests/critical_check_engine.rs  [authorized by crates/blastit/tests/critical_check_engine.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.1.13
$ make rs-lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.86s          # exit 0
$ make rs-doc
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.30s          # exit 0
$ uv run --no-project tools/plan.py validate
plan OK
```

`python/tests/critical/test_check_contract.py` untouched (`git status` clean) and unaffected —
its per-route twin contract is unchanged.

## Follow-ups

1. **Toolchain drift in `cargo fmt`:** the installed `rustfmt 1.11.0-stable (2026-11-09)` sorts
   `use` groups version-first (`{Value, json}`), while HEAD's blastit test files are committed in
   that order — fine — but the *editor auto-format hook* in this session re-sorted imports
   ASCII-first on write (exactly the LESSONS `rustfmt` vs `cargo fmt` bullet). Restored by hand;
   every future agent editing these files will hit it. Worth a note in `AGENTS.md`/LESSONS or
   disabling the hook for `crates/**`.
2. **`cargo fmt --all -- --check` is red on `crates/blastit-sqlpolicy/tests/critical_policy_surface.rs`**
   (MB.1.12's in-flight file at the time of this run) — not this node's path, left alone.
3. The group fix is asserted as a document, but no test applies it: a manifest with
   `groups = ["connection"]` + `[group.connection.db]` that makes the setup statement *granted*
   is still only proven by the audit's own probe, not by a test. Natural follow-up for
   `critical_check_engine.rs` if the lead wants the positive application twin too.
