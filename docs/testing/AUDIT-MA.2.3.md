# AUDIT MA.2.3 — CLI, unprivileged `dev`, framework-agnostic `check`

- **Date:** 2026-11-12
- **Scope:** `crates/blastit/src/{cli,config,check,audit_v1,infer_routes,dev_learn,supervise,stage0}.rs`,
  `crates/blastit-py/src/**`, `python/blastit/{_check_pytest,pytest_plugin,fold}.py`, and the
  tests listed under *Inventory*.
- **Commits audited:** `15931c6` (MA.2.3.2 CLI), `0ea35c0` (MA.2.3.3 dev), `865050e` (MA.2.3.4
  audit engine), `d73ded3` (MA.2.3.5 orchestrator), plus `d291078` (MA.2.3.1 Python/`Makefile`).
- **Requirements:** R1..R6, R9..R10, R17..R23, R27..R42; `ADR-0001/0003/0004/0005`;
  `docs/design/ma-lean-dx.md` §1.1, §1.2, §1.3, §4.2 E, §7.1, §7.2.
- **Not re-audited (closed by earlier reports):** R7/R8/R11/R24 manifest, R14..R16 ingress,
  R12/R13 abort, R26/R31 dbproxy — see `AUDIT-MA.2.1.md`, `AUDIT-MA.2.2.md`.

## Summary

**Verdict: BLOCKED at the epic gate — not for the feature, but for the CI job that proves it.**

The feature itself holds: the eleven protected Rust suites and all 49 Python critical tests pass
unprivileged and offline (`make test-critical` green), every §7.2 row for this epic has an allow
twin and a deny twin, and the R27 golden, the byte-exact §1.3 human block and the nine finding
kinds are real oracles that fail when the behaviour is removed.

Three things block the gate:

1. **The `pytest-plugin` CI job (`.github/workflows/ci.yml:222`, `pytest python/tests`) is red.**
   It is the *only* job that runs the standard Python tier and the real-PostgreSQL `check`
   orchestrator, and it collects `python/tests/critical/` too. Three independent causes, all
   reproduced below: 7/10 contract tests fail as soon as `pytest-django` is importable;
   `test_plugin_helpers.py:61` raises `NameError`; `test_pytest_plugin.py` expects two
   connection-setup statements and a warning-free manifest that Django 6.1 / MA.2.2.1 no longer
   produce.
2. **`blastit dev --learn` is not reliably exit `0` on a clean `SIGTERM`** (~9 % of runs: 3 of 33):
   a late `ctl.chan` write hits `EPIPE` during shutdown, `Supervisor::send_ctl` treats it as fatal,
   and the protected test (and §1.1) sees exit `1`.
3. **Two R29/R33/R35 rules survive deleting the guard they claim to protect**, proved by mutation
   below: `check --learn` still exits `0` after a refused statement, and `evaluate` can emit
   `add_query` carrying `…`.

`cargo-mutants` is **not installed** in this offline toolchain, so `make mutants-rust PKG=blastit`
cannot run (`error: no such command: mutants`; `~/.cargo/bin` holds only rustup's components). The
fallback is a 20-mutant anchored spot-check in a scratch copy, aimed at the branches the protected
suites do **not** name: **9/20 killed (45 %)**; `audit_v1` (the policy module) **5/9 (56 %)** —
below §7.1's 75 % bar.

## Method

1. Read every in-scope test file and the four modules behind it; classified each test against the
   `testing` tiers and the §7.2 positive/negative matrix.
2. Ran the real gates: `make test-critical`, then the CI job's own command
   (`pytest python/tests -q`) against a throwaway PostgreSQL 15, with `pytest`, `psycopg`,
   `django 6.1.3`, `pytest-django 4.14` and `pytest-xdist` — the versions the job installs.
3. Ran `crates/blastit/tests/critical_dev_and_fail_closed.rs` 33 times to measure flakiness.
4. Mutation fallback: `rsync` of the tree (no `target/`, no `.git/`) to
   `/tmp/opencode/blastit-ma23-audit`, a separate `CARGO_TARGET_DIR`, and a runner that applies
   exactly one anchor-checked replacement, runs the crate's tests (lib + critical integration
   targets) or the Python contract suite, then restores the file from memory. The working tree was
   never mutated. CLI mutants were scored twice: once against the protected matrix only, once
   against `--lib` too, because `cli.rs`'s unit tests are standard tier.

