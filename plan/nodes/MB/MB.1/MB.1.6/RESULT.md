# Result MB.1.6 — Missing manifest first, group hint, no allocation for policy-less bodies

**STATUS: done** — the three `MA`-gate defects are fixed independently, each with its own oracle, and the full gate is green.

## Summary

- **(a) Missing `./blastit.toml` is named before `BLASTIT_APP`** at both entry points (`stage0::run` and `stage0::run_dev`): the manifest is read first, so an empty directory says `./blastit.toml: No such file or directory (os error 2)` instead of blaming a setting. A manifest that exists but declares no `app` still fails with `BLASTIT_APP: the WSGI callable … is required`. Pinned by one **new** test in the protected suite (the existing 11 are untouched).
- **(b) `blastit check` names a group grant for connection-setup SQL**: a grantable `missing_grant` whose statement is the driver's own session setup now carries `{"action":"add_group","to":"[group.connection.<proxy>].queries"}` instead of a per-route `add_query`. Every other grantable statement keeps `{"action":"add_query","to":"[route.<proxy>].queries"}`; transaction control and redacted literals stay `observe`.
- **(c) `Scheduler::plan_request_body` allocates nothing for a policy-less route**: it returns `Ok(BodyPlan::AllowEmptyOnly)` before building the method or reading any header. Measured: **69.02 → 66.02 allocations per request** on the same head (the three copies), proven by the existing `head_allocs.rs` counting allocator.
- **(d) `docs/troubleshooting.md`** reconciled: the missing-file row is the empty-directory symptom, the `BLASTIT_APP` row now means "the manifest exists but declares no `app`", and `blastit check` gained one connection-setup row pointing at `[group.<name>.db]` + top-level `groups = ["..."]`. That row now lists all three statement shapes the code classifies as session setup — `SELECT set_config(…)`, `SET …` **and** `SHOW …` (`audit_group_hint.rs` pins the `SHOW` case) — still one line, no new row.

**One deliberate narrowing of (b) (contract, not preference):** the hint uses `blastit_sqlpolicy::is_introspection_setup_sql` **plus** the exact `KNOWN_STATIC_SETUP_SQL` match, because `is_introspection_setup_sql("SELECT 1")` is `true` (a constant `SELECT` is replayable setup). `SELECT 1` must keep `add_query` — `python/tests/test_check.py:224` and `python/tests/critical/test_check_contract.py:989` assert exactly that, and both are outside this node's `paths`. `SET …`/`SHOW …` are still classified through `is_introspection_setup_sql`, so multi-statement text (`SET x; DROP TABLE t`) never gains the hint.

## Evidence

Environment: `release/0.2.0`, unprivileged host uid 1000, `CARGO_NET_OFFLINE=true`, uv CPython 3.14.10.

### 1. Missing manifest named first, at both entry points

Three legs, one per entry point / privilege state; the **stream** differs per leg and is stated with each:

| leg | exit | stdout | stderr | verdict |
|---|---|---|---|---|
| `blastit dev`, empty dir | 1 | **0 B** | `blastit dev: ./blastit.toml: No such file or directory (os error 2)` | names the manifest |
| production, uid 1000 | 1 | 2046 B, `app.output` events only | **0 B** | capability refusal only, no `BLASTIT_APP` |
| production, root (`sudo -n`) | 1 | 1017 B, `app.output` events only | **0 B** | names the manifest, no `BLASTIT_APP` |

Stream contract (confirmed by these runs): `blastit dev` writes startup errors to **stderr**; production
surfaces them as `app.output` events on **stdout** (`stage0.rs:424`), never on fd 2 — so a production
assertion belongs on stdout, not `err.txt`.

```console
$ CARGO_NET_OFFLINE=true cargo build --locked -p blastit
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.29s

$ tmp=$(mktemp -d) && ( cd "$tmp" && env -u BLASTIT_APP -u BLASTIT_MANIFEST "$OLDPWD/target/debug/blastit" dev ) >out.txt 2>err.txt
exit=1
$ grep -q 'blastit.toml' err.txt && ! grep -q 'BLASTIT_APP' err.txt && test ! -s out.txt && echo "dev names the manifest"
dev names the manifest
out.txt bytes: 0 | err.txt bytes: 68
$ cat err.txt
blastit dev: ./blastit.toml: No such file or directory (os error 2)
```

Leg 2 — an unprivileged production boot is refused by the capability gate (`stage0.rs:360`) **before**
`Config::from_env` (`stage0.rs:416`), so the refusal is the only output and the manifest is never reached:

```console
$ tmp=$(mktemp -d) && ( cd "$tmp" && env -u BLASTIT_APP -u BLASTIT_MANIFEST "$OLDPWD/target/debug/blastit" ) >out.txt 2>err.txt
exit=1
out.txt bytes: 2046 | err.txt bytes: 0
$ grep -q 'cap_' out.txt && ! grep -q 'BLASTIT_APP' out.txt && echo "unprivileged refuses at the capability gate"
unprivileged refuses at the capability gate
$ head -2 out.txt
{"ts":"…","level":"info","event":"boot","component":"init","stage":"stage0","seccomp":true,"preset":null}
{"ts":"…","level":"error","event":"app.output","component":"init","stream":"stderr",
 "line":"init runs as uid 1000 and is missing cap_chown, cap_dac_override, cap_fowner, cap_kill, cap_setgid, cap_setuid.","part":1,"parts":1}
```

