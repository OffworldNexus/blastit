# MA.2.2.2 — Leaf 5: surgical comment-preserving manifest editor and atomic write

## Summary

Implemented `crates/blastit-manifest/src/edit.rs` and wired it as
`blastit_manifest::edit` (`lib.rs` gained the module and one doc bullet):

- **`apply_edits(source, &ManifestEdits) -> Result<String, EditError>`** parses
  first, resolves the edits against the parsed model, splices *whole lines*, and
  then re-parses the result and compares it with the model the edit intended.
  Everything it does not own is reproduced byte for byte: comments, blank lines,
  ordering and the author's quoting (the tests assert every original line is
  still present verbatim).
- **Insertion point from the model, not the text.** New routes go above the
  first `Route::is_unrestricted_catch_all()` (`path = ".*"`,
  `path = ['.*']`, and the anchored `['^.*$']` spelling), or at EOF. A
  method-restricted catch-all is *not* an insertion point, so another method
  still falls through in the author's order. Comment lines written directly
  above the catch-all move with it, so a section comment stays attached.
- **Grants are merged, not regenerated.** A one-line `queries = [...]` is
  extended in place (the existing entry keeps its exact quoting, a trailing
  comment is kept, an empty `[]` gets no leading comma); a grant with no
  `queries` key gets one after the `[route.<proxy>]` header; a route with no
  grant table gets a whole `[route.<proxy>]` block at the end of its own block.
  Duplicates are compared **after normalisation**, so `$1` on the wire and `$?`
  in the file are the same grant.
- **Exact normalised `queries` (`R24`, `R29`).** Every proposed statement is
  normalised (`$1` → `$?`, whitespace, trailing `;`). A statement that still
  holds `…` (`RedactedQuery`) or an inline quoted literal outside
  `KNOWN_STATIC_SETUP_SQL` (`LiteralQuery`) is refused; the message names
  `queries[N]`, the route and the fix ("bind the value and learn again") and
  never echoes the statement.
- **Idempotent.** Applying the same `ManifestEdits` to its own output returns
  that output byte for byte (`assert_eq!` on the full string); an empty edit set
  returns the source unchanged.
- **Fail-closed named errors** instead of a half-written file: unknown proxy
  (names the `[proxy.<name>]` declaration to add), grant on a route neither
  declared nor proposed, `unrestricted = true` next to exact queries, an inline
  grant table (`db = { … }`, which cannot be patched without duplicating the
  key), a duplicate route name with another path, an invalid/duplicate/empty
  inferred route name or regex, and `Verification { section }` when the
  postcondition would move something the edit does not own.
- **`write_atomic(path, content) -> io::Result<()>`**: same-directory
  `.{filename}.{pid}.tmp` created with `O_CREAT | O_EXCL` (a symlink planted at
  that name is refused, never written through), `write_all`, `fsync`, the
  author's file mode preserved, `rename`, then a directory `fsync`. A
  `TempFileGuard` unlinks the temp file on every failure path, so neither a
  successful nor a failed write leaves a `.tmp` behind.
- **Follow-up from `MA.2.2.1` honoured:** a proxy named `app`, `version` or
  `preload` is legal when its `[proxy.<name>]` declaration exists, in any
  position in the file (the guard resolves declarations document-wide), and the
  refusal for an undeclared one names the declaration. The regression test
  declares `[proxy.app]` *after* the `[[route]]` blocks and its `[route.app]`
  grant still parses, validates and is patched.

`ProposedRoute`, `ProposedGrant`, `ManifestEdits` and `EditError` are public and
`Serialize`/`Deserialize` exactly as `docs/design/ma-lean-dx.md` §4.2 B shapes
them (with `#[serde(default)]` on the two lists and `path`/`queries`, so a
partial JSON document still deserialises).

## Review notes (self-check)

- `apply_edits` does not depend on `toml_edit` spans or on `toml` spans. The
  route index comes from the parsed `Manifest`; a small structural scanner maps
  it to the Nth `[[route]]` line while tracking `"…"`, `'…'`, `"""…"""`,
  `'''…'''`, comments and bracket depth, so a `[[route]]` inside a multi-line
  string or an array is never mistaken for a header.
- No `unwrap()`/`expect()`/`panic!()`/unchecked slice indexing on the editing
  path (`chars[index]` in the scanner is bounded by the loop condition, every
  other access is `get`).
- Denied halves are tested for every rule added here, not just the happy paths.

## Evidence