## Inventory & classification

| File | Tests | Class | Note |
|---|---|---|---|
| `crates/blastit/tests/critical_check_engine.rs` | 13 | critical | 13 inference rows, R27 golden, 9 kinds, byte-exact §1.3 |
| `crates/blastit/tests/critical_config_and_presets.rs` | 11 | critical | R1..R3/R6/R9/R10/R18/R23, CLI matrix, help |
| `crates/blastit/tests/critical_dev_and_fail_closed.rs` | 11 | critical | R17/R19..R22/R29; **2 tests flaky (F4)** |
| `crates/blastit-py/tests/critical_check_schema.rs` | 6 | critical | schema/count/bridge boundary; wrapper tested at Rust level |
| `python/tests/critical/test_check_contract.py` | 21 (+3 live-server) | critical | R4/R5/R30/R39; **red under pytest CI (F1)** |
| `python/tests/critical/test_fold_literals_and_atomic.py` | 22 | critical | R25/R29/R40/R41; all eight literal forms, `.tmp` safety |
| `python/tests/critical/test_stdlib_only.py` | 6 | critical | R40/R41; AST walk + two negative twins |
| `python/tests/test_check.py` | 10 | standard (PG CI) | real pytest/django/dbproxy + real PG; green |
| `python/tests/test_fold.py` | 117 | standard | only the CI pytest job runs it |
| `python/tests/test_plugin_helpers.py` | 53 | standard | **1 broken (F2)** |
| `python/tests/test_pytest_plugin.py` | 39 | standard | **7 red (F3)** |
| `python/tests/test_live_server.py` | 5 | standard | green with a real wheel |
| `python/tests/test_logs.py`, `test_wire_capture.py` | 33 + 24 | standard | CI pytest job |
| `crates/blastit/src/{cli,check,audit_v1,infer_routes,dev_learn,config,stage0}.rs` unit | 11/11/5/4/8/41/10 | standard | run by `cargo test --workspace` |
| `crates/blastit/tests/smoke.rs` | 31 | standard | end-to-end smoke of the binary |

Totals in scope: **90 protected critical**, ~380 standard, 0 docs-run tests (only the accepted
`docs_fences.rs` TOML fences), 0 e2e (MA.3 owns e2e).

**Fluff 0, broken-oracle 3 (F1..F3 are test-harness defects), misplaced 1 (noted), tautologies 0.**
Two assertions are near-duplicates, both cheap to keep:

- `test_stdlib_only.py:75-79` re-asserts `project.dependencies == []` (already at line 64) to prove
  `maturin` stays build-only; the extra claim is `build-system.requires` non-empty, so it is not a
  pure duplicate.
- `critical_config_and_presets.rs:223` (`both_presets_supply_their_four_values`) pins literals
  rather than reading `PRESETS` back, which is the right shape.

## Tier wiring — what runs where

| Test | `make check` | CI `rust`/`pytest-plugin` | Verdict |
|---|---|---|---|
| `crates/*/tests/critical_*.rs` | `rs-test` + `rs-test-critical` | rust job | run everywhere |
| `python/tests/critical/*` | `test-critical` (`unittest discover`) | **also collected by `pytest python/tests`** | green in `make check`, **red in CI (F1)** |
| `python/tests/test_check.py` | **never** | pytest job (needs PG) | the epic's real-orchestrator proof exists only in CI |
| `python/tests/test_{fold,plugin_helpers,pytest_plugin,live_server,logs,wire_capture}.py` | **never** (no `pytest` in the offline gate) | pytest job | red (F2, F3) for two nodes' worth of changes |
| `crates/blastit-manifest/tests/docs_fences.rs` | `rs-test` | — | 6 manifest fences parse; no CLI/doc snippet is executed |

