# RESULT — MA.2.2.8

**Status:** done

**Changed:** `crates/blastit-manifest/tests/critical_manifest_011.rs` (test-only; +2 tests,
+110 lines, pure additions). No production code, no `Cargo.toml`/`Cargo.lock`, no `docs/**`,
no `#[ignore]`. Every existing reserved-key, literal and `KNOWN_STATIC_SETUP_SQL` test is
unchanged (`git diff` on the file has no deleted line).

## What the two new tests pin

Audit gap G4, mutants `mf.group_arm_guard_off` and `mf.validate_no_normalize`.

- `a_reserved_key_inside_a_grant_body_is_not_an_unknown_field` — six negative twins:
  `version = 1`, `app = "a:b"` and `preload = ["m"]` written as a field of
  `[group.session.db]` and of `[route.db]`. Each source must fail with **exactly**
  `[ManifestError::NotTopLevel { key }]` and the documented
  `"{key} after a [table] is not top-level; move it above the first table"` — not the
  `UnknownKey { path: "group.session.db.version" }` the grant walk produces when the group
  arm's `check_grant` is missing. The allowed twin (both bodies carrying `queries = []`, group
  attached to the route) boots so the test is not refusing every grant body.
- `the_wire_spelling_of_the_fixed_setup_statement_is_exempt` — positive twin for the `$1`
  spelling: `queries = ["SELECT set_config('TimeZone', $1, false)"]` parses, validates with no
  warnings, and the model keeps `$1` verbatim (normalisation is for comparison, not rewriting).
  The denied twin is the same call with another literal
  (`'application_name'`), which is still `LiteralQuery { rule: 0 }`, so the exemption is the
  exact protocol statement and not "any `set_config`".

## Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test critical_manifest_011 --locked
running 17 tests
test a_reserved_key_after_a_table_names_the_key_and_the_fix ... ok
test a_redacted_placeholder_cannot_be_granted ... ok
test a_reserved_key_inside_a_grant_body_is_not_an_unknown_field ... ok
test an_ingress_table_needs_a_known_body ... ok
test app_must_be_module_attribute ... ok
test app_and_preload_parse_in_both_spellings ... ok
test an_inline_literal_is_refused_and_never_echoed ... ok
test groups_stays_top_level_at_the_top_and_inside_a_route ... ok
test a_statement_is_covered_by_tables_or_by_groups_but_not_by_itself ... ok
test every_literal_form_in_a_grant_is_refused ... ok
test raw_ingress_and_egress_are_boot_warnings ... ok
test allow_raw_is_an_unknown_key_on_every_other_codec ... ok
test author_chosen_names_may_repeat_a_top_level_key ... ok
test the_fixed_protocol_statements_may_keep_their_literal ... ok
test both_catch_all_spellings_are_unrestricted ... ok
test every_body_codec_parses ... ok
test the_wire_spelling_of_the_fixed_setup_statement_is_exempt ... ok

test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Crate-wide, no regression from the edited file:

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked
test result: ok. 9 / 17 / 18 / 3 / 13 / 33 / 11 / 6 / 29 passed; 0 failed; 0 ignored
   Doc-tests blastit_manifest
test result: ok. 4 passed; 0 failed; 0 ignored
```

## Mutation proof (scratch copy, tree never mutated)

`rsync -a --exclude target --exclude .git` to `/tmp/opencode/blastit-ma2278-mut` (shared with
MA.2.2.7), `CARGO_TARGET_DIR=/tmp/opencode/blastit-ma2278-target`, one anchor-checked edit per
run, pristine source restored from a copy between runs.

```console
# pristine scratch tip
running 17 tests ... test result: ok. 17 passed; 0 failed; 0 ignored

# mf.group_arm_guard_off: parse.rs group arm, `check_grant(grant, errors);` -> `let _ = grant;`
running 17 tests
test a_reserved_key_inside_a_grant_body_is_not_an_unknown_field ... FAILED
  panicked at critical_manifest_011.rs:331: assertion `left == right` failed:
  for "version" in "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\n[group.session.db]\nversion = 1\n"
  (not UnknownKey)
  left: [UnknownKey { path: "group.session.db.version" }]
  right: [NotTopLevel { key: "version" }]
test result: FAILED. 16 passed; 1 failed; 0 ignored

# mf.validate_no_normalize: validate.rs `let query = normalize(query);` ->
# `let query = query.to_string();` (line 219)
running 17 tests
test the_wire_spelling_of_the_fixed_setup_statement_is_exempt ... FAILED
  panicked at critical_manifest_011.rs:548: the $1 spelling is the same statement:
  ManifestErrors([LiteralQuery { location: Route { index: 0, name: "notes" },
  proxy: ProxyName("db"), rule: 0 }])
test result: FAILED. 16 passed; 1 failed; 0 ignored
```

Both runs leave the other 16 tests green, so each new test is the sole oracle for its branch.

## `make check`

```console
$ make check
MAKE_CHECK_EXIT=0
guard: OK (10 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 6 changed file(s)
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [VIOLATION: not covered by any node path]
  ...
  report only: 4 breaking change(s) would fail an enforced run
cargo fmt --all -- --check                        # clean
cargo clippy --workspace --all-targets --locked -- -D warnings   # Finished
cargo doc --workspace --no-deps --locked ... RUSTDOCFLAGS=-D warnings   # Finished
cargo test --workspace --all-targets --locked     # all suites ok
...
All checks passed!                                # ruff
Build finished.                                   # zensical --strict
plan OK                                           # tools/plan.py validate
```

`critical-guard` runs report-only without `--node` and lists the three other protected files
that parallel MA.2.2.5/2.2.6 sessions are editing at the same time; per node it authorizes this
file and nothing else:

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MA.2.2.8
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [authorized by crates/blastit-manifest/tests/critical_manifest_011.rs]
critical-guard: FAILED: 3 protected critical test(s) changed ... without a covering path in node MA.2.2.8
```

(the three failures are the other nodes' files, not this node's; `make check` itself exits 0.)

## Notes

- A concurrent formatting pass in the shared tree had reflowed two pre-existing `assert!`
  blocks in this file; `rustfmt --edition 2024` on the file (in this node's `paths`) restored
  them, so `cargo fmt --all -- --check` is clean and this node's diff is pure additions.
- `tests/docs_fences.rs` stays in the standard tier, as the node requires.