```console
# RED before the module existed (the protected suite is created first)
$ cargo test -p blastit-manifest --test critical_manifest_edit --locked
error[E0432]: unresolved import `blastit_manifest::edit`
  --> crates/blastit-manifest/tests/critical_manifest_edit.rs:21:23
   | use blastit_manifest::edit::{
   |                       ^^^^ could not find `edit` in `blastit_manifest`
error: could not compile `blastit-manifest` (test "critical_manifest_edit") due to 1 previous error

# GREEN
$ cargo test -p blastit-manifest --test critical_manifest_edit --locked
running 16 tests
test comments_quoting_and_untouched_lines_survive_an_edit ... ok
test a_route_is_inserted_above_both_catch_all_spellings ... ok
test a_method_restricted_catch_all_is_not_an_insertion_point ... ok
test applying_the_same_edits_twice_changes_nothing ... ok
test a_grant_joins_an_existing_queries_list_without_rewriting_it ... ok
test every_edit_fixture_round_trips_through_parse_and_validate ... ok
test a_proposed_route_has_to_be_routable ... ok
test an_inline_literal_is_refused_without_echoing_it ... ok
test a_redacted_placeholder_is_refused_without_echoing_it ... ok
test a_grant_on_an_undeclared_proxy_names_the_missing_declaration ... ok
test an_unrestricted_grant_cannot_take_exact_queries ... ok
test an_inferred_route_name_cannot_silently_change_a_path ... ok
test an_inline_grant_table_is_named_rather_than_duplicated ... ok
test a_grant_on_a_reserved_proxy_name_still_boots ... ok
test the_atomic_write_leaves_no_temp_file_and_the_result_parses ... ok
test a_failed_write_removes_its_temp_file ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

$ cargo test -p blastit-manifest --locked
test result: ok. 9 passed   (unit)
test result: ok. 15 passed  (critical_manifest_011)
test result: ok. 16 passed  (critical_manifest_edit)
test result: ok. 3 passed   (docs_fences)
test result: ok. 13 passed  (grants)
test result: ok. 33 passed  (parse)
test result: ok. 11 passed  (routing)
test result: ok. 6 passed   (uid)
test result: ok. 29 passed  (validate)
test result: ok. 4 passed   (Doc-tests)
# 139 total, 0 failed, 0 ignored

$ cargo fmt -p blastit-manifest -- --check
FMT_CLEAN

$ cargo clippy -p blastit-manifest --all-targets --locked -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.43s

$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-manifest --no-deps --locked
Documenting blastit-manifest v0.1.0
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.68s

$ CARGO_NET_OFFLINE=true make guard critical-guard plan-validate py-lint
guard: OK (5 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 26 changed file(s)
  no change to a protected critical test
plan OK
ruff: All checks passed!
3 files already formatted

$ CARGO_NET_OFFLINE=true make docs-check
uvx --offline zensical build --strict
No issues found

$ CARGO_NET_OFFLINE=true make test-critical
cargo test -p blastit-ingress --test critical_ingress      -> ok. 55 passed
cargo test -p blastit-manifest --test critical_manifest_011 -> ok. 15 passed
cargo test -p blastit-manifest --test critical_manifest_edit -> ok. 16 passed
cargo test -p blastit-sqlpolicy --test critical_literals    -> ok. 25 passed
no Python critical tests yet (python/tests/critical/)
```

### `cargo fmt --check` / `make check` are not green tree-wide (not this node)

`make check` stops in `rs-fmt` (`Makefile:49`) on files **outside this node's
paths**, written by the parallel nodes MA.2.2.3 (http/scheduler) and MA.2.2.4
(dbproxy) while they are mid-flight:

```console
$ cargo fmt --all -- --check
Diff in .../crates/blastit-dbproxy/src/config.rs
Diff in .../crates/blastit-dbproxy/src/hooks.rs
Diff in .../crates/blastit-dbproxy/src/state.rs
Diff in .../crates/blastit-http/src/conn.rs
Diff in .../crates/blastit-http/tests/critical_early_abort.rs
Diff in .../crates/blastit-scheduler/src/scheduler.rs
# none in crates/blastit-manifest (see the scoped `--check` above)

$ CARGO_NET_OFFLINE=true make check
make: *** [Makefile:49: rs-fmt] Error 1
```

The workspace lint/doc gates fail for the same reason, again only inside
`blastit-dbproxy` (an in-progress signature change there):

```console
$ CARGO_NET_OFFLINE=true make rs-lint
error: could not compile `blastit-dbproxy` (lib test) due to 10 previous errors
  (e.g. `credentials_match(0, None, /* bool */)` missing an argument)
make: *** [Makefile:52: rs-lint] Error 101

$ CARGO_NET_OFFLINE=true make rs-doc
error: could not compile `blastit-dbproxy` (lib) due to 3 previous errors
  (E0061, E0432 in crates/blastit-dbproxy/src/state.rs / server.rs)
make: *** [Makefile:55: rs-doc] Error 101
```

Every gate scoped to this node's crate passes (`clippy`, `fmt`, `doc`, all
tests) and every gate that does not compile the in-flight crates passes
(`guard`, `critical-guard`, `plan-validate`, `py-lint`, `docs-check`,
`test-critical`). The tree-wide `make check` is expected to be re-run by the
lead once MA.2.2.3/MA.2.2.4 land; nothing in this node needs to change for it.

## Files

- `crates/blastit-manifest/src/edit.rs` (new, 1464 lines incl. rustdoc)
- `crates/blastit-manifest/src/lib.rs` (`pub mod edit;` + one doc bullet)
- `crates/blastit-manifest/tests/critical_manifest_edit.rs` (new, protected)

## Follow-ups

- `MA.2.2.10` (`dev --learn`) and `MA.2.2.11` (`check --learn`) should pass the
  wire-normalised statements straight into `ProposedGrant::queries`; `apply_edits`
  normalises again and is idempotent, so a second run after a successful learn
  writes nothing.
- `write_atomic` keeps the author's mode but not ownership/xattrs; a manifest
  replaced through a `blastit dev --learn` run as the same UID is unaffected.
- `apply_edits` deliberately does not validate the whole document (only the
  proposals), because `--learn` may run against a manifest whose boot errors are
  what the learning run is about to fix; it does refuse everything that would
  produce a *worse* document (literal/redacted queries, undeclared proxies,
  conflicts, inline grants, unrestricted + queries).
- A multi-line `queries = [` list is rewritten on one line when it is extended
  (documented in the module docs and at `patch_grant`); the rest of the document
  is untouched. Worth revisiting only if real manifests start wrapping queries.