`make check` is honest about Rust and the protected Python tier, but the standard Python tier has
**no local entry point at all** — which is why F2/F3 went unnoticed since `d291078`. Adding a
`py-test` target that runs `pytest python/tests` when pytest is available is worth a follow-up
(not a task here; the tier's real home is the CI job).

## Requirement → test map (this epic's rows only)

| Req | Protected oracle | Status |
|---|---|---|
| R1..R3, R6, R9..R10, R18, R23 | `critical_config_and_presets.rs` (CWD discovery, set-but-empty vars, both presets, overrides incl. `""`, log-format defaults, seccomp abort vs warning, `build` on `preload` only, CLI matrix with exact exit code + empty stdout, three help screens, `--version` refusal) | complete; `--port=0` and the `=` spellings live only in standard tier (acceptable: the exit-code contract is what §1.1 promises) |
| R17, R19..R22, R29 (dev) | `critical_dev_and_fail_closed.rs` (five roles, `privilege: "unprivileged-dev"`, 3-line banner ×2 + `EXPOSED`, `DATABASE_URL`/`BLASTIT_*`/`ENV_STRIP` stripped, per-route `.s.PGSQL`, `ENOENT` twin, `egress=false` `EPERM|ENOSYS` + twin, root removal, json has no `port`, `--learn` write, per-route attribution, unrouted-vs-first-route) | complete but **flaky (F4)** |
| R27..R28, R32..R38 | `critical_check_engine.rs` + `critical_check_schema.rs` (13 rows, golden route + 2 exact queries + bootable edit + clean re-audit, 9 kinds + interaction counts, byte-exact human block, unrouted never a `missing_grant`, `…`/`SAVEPOINT` → `observe`, schema versions) | covered except **F6** (`add_query` with `…`), **F7** (`no_observations` second clause), **F8** (`would_write` zero-request), **F9** (inference rules beyond the 13 rows) |
| R4..R5, R30, R39..R42 | `test_check_contract.py` (+ `test_fold_literals_and_atomic.py`, `test_stdlib_only.py`, `test_check.py` in PG CI) | contract covers exit 0/1/2/3 incl. pytest `5`, stdout purity vs `print()`+`os.write(1)`, one document, corrupt obs line, legacy entry point blocked, credential barrier, Django handshake/teardown twins, live-server fallback; **F5** (`--learn` refusal exit code) missing; **F1** makes the suite red in CI |
| R40/R41/R42 | `test_stdlib_only.py`, `Makefile critical-skip-check` (`#[\s*ignore]`, `needs_root(`, `pytest.mark.skip`, `pytest.skip(`, `importorskip(`, `unittest.skip`) | complete |

## Findings

### F1 — BLOCKER: the contract suite fails under the CI job (7 of 10)

`python/tests/critical/test_check_contract.py:699-705` builds the child environment from
`os.environ` + `PYTHONPATH=<project>/stub:<wheel>`, and the stub directory is what shadows
`pytest`. `install_django_stub()` (`:629`) only writes `pytest_django.py` for the Django twins, so
the seven non-Django tests let `_check_pytest._refuse_unhandshaken_django()`
(`python/blastit/_check_pytest.py:765`) import **the real** `pytest_django`, whose
`@pytest.fixture` decorators then run against the stub `pytest` module:

```console
$ DATABASE_URL=… PYTHONPATH=<stage>:python uv run --no-project --with pytest --with psycopg \
    --with django --with pytest-django --offline pytest python/tests/critical/test_check_contract.py -q
7 failed, 17 passed
  File ".../_check_pytest.py", line 765, in _refuse_unhandshaken_django
    import pytest_django
  File ".../pytest_django/fixtures.py", line 56, in <module>
    @pytest.fixture(scope="session")
AttributeError: module 'pytest' has no attribute 'fixture'
→ blastit check exits 3 instead of 0/2/1
```

Green in `make test-critical` only because that interpreter has no `pytest_django` on its path.
Fix in the harness (always write the empty `pytest_django.py` stub for every `Project`, keeping the
dedicated "pytest-django is configured" twin that uses the real fail-closed path), not in
production code.

### F2 — BLOCKER: `test_plugin_helpers.py:61` `NameError`

`d291078` (MA.2.3.1) replaced `pytest.raises(Exception)` with `pytest.raises(psycopg.ProgrammingError)`
while the module-level guard at `:25` is a bare `pytest.importorskip("psycopg")`, which does not
bind the name. CI installs psycopg, so the test runs and raises:

```console
E       NameError: name 'psycopg' is not defined
python/tests/test_plugin_helpers.py:61: NameError
```

Bind it (`psycopg = pytest.importorskip("psycopg", …)` or `import psycopg` after the skip) and drop
the misleading `# noqa: F821 - importorskip binds it`.

### F3 — BLOCKER: `test_pytest_plugin.py` fixture drift (7 tests)

Two independent expectations no longer hold:

- `assert len(baseline.setup_statements()) == 2` (`:349`): Django 6.1 sends `set_config('TimeZone', …)`
  only when the server's `TimeZone` differs from `settings.TIME_ZONE`
  (`django/db/backends/postgresql/base.py:_configure_timezone`), so only
  `SET application_name = 'blastit-test'` is observed. Cascade:
  `test_the_report_counts_what_the_manifest_does_not_cover`, `…terminal_summary…`,
  `test_strict_fails_the_session`, `…report_is_enough…`, `…more_than_one_proxy_needs_a_mapping`,
  `test_xdist_workers_are_merged`, `…not_merged_twice`.
- `report["manifest"]["warnings"] == []`: since `80638f1` (MA.2.2.1) the fixture's
  `python/tests/project/blastit.toml:116` (`egress = true`, no address/port) emits
  `route "note-cache": egress = true opens any outbound address and port`.

Fix the fixture/pytest-django expectations, not the warning (the warning is the documented R11
behaviour).

### F4 — BLOCKER: `blastit dev` clean shutdown sometimes exits 1

33 runs of the protected suite: **3 failures** in two different tests
(`learn_writes_routes_and_queries_on_sigterm:1100`,
`first_declared_route_keeps_all_queries_when_unrouted_requests_exist:1299`), both on
`assert_eq!(status.code(), Some(0))`:

```console
19:44:06.007 INFO  shutdown    drain
19:44:06.008 INFO  process.exited a  pid=1503776 uid=1000  status=0
19:44:06.008 ERROR app.output  stderr  ctl.chan: Broken pipe (os error 32)
```

`Supervisor::send_ctl` (`crates/blastit/src/supervise.rs:759-783`) already returns quietly when the
ctl fd is gone, but maps `EPIPE` to `self.fail("ctl.chan: {err}")` → `_exit(1)`. During `drain` the
scheduler is legitimately gone; a late `KILL_VIEW`/`EXITED` ack must not change a clean exit code.
This is both a §1.1 contract violation (`--learn` clean exit is `0`) and a gate-flaking critical
test. Treat `EPIPE`/`ECONNRESET` like the `None` arm while shutting down, and add a deterministic
regression (e.g. a supervisor unit test with a closed ctl fd, or drop the `--learn` path's ctl send
after `drain`).

### F5 — MAJOR: `check --learn` exits `0` after a refused statement

`crates/blastit/src/check.rs:940` is the only place that turns a refusal into an exit code:

```rust
let clean = plan.requests > 0 && plan.refused.is_empty() && failure.is_none();
```

Deleting `plan.refused.is_empty()` keeps every test green (`critical_check_engine.rs` only pins
`LearnPlan::would_write`, `test_check_contract.py` only pins "writes nothing when the suite
fails"). §1.1 promises exit `1` for "`--learn` refused a statement". Oracle to add: a contract run
whose suite sends an inline-literal statement → exit `1`, `"wrote": false`, manifest byte-identical,
and the refusal on stderr.

### F6 — MAJOR: `evaluate` can emit `add_query` carrying `…` (R33)

`audit_v1.rs:661-673` guards the `add_query` branch with `!normalized.contains('…')`; removing the
guard survives the whole crate **and** the Python contract, because `a_redacted_literal_and_a_
savepoint_are_observe_never_add_query` feeds a `Refused` verdict, never a lying `Query` verdict
with a redaction. The twin for `plan_learn` exists (`an_inline_literal_or_zero_requests_never_writes`,
"a verdict that wrongly calls a redaction grantable"); the engine has no twin. Add the same line
with `verdict.kind = "query"` + `normalized` containing `…` and assert `fix.action == "observe"`.

### F7 — MAJOR: `no_observations` second clause is unprotected (R35)

`audit_v1.rs:695` fires when `exercised == 0 || (!proxies.is_empty() && statements == 0)`. Dropping
the second clause survives, because the only empty-suite test has zero requests *and* zero
statements (so the first clause covers it) and `ingress_findings_have_positive_twins` makes
requests without SQL but only asserts the *absence* of the ingress kinds. §4.2 E.3 names the second
clause explicitly ("a suite that exercised routes but no proxy statement").

### F8 — MINOR: `would_write` zero-request rule is unprotected

`audit_v1.rs:1171`'s `self.requests > 0` is untested in isolation: the existing zero-request test
also has zero routes/queries, so the last conjunct alone decides. A run with 0 HTTP requests but an
unrouted SQL statement (the gate can observe SQL without a request, and `plan_learn` takes paths
from SQL too) must still produce `"wrote": false`.

### F9 — MINOR: inference rules beyond the 13-row table

`infer_routes.rs` documents three rules the fixture table does not pin: a root-level UUID is a
placeholder (`/550e8400-…` → `route`, not a UUID-shaped name), runs of separators collapse to one
`-` (`/a..b` → `a-b`), and the disambiguation reaches `-3`. Mutants for all three survive.

### F10 — MINOR: two CLI branches the protected matrix does not pin

`parse_check` refuses the forwarded `--blastit-manifest` only by exact word; the
`--blastit-manifest=<path>` spelling survives the protected matrix (exit `3` from pytest's usage
error instead of the documented `2`). The `dev` unknown-word arm survives too, but it is
**equivalent** for the contract (`--bogus` still exits `1`, empty stdout; only the message differs).

### F11 — MINOR: display-only branches unpinned

`audit_v1::plural`'s singular (`1 problem`, `1 statement`, `1 request`) never runs in the protected
tier (the human-block tests have 0, 4 and 11 findings), and `transaction_label`'s `END`/`START`
aliases are unasserted (`END` → `END` survives; harmless, arguably more accurate).

## Mutation spot-check

`cargo-mutants` unavailable (see Summary). 20 mutants, 9 killed (45 %); excluding three
practically-equivalent mutants (`END` label, `dev` unknown-word message, unreadable `http_obs`),
9/17 (53 %).

| # | Module | Mutation | Verdict |
|---|---|---|---|
| 1 | `audit_v1` | body-sample gate `body_len > 0` → `>= 0` | **killed** |
| 2 | `audit_v1` | drop the `…` veto on `add_query` | survived → F6 |
| 3 | `audit_v1` | tx-control denial gets `add_query` | **killed** |
| 4 | `audit_v1` | `END` no longer labelled `COMMIT` | survived (cosmetic) |
| 5 | `audit_v1` | `unexercised_route` not suppressed on an empty suite | **killed** |
| 6 | `audit_v1` | `no_observations` drops the zero-statement clause | survived → F7 |
| 7 | `audit_v1` | dead route grant ignores group coverage | **killed** |
| 8 | `audit_v1` | `plan_learn` grants a `…` statement | **killed** |
| 9 | `audit_v1` | `would_write` drops `requests > 0` | survived → F8 |
| 10 | `infer_routes` | root-level UUID not dynamic | survived → F9 |
| 11 | `infer_routes` | no `-` run collapsing | survived → F9 |
| 12 | `infer_routes` | trailing trim dropped | **killed** |
| 13 | `infer_routes` | `-N` suffix jumps by two | survived → F9 |
| 14 | `cli` | `--dry-run` without `--learn` accepted | **killed** (protected matrix **and** unit) |
| 15 | `cli` | forwarded `--blastit-manifest=<v>` accepted | survived → F10 |
| 16 | `cli` | `--port 0` accepted | **killed** (unit test only) |
| 17 | `cli` | `dev` unknown word falls through | survived (equivalent) |
| 18 | `check` | `--learn` refusal still exits `0` | survived → F5 |
| 19 | `check` | failed suite → `1` instead of `3` | **killed** (contract: 6 tests) |
| 20 | `check` | unreadable `http_obs` treated as empty | survived (branch is practically unreachable: the runtime dir is `0700` and the gate creates the file) |

Per module: `audit_v1` 5/9 (56 %), `infer_routes` 1/4, `cli` 2/4, `check` 1/3. The policy module is
below §7.1's 75 % bar; every meaningful survivor is tasked or listed above.

## Gates run

```console
$ make test-critical
… 11 protected Rust suites ok …
python -m unittest discover -s python/tests/critical
Ran 49 tests in 3.951s
OK

$ for i in $(seq 33); do cargo test -p blastit --test critical_dev_and_fail_closed; done
3 failed runs (F4), 30 green

$ DATABASE_URL=postgresql://…@127.0.0.1:55439/audit PYTHONPATH=<wheel>
  pytest python/tests -q          # the CI job's command, real PG + django 6.1.3
19 failed, 302 passed, 1 skipped
  critical/test_check_contract.py   7 failed   (F1)
  test_plugin_helpers.py            1 failed   (F2)
  test_pytest_plugin.py             7 failed   (F3)
  test_live_server.py               3 failed   (artefact of a PYTHONPATH stage:
                                                the fixture re-execs `python -m blastit`
                                                with a scrubbed env; green with an installed wheel)
```

`cargo mutants --version` → `error: no such command: mutants`.

## Gaps → tasks

| # | Severity | Gap | Task |
|---|---|---|---|
| F1+F2 | BLOCKER | pytest-plugin CI job red (contract stub, psycopg binding) | MA.2.3.6 |
| F3 | BLOCKER | `test_pytest_plugin.py` fixture drift (Django 6.1 setup statement, `egress` warning) | MA.2.3.7 |
| F4 | BLOCKER | `dev` clean `SIGTERM` → exit 1 on a late `ctl.chan` `EPIPE`; flaky protected test | MA.2.3.8 |
| F5 | MAJOR | `--learn` refused statement still exits `0` (R29) | MA.2.3.9 (after MA.2.3.6) |
| F6+F7+F8 | MAJOR | engine rules unprotected: `add_query` with `…`, `no_observations` zero-statement clause, `would_write` zero-request | MA.2.3.10 |
| F9, F10, F11 | MINOR | inference extras, CLI `=` spelling, singular/alias rendering | proposed, not filed |

## Follow-ups

- **`make check` has no Python standard tier.** There is no `py-test` target, so 117 `test_fold.py`
  tests and the 39 plugin tests run only in CI. A `make py-test` (skipping when `pytest` is absent,
  or via `uv run --no-project --with pytest`) would have caught F2/F3 at `d291078`.
- **`_blastit.check_audit` / `_blastit.plan_learn` have no Python caller.** The bridge exists per
  §4.2 E, is exercised by `crates/blastit-py` unit tests, and `probe_ingress` is used by the gate;
  the other two are currently dead API surface. Decide whether `pytest_plugin`'s report should use
  them (removing a second implementation of the same counts) or drop them.
- **Undeclared-proxy observations are silently dropped** in `evaluate` (`audit_v1.rs:637-641`):
  a statement on a proxy the manifest never declares counts nowhere and yields no finding. The nine
  kinds have no slot for it, and `check.rs` never binds a listener for an undeclared proxy, so this
  is defence-in-depth only; document it rather than invent a tenth kind.
- **`resolve_targets` oracles live in the standard tier** (`check.rs` unit tests) but are
  redundantly protected: if resolution stopped, the contract's `test_a_denied_statement_exits_one_
  with_one_document` would see an unrouted statement and fail. No task.
- **`test_live_server.py` is green only with an installed wheel**, as its own docstring says; the
  three failures in the audit run were the stage's `PYTHONPATH` not surviving the fixture's
  scrubbed child environment, not a product defect.
