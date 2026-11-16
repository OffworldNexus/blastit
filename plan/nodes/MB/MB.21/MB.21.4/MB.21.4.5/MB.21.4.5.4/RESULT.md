# Result — MB.21.4.5.4

STATUS: done

**Review response (PASS, 2 MINORs):** `stage1.rs:295` no longer describes slot 0 as
having a pattern that never matches — it says the fallback view holds no pattern
and so never matches; `Match::route` now documents that `for_learn` shifts every
index by one, so its index is the manifest index `+ 1` and must not be paired
with `Manifest::route`. Text only; re-ran `cargo fmt --all -- --check` (clean),
`make rs-doc` (clean) and `cargo test -p blastit --lib --features python stage1::`
(8 passed).

`dev --learn`'s routing table is now built from the model, and every Rust doc
example / unit fixture left in this leaf's paths reads a `Blastfile`.

## What changed

**(a) Model-side routing table.** `RoutingTable::for_learn(&manifest)` was added
beside `RoutingTable::new` in `crates/blastit-manifest/src/routing.rs`, over the
same `CompiledRoute` vector: it calls `new` (so a bad pattern is the same
`ManifestError::InvalidRegex`) and inserts one leading entry whose rule list is
**empty**. An empty rule list cannot be selected by `route()` and contributes
nothing to `methods_for()`, so "slot 0 never matches" is structural, not a
pattern that happens to be unsatisfiable. Declared routes shift to `1..=N` by the
`insert(0, …)` itself. The public surface is `for_learn` only; `new` keeps its
signature and error type; zero new dependencies; no new table type.

`stage1::learn_routing_table` no longer renders a document, and `toml_basic_string`
is deleted with it:

```rust
/// Builds the routing table for `dev --learn`, reserving slot `0` for the
/// scheduler's unrouted-request fallback (`R26`, `R27`).
///
/// Slot `0` carries no pattern — [`RoutingTable::route`] can never select it —
/// and declared routes `0..N` are their manifest entries, shifted to `1..=N`
/// in exact manifest order with their original methods and patterns intact.
/// The table is built from the model [`RoutingTable::for_learn`] already holds:
/// no document is rendered and re-parsed.
fn learn_routing_table(manifest: &Manifest) -> Result<RoutingTable, String> {
    RoutingTable::for_learn(manifest).map_err(|errors| errors.to_string())
}
```

Behaviour is unchanged: `dev_learn` and `tests/critical_learn_executor_blocks.rs`
still splice the editor's output (both re-run below); `LEARN_FALLBACK_ROUTE` is
still slot 0's name in events, and nothing in the table carries route names.

**(b) The remaining fixtures.** `stage1`, `infer_routes`, `php_view`,
`tests/critical_check_engine.rs`, `blastit-env` and `blastit-sqlpolicy` now read
`Manifest::from_blastfile_str` / `Manifest::parse_blastfile`. Where a shape has no
`Blastfile` spelling it is built through `Proxy::new` + `Manifest::from_parts`
with the reason in a doc comment: `url` beside `ca_cert` (§7.4: `url` is the
one-line form only), `url` beside a split slot, an unknown slot, a variable name
that is not `$IDENT`, and the `output` rename map (§7.5). `Proxy::output` and
`TableGrants::to_toml` are untouched (follow-ups).

## Acceptance evidence

### 1. Both greps print nothing over those paths

```console
$ grep -rnE 'from_toml_str|toml::' crates/blastit/src crates/blastit-env crates/blastit-sqlpolicy crates/blastit/tests/critical_check_engine.rs
rc=1        # no match
$ grep -rnE 'blastit[.]toml' crates/blastit/src crates/blastit-env crates/blastit-sqlpolicy crates/blastit/tests/critical_check_engine.rs
rc=1        # no match
```

### 2. `for_learn`, the function body, and stage1's assertions

`RoutingTable::for_learn` (with its doctest, which runs under
`cargo test -p blastit-manifest --doc`):

```rust
pub fn for_learn(manifest: &Manifest) -> Result<Self, ManifestErrors> {
    let mut table = Self::new(manifest)?;
    // One reservation in front of the compiled table: every declared route
    // shifts by exactly one, which is the index the scheduler's learn path
    // expects.
    table.routes.insert(
        0,
        CompiledRoute {
            methods: None,
            rules: Vec::new(),
        },
    );
    Ok(table)
}
```

Slot 0 / shifted indices / method filter / non-ASCII pattern — the stage1 unit
test, now over a `Blastfile` (`view a GET, POST regex '/a/[0-9]+', regex
'/café/1'` and `view b regex '/robots\.txt'`):

