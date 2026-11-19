# MB.40.2 — the four live P0/High bugs (review round 2)

Every item below is one reviewer finding: what was wrong, what changed, and the
test that now proves it.

## 1. [BLOCKER] the coverage gate was `cfg(coverage)`, which rustc never sets

**Finding** `rustc --print cfg -C instrument-coverage` prints **no** `coverage`
(reproduced: exit 1, empty) — that spelling is a nightly compiler-internal switch.
So `flush()` compiled to an empty function in exactly the instrumented build that
needs it, profiles were still lost on `_exit`, and both tests were green either
way.

**Change**
* `crates/blastit/build.rs` owns the gate: the `coverage` cargo feature **or**
  `BLASTIT_COVERAGE=1` (for a coverage target that already passes `RUSTFLAGS`)
  emits `cargo:rustc-cfg=coverage`, always emits
  `cargo:rustc-check-cfg=cfg(coverage)`, and exports its decision as
  `cargo:rustc-env=BLASTIT_COVERAGE_GATE=on|off`.
* `crates/blastit/Cargo.toml` gains `coverage = []`, documented as
  “only together with `-C instrument-coverage`”.
* `crates/blastit/src/coverage.rs` keeps `#[cfg(coverage)]` but the module doc
  now states where that cfg comes from, and the extern declaration is a `//`
  comment (rustdoc generates nothing for an `extern` block, so a `///` there was
  an `unused_doc_comments` warning in the coverage build itself).
* The workspace `[workspace.lints.rust] unexpected_cfgs` entry from round 1 is
  gone: `build.rs` declares the cfg for the one crate that uses it.

**Tests**
* `coverage::the_coverage_gate_is_the_build_scripts_decision` — asserts
  `cfg!(coverage)` equals `env!("BLASTIT_COVERAGE_GATE") == "on"`, i.e. the cfg
  that selects the extern block *is* the build script's decision. This fails if
  the gate ever goes back to rustc's non-existent cfg.
* `coverage::the_instrumented_build_references_the_profile_writer`
  (`#[cfg(coverage)]`) — takes the address of `__llvm_profile_write_file`; it
  does not compile unless the gate selected the extern block, and does not link
  unless the runtime exports the symbol. Proves the caller is genuinely
  referenced, not an empty body.

**Evidence (real instrumented build, separate target dir)**
```
$ CARGO_TARGET_DIR=/tmp/opencode/cov-target BLASTIT_COVERAGE=1 \
    RUSTFLAGS="-C instrument-coverage" cargo build -p blastit --locked
    Finished `dev` profile … (no warnings)
$ nm /tmp/opencode/cov-target/debug/blastit | grep -c __llvm_profile_write_file
1
00000000018eb740 t __llvm_profile_write_file

$ … cargo test -p blastit --lib --locked coverage::
running 4 tests
test coverage::tests::the_coverage_gate_is_the_build_scripts_decision ... ok
test coverage::tests::the_instrumented_build_references_the_profile_writer ... ok
test coverage::tests::inherit_adds_nothing_when_the_build_was_not_given_a_profile_file ... ok
test coverage::tests::the_profile_variable_is_the_one_rustc_reads ... ok
test result: ok. 4 passed; 0 failed
```

## 2. [MAJOR] normalize's shape tails (`IN ($?, ...)`, `VALUES ($?), ...`)

**Finding** `restore_parameters` restored `$?` and `…` but left the shape
ellipsis, so the two shapes Django sends on almost every list page
(`__in`/`prefetch_related`) and on `bulk_create` still failed to parse → false
`missing_grant`.

**Change** `crates/blastit-sqlpolicy/src/normalize.rs`: `restore_parameters`
now runs `expand_tails` over each code region. A `, ...` tail expands into one
more element: after a placeholder (`IN ($?, ...)`) it gains `$1`; after a `)`
(`VALUES ($?, $?), ...`) it repeats the group that closes there. Any other `...`,
and any `...` inside a literal or comment, is left untouched, and the expansion
never widens the access set — a tail only ever adds parameters.

**Tests**
* `normalize::restore_parameters_gives_the_parser_a_placeholder_and_a_literal_shape`
  — both tails (`IN ($?, ...)` → `IN ($1, $1)`, `VALUES ($?), ...` →
  `VALUES ($1), ($1)`, two-column row run), a non-tail `...`, a `...` in a
  literal.
* `sqlpolicy::a_table_grant_checks_the_observation_spelling_of_a_statement`
  — **ALLOW** for `IN ($?, ...)` and `VALUES ($?), ...` under
  `select/insert items(id)`; **DENY** twins with an ungranted column in the same
  two shapes (`select items(id)` must never cover `items.secret`), plus the wire
  forms (`= $1`, `IN ($1, $2, $3)`).