Leg 3 — run as the identity that can reach the configuration (root), the reorder is observable end to end:

```console
$ if sudo -n true 2>/dev/null; then tmp=$(mktemp -d); ( cd "$tmp" && sudo -n env -u BLASTIT_APP -u BLASTIT_MANIFEST "$OLDPWD/target/debug/blastit" ) >out.txt 2>err.txt; grep -q 'blastit.toml' out.txt && ! grep -q 'BLASTIT_APP' out.txt && echo "root names the manifest"; else echo "SKIP root leg: no passwordless sudo (CI smoke (root) covers it)"; fi
exit=1
root names the manifest
out=1017 err=0
$ grep -o '"line":"[^"]*blastit\.toml[^"]*"' out.txt
"line":"./blastit.toml: No such file or directory (os error 2)"
```

The other order still holds — a manifest that exists and declares no `app` still blames `BLASTIT_APP`,
and never claims a file is missing (root, same stdout/0-byte-stderr shape):

```console
$ printf 'version = 1\n\n[[route]]\nname = "home"\npath = ["/"]\n' > "$tmp/blastit.toml"
$ ( cd "$tmp" && sudo -n env -u BLASTIT_APP -u BLASTIT_MANIFEST "$OLDPWD/target/debug/blastit" ) >o2.txt 2>e2.txt
exit=1 out=1026 err=0
$ grep -o '"line":"BLASTIT_APP[^"]*"' o2.txt
"line":"BLASTIT_APP: the WSGI callable (`module:attribute`) is required"
$ grep -c 'No such file' o2.txt e2.txt
o2.txt:0
e2.txt:0
```

The unprivileged, deterministic half of the contract is pinned in the protected suite (item 2) through
`blastit dev`, which reaches its configuration without any capability and shares the reordered code path.

### 2. Pinned in the protected suite (one new test; the existing 11 stay)

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_config_and_presets --locked
running 12 tests
test a_boot_event_round_trips_the_build_and_never_a_port ... ok
test a_set_but_empty_variable_wins_over_the_manifest ... ok
test a_set_variable_overrides_a_preset_even_when_empty ... ok
test an_empty_directory_names_the_missing_manifest_before_the_app ... ok
test an_environment_driven_abort_is_a_real_exit_code_and_names_its_cause ... ok
test both_presets_supply_their_four_values ... ok
test cli_matrix_rejects_each_bad_command_line ... ok
test cwd_manifest_is_discovered_with_no_upward_search ... ok
test dev_and_check_ignore_an_exported_preset ... ok
test log_format_defaults_to_json_for_serve_and_pretty_for_dev_and_check ... ok
test seccomp_zero_aborts_with_a_preset_and_warns_without_one ... ok
test top_level_help_lists_the_three_commands_and_the_presets ... ok
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
```

The new test drives the real binary twice in a fresh sandbox: an empty directory must name `blastit.toml`
and must **not** mention `BLASTIT_APP` (stdout empty, exit 1); a manifest without `app` must name
`BLASTIT_APP` and must not claim anything is missing. `BLASTIT_MEM_HIGH_WATERMARK=100` keeps the dev
memory refusal out of the way.

### 3. Group hint, with its per-route twin

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test audit_group_hint --locked
running 4 tests
test transaction_control_and_a_redacted_literal_never_name_a_group ... ok
test connection_setup_sql_names_a_group_grant ... ok
test a_plain_statement_keeps_the_per_route_grant ... ok
test a_set_and_a_show_are_session_setup_too ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_check_engine --locked
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

The new suite compares the whole serialised `fix` object (`serde_json::to_value(finding)["fix"]`), so an
extra field or a renamed `to` fails it:

- `SELECT set_config('TimeZone', $?, false)` → `{"action":"add_group","to":"[group.connection.db].queries"}`
- `SET search_path = public`, `SHOW standard_conforming_strings` → the same group hint
- `SELECT 1`, `DELETE FROM "t" WHERE "t"."id" = $?` → `{"action":"add_query","to":"[route.db].queries"}` (the twin)
- `SAVEPOINT s1` (transaction control) and a redacted `…` literal → `{"action":"observe"}`, and no finding
  in that report carries `add_group`

### 4. Zero header allocations on a policy-less route

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test head_allocs --locked -- --nocapture
running 2 tests
allocations per 8-header request: decoded 47, raw 6
test a_raw_head_is_forwarded_for_far_fewer_allocations_than_a_decoded_one ... ok
allocations per head: no policy 66.02, body = "none" 69.02
test a_policy_less_head_is_planned_without_copying_its_headers ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
```