```rust
        assert_eq!(
            table.route("GET", "/a/42"),
            Some(blastit_manifest::Match { route: 1, rule: 0 })
        );
        assert_eq!(
            table.route("POST", "/café/1"),
            Some(blastit_manifest::Match { route: 1, rule: 1 })
        );
        assert_eq!(table.route("DELETE", "/a/42"), None);
        assert_eq!(
            table.route("GET", "/robots.txt"),
            Some(blastit_manifest::Match { route: 2, rule: 0 })
        );
        // Slot 0 is the never-matching fallback: no request is routed into it,
        // and it contributes no method to a `405`.
        assert_eq!(table.route("GET", "/unrouted"), None);
        assert_eq!(table.route("GET", "/"), None);
        assert_eq!(table.methods_for("/unrouted"), None);
```

`cargo test -p blastit --lib` builds the default (`php`) feature set, where the
stage1 test module is compiled out (`#[cfg(all(test, feature = "python"))]`), so
the same test was also run with `--features python`:

```console
$ cargo test -p blastit --lib --locked --features python stage1::tests::learn_routing
test stage1::tests::learn_routing_table_reserves_slot_zero_and_preserves_declared_matches ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 237 filtered out; finished in 0.01s
```

The refused path of the same invariant — a pattern `new` would reject is
rejected by `for_learn` with the same `ManifestError::InvalidRegex`
(`routing::tests::for_learn_refuses_a_pattern_that_new_refuses`):

```console
$ cargo test -p blastit-manifest --lib --locked routing::
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 147 filtered out; finished in 0.00s
$ cargo test -p blastit-manifest --lib --locked
test result: ok. 157 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.39s
```

### 3. Every command exits 0

```console
$ cargo test -p blastit --lib --locked
test result: ok. 212 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.36s

$ cargo test -p blastit --test critical_check_engine --locked
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
   (includes `infer_routes_reproduces_all_thirteen_rows` over `Blastfile` views)

$ cargo test -p blastit-env --all-targets --locked       # (see caveat below)
test result: ok. 50 passed; 0 failed; ...        # src/lib.rs unit tests
test result: ok. 25 passed; 0 failed; ...        # tests/environment.rs

$ cargo test -p blastit-env --doc --locked
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
   (postgresql.rs and lib.rs doctests read `Blastfile` literals)

$ cargo test -p blastit-sqlpolicy --all-targets --locked
test result: ok. 234 passed; 0 failed; ...       # src/lib.rs
test result: ok. 10 passed ... ok. 7 passed ... ok. 25 passed ... ok. 22 passed ... ok. 10 passed ... ok. 18 passed

$ make rs-doc
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.74s
   Generated .../target/doc/blastit/index.html and 13 other files
```

**Caveat on the acceptance command as written.** `cargo test -p blastit-env
--all-targets --doc` is refused by cargo itself:

```console
error: can't mix --doc with other target selecting options
```

so the two halves above are the same coverage (all targets, then all doctests).
Nothing was skipped.

Extra checks beyond the list, because the node requires unchanged `--learn`
behaviour:

```console
$ cargo test -p blastit --test critical_learn_executor_blocks --locked
test result: ok. 2 passed; 0 failed; 0 ignored; ...   # the learned document is spliced
$ cargo test -p blastit --lib --features python dev_learn::
test result: ok. 11 passed; 0 failed; 0 ignored; ...
$ cargo clippy -p blastit-manifest --lib --locked -- -D warnings
    Finished `dev` profile (no warnings)
$ cargo clippy -p blastit-env -p blastit-sqlpolicy -p blastit \
      --all-targets --locked --features python -- -D warnings
    Finished `dev` profile (no warnings)
```

`cargo fmt --all -- --check` is clean for all eleven files of this leaf; the
remaining diffs in the tree are `crates/blastit-manifest/tests/{routing,
validate,mutation_gaps}.rs`, all owned by the in-flight sibling `MB.21.4.5.3`
and outside this node's paths.

### 4. `critical-guard`

```console
$ make critical-guard BASE=2ef786ce381dce05208d8d4d09a50f8eb4d7589b NODE=MB.21.4.5.4
  M  crates/blastit/tests/critical_check_engine.rs  [authorized by crates/blastit/tests/critical_check_engine.rs]
  ... 8 more, all in sibling leaves ...
critical-guard: FAILED: 8 protected critical test(s) changed or deleted without a covering path ...
```

The only protected file this leaf touched is authorized by its own path. The
eight violations are other leaves' in-flight edits in this shared worktree —
`critical_manifest_011.rs` (5.1), `critical_static_manifest.rs` +
`critical_php_manifest.rs` (5.2), and the five `crates/blastit-scheduler/tests/
critical_*.rs` (5.5). Proof that no violation is mine: the guard is clean once
those leaves' nodes are named alongside mine (each file resolves to its own
node's path, and mine still resolves to `crates/blastit/tests/critical_check_engine.rs`):