* `audit_v1::a_table_grant_allows_a_parameterised_statement_the_check_saw_normalised`
  (allowed `= $?` *and* `IN ($?, ...)`) and
  `audit_v1::a_table_grant_still_denies_an_ungranted_column_on_the_same_statement`
  (denied `= $?` *and* `IN ($?, ...)`).

## 3. [MAJOR] `dev --learn` could attribute a probe's SQL to the one legitimate view

**Finding** the path filter kept `/.env` out of `views` but not out of `grants`:
dbproxy tags fallback statements `ObsTarget::Routed { route: LEARN_FALLBACK_ROUTE }`
with no path, so `fallback_owner` gave a probe request's statement to the single
view a legitimate request discovered.

**Change** `crates/blastit/src/dev_learn.rs::edits` computes
`unattributable` = any `self.unrouted` path that `--learn` refuses (traversal,
dotfile) or a `static` mount answers. The fallback owner is only used when the
whole batch is attributable; otherwise no fallback statement is granted and
`edits` returns a note that `finish` prints on stderr
(`learn: an unrouted request was a path --learn never turns into a view …`), which
never echoes the probe itself.

**Test** `dev_learn::a_scanner_probe_never_donates_its_sql_to_the_one_proposed_view`
— two unrouted requests (`/notes/` + `/.env`) and two fallback statements: the
probe proposes no view, **no** fallback statement is granted (so `SELECT 2`
cannot land in `notes`), and the note is emitted. The existing
`one_proposed_route_receives_its_own_fallback_statement`-style test still shows
the legitimate single-view case granting its query, with `note == None`.

## 4. [MAJOR] the Python gate test double-closed the adopted control fd

**Finding** `_check_pytest._control()` adopts `BLASTIT_CHECK_CTL_FD` with
`socket.socket(fileno=…)`, i.e. it takes ownership. The tests also handed over
`suite_end.fileno()` and later closed `suite_end`, so teardown closed one fd
twice (`FAILED (errors=3)`).

**Change** every test hands the gate its **own** descriptor
(`os.dup(suite_end.fileno())`), so the two objects never share an fd, and
`_restore` closes the adopted channel under `contextlib.suppress(OSError)` as a
backstop.

**Evidence**
```
$ PYTHONPATH=python uv run --no-project python -m unittest discover \
    -s python/tests -p 'test_check_pytest_gate.py'
.....
Ran 5 tests in 0.02s
OK
```

## 5. [MINOR, behavioural] the `note_self_write` call was decorative

**Finding** it was taken before the write and no observation batch is consumed
before `_exit`, so it could not suppress anything; deleting it left the test
green.

**Change** deleted — `Policy::note_self_write`, `Reload::note_self_write`, the
`Supervisor::finish` call and the round-1 unit test are gone, so no dead API and
no decorative call remain. The behaviour the item cares about is already covered
end to end by the protected
`crates/blastit/tests/critical_dev_reload.rs::learn_skips_the_manifest_cycle_and_still_reloads_views`
(a `Blastfile` save under `--learn` warns once and a later **source** save still
reloads the views), which stays untouched and green.

## 6. [MINOR] stale `plan_learn` doc comment

**Change** `crates/blastit/src/audit_v1.rs::plan_learn`'s doc now says every
observed statement is mapped to its view — the route a routed request resolved
to, or the view this run proposes for an unrouted one — with a declared view
proposing only what `check`'s own allowlist reports missing, and probe/static
paths proposing neither a view nor a grant.

## Verification

```
$ make fast-check            # see tail below
$ PYTHONPATH=python uv run --no-project python -m unittest discover \
    -s python/tests -p 'test_check_pytest_gate.py'
$ cargo test -p blastit -p blastit-sqlpolicy -p blastit-manifest -p blastit-py \
    --features blastit/python --locked
```

## Follow-ups

* `python/tests/runnable_docs/fixtures/hello/` is run page-by-page by the docs
  harness, so no `tests/` directory was added there; the store-less `check` path
  is covered by the real-socket gate tests and the real `UnixListener` test in
  `check.rs`.
* `WireSqlObs::allowed` (dbproxy's verdict on the wire statement) stays unused by
  `evaluate`: the restored-text analysis now agrees with the proxy, and deriving
  `missing_grant` from the proxy would add a second source of truth.
* No coverage target exists in the `Makefile` (outside this node's paths); a
  future `rs-coverage` target should export `BLASTIT_COVERAGE=1` together with
  `RUSTFLAGS=-C instrument-coverage` (or pass `--features coverage`).