(`--nocapture` is a libtest flag, so it needs the `--` separator; without it cargo rejects it.)

**Before/after, measured (not asserted by hand):**

| tree | no `[route.ingress]` | `[route.ingress] body = "none"` | delta |
|---|---|---|---|
| before the fix (early return removed) | 69.02 | 69.02 | 0 — **the oracle fails** |
| after the fix | **66.02** | 69.02 | **3 per request** |

The three are exactly the strings the old path built before consulting the policy: `method`
(`String::from_utf8_lossy(..).into_owned()`) plus the `content-type` and `content-encoding` copies from
`head_header_text`. Both routes' plans are `AllowEmptyOnly`, so nothing else in the request differs.

Method: two fresh `World`s whose manifests differ only in the `[route.ingress]` table, each warmed up and
then measured on its second 100-request pass. The **first** measured pass in a fresh world is cheaper
(23.6 vs 66.0 per request) because the client's flow tables and the scheduler's maps are still growing, so
only a second pass is comparable across worlds; this is in the test's doc comment. `ARMED` moved from a
global `AtomicBool` to a thread-local `Cell<bool>` so the two tests in the file (which run in parallel, on
different threads) cannot disarm each other's measurement — still one global allocator.

Runs are stable: 3/3 consecutive runs printed `66.02` / `69.02` after the fix, and the reversed-order run
before it printed `69.02` / `69.02` (a failing assertion).

### 5. Docs + full gate

```console
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
exit=0
guard: OK (24 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 20 changed file(s)  # report only: make check has no --node
  M  crates/blastit/tests/critical_config_and_presets.rs  [VIOLATION: not covered by any node path]
  report only: 1 breaking change(s) would fail an enforced run
cargo fmt --all -- --check                     # clean
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked   # clean
cargo test --workspace (unit/integration/doc)  # all green — 85 `test result: ok` lines, 0 `failures:`
python -m unittest discover -s python/tests/critical
Ran 50 tests in 4.100s / OK
ruff check + ruff format --check       # All checks passed!
docs-unit                              # 46 passed, 57 subtests passed
zensical build --strict                # No issues found
wheel built: blastit-0.1.1-cp314-cp314-manylinux_2_39_x86_64.whl  sha256 85ceae9e12e6 · installed package verified
docs fences: 38 fences: 26 executed, 12 accounted, 0 failed · 21.0s
plan.py validate: plan OK

$ uv run --no-project tools/critical_guard.py --base 9152197 --node MB.1.6
critical-guard: 9152197..working tree, 20 changed file(s)
  M  crates/blastit/tests/critical_config_and_presets.rs  [authorized by crates/blastit/tests/critical_config_and_presets.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.1.6   # exit=0

$ uv run --no-project tools/plan.py validate
plan OK
```

Log: `/tmp/opencode/mb16/make-check-2.log` (2673 lines). The only note in the log's first block is the
expected `critical-guard` "report only" line: `make check` runs the guard without `--node`, so an authorized
protected edit reads as a violation there. The enforced form (`--base 9152197 --node MB.1.6`, above) is the
gate that decides, and it passes.

## Files

- `crates/blastit/src/stage0.rs` — read the manifest before checking `app`, at both `run` and `run_dev`.
- `crates/blastit/src/audit_v1.rs` — `Fix::add_group`, `is_connection_setup`, the `missing_grant` branch,
  and the module/`Fix` docs for the new closed-set member.
- `crates/blastit-scheduler/src/scheduler.rs` — `plan_request_body` returns `AllowEmptyOnly` before the
  method/header copies; `plan_body` now receives `Some(policy)`.
- `crates/blastit/tests/critical_config_and_presets.rs` — **+1 test only** (11 existing untouched).
- `crates/blastit/tests/audit_group_hint.rs` — new: the group hint and its three twins.
- `crates/blastit-scheduler/tests/head_allocs.rs` — extended: thread-local arming + the policy-less oracle.
- `docs/troubleshooting.md` — the two reordered rows, one `blastit check` row, and `SHOW …` added to that
  row's statement list (it is classified as session setup too).

## Follow-ups (outside this node's `paths`)

- **`docs/ci.md`** (`:74-75`) still closes `fix.action` at eight values, while the engine now also emits
  `add_group` (`to = "[group.connection.<proxy>].queries"`); `R38` in `docs/design/ma-lean-dx.md` lists the
  same eight. Owned by node **MB.1.10** — deliberately not touched here.
- **`docs/knowledge/LESSONS.md`** (outside `paths`): worth one bullet — *a fresh scheduler `World`'s first
  100-request pass allocates ~23.6 vs ~66.0 per request afterwards, so allocation oracles must compare
  second passes in separate worlds, and a per-thread armed flag (not a global) is required as soon as one
  `head_allocs`-style binary has two tests.* Also: *production stage-0 errors are `app.output` events on
  **stdout**, so a "startup error on stderr" assertion only holds for `blastit dev`; an unprivileged
  production boot never reaches `Config::from_env` because the capability gate precedes it.*