```console
$ uv run --no-project tools/critical_guard.py --base 2ef786ce… \
    --node MB.21.4.5.1 --node MB.21.4.5.2 --node MB.21.4.5.4 --node MB.21.4.5.5
critical-guard: OK: all 9 protected change(s) are authorized by node(s) MB.21.4.5.1, MB.21.4.5.2, MB.21.4.5.4, MB.21.4.5.5
exit=0
```

## Follow-ups

- **`Proxy::output`** — `crates/blastit-manifest/src/model.rs`
  (`Proxy`'s `output` field, `Proxy::new`, `Proxy::output()`), still rendered by
  `crates/blastit-env/src/postgresql.rs` (`default_outputs`, the explicit
  `output` templates) and `crates/blastit-env/src/env.rs`. After `MB.21.4.5.6`
  no reader can produce it, so the field and both rendering paths can go.
- **`TableGrants::to_toml` and its Python caller** —
  `crates/blastit-sqlpolicy/src/tables.rs` (the method and its ~15 TOML-output
  assertions), its only production caller `crates/blastit-py/src/audit.rs`
  (the `"toml"` key of `table_grants`, lines ~366 and ~1553), the protected
  assertions in `crates/blastit-sqlpolicy/tests/critical_policy_surface.rs`, and
  the reference to the retired key in `python/blastit/pytest_plugin.py`.
- **Not a defect, just the rest of the epic:** `crates/blastit-manifest/src/
  {lib,model,validate,edit}.rs` still carry `from_toml_str` (crate doctest
  included) — that is `MB.21.4.5.6`'s erasure, and those files are outside this
  leaf's paths.
- **Tooling gotcha (no `docs/` edit allowed here):** `cargo test … --all-targets
  --doc` is rejected with `error: can't mix --doc with other target selecting
  options`; run `--all-targets` and `--doc` separately. Worth a
  `docs/knowledge/LESSONS.md` line from a node that owns it.
- **Shared-worktree note:** a concurrent agent formats this tree with an older
  `rustfmt` (it re-sorts `use` lists and `assert!` chains to the pre-2024 style,
  which the canonical `cargo fmt --check` then flags). The eleven files here were
  formatted with the tree's canonical rustfmt at the end; expect the same churn
  in any other leaf's files while that runs. The same worktree carried, for a
  while, a clippy failure in `crates/blastit-manifest/tests/validate.rs`
  (`clippy::unnecessary_to_vec`, MB.21.4.5.3's file). Scoped to this leaf,
  `cargo clippy -p blastit-manifest --lib -- -D warnings` and
  `cargo clippy -p blastit-env -p blastit-sqlpolicy -p blastit --all-targets
  --features python -- -D warnings` are both clean; the failure is not in a file
  this node touches.

## Files

| File | Change |
|---|---|
| `crates/blastit-manifest/src/routing.rs` | `RoutingTable::for_learn`; both doctests read a `Blastfile`; a unit test proves a pattern `new` refuses is refused here too |
| `crates/blastit/src/stage1.rs` | `learn_routing_table` one line over `for_learn`; `toml_basic_string` deleted; unit test on a `Blastfile` + slot-0 assertions; two stale `[route.static]` comments |
| `crates/blastit/src/infer_routes.rs` | fixture builds `view` sections; catch-all test covers the two lowered spellings |
| `crates/blastit/src/php_view.rs` | `PAGE` is `entrypoint php "public/index.php"` + `view app "/*"`; WSGI fixture is a view; `parse_blastfile` |
| `crates/blastit/tests/critical_check_engine.rs` (protected, in `paths`) | fixture builds `view {name} {pattern} {}`; all 13 `infer_routes` rows kept, the two catch-all rows spelled `"/*"` and `regex '.*'`; one stale comment and one test name de-retired |
| `crates/blastit-env/src/env.rs` | unit fixtures read `postgres $DATABASE_URL` / an empty `Blastfile` |
| `crates/blastit-env/src/lib.rs` | crate doctest: `postgres $DATABASE_URL` + a split `postgres reports { … ca_cert $REPORTS_CA_CERT }` |
| `crates/blastit-env/src/postgresql.rs` | module doctest is the eight-slot block; unit fixtures are `$DATABASE_URL` / slot blocks; the five unexpressible shapes go through `Proxy::new` with the reason |
| `crates/blastit-env/tests/environment.rs` | §5.1 example assembled from two `Proxy::new`s (URL+CA, rename map); one-proxy fixtures read `Blastfile` stores; output-map tests attach it through `Proxy::new`; all wiring assertions kept (in-place rewrite, CA gone, split outputs, env-var refusals) |
| `crates/blastit-sqlpolicy/src/learn.rs` | "writes the ones it saw into the `Blastfile`" |
| `crates/blastit-sqlpolicy/src/tables.rs` | module fence is the reference's grant block |
