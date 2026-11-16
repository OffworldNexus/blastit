# RESULT MB.21.4.5.6

STATUS: done

## Summary

The retired manifest format is gone from Rust. `crates/blastit-manifest/src/parse.rs` is deleted with no
replacement; `Manifest::from_toml_str` and `Manifest::parse_and_validate` are gone; `mod parse` is gone;
`MissingVersion` / `UnsupportedVersion` are gone; the retired patcher (`ManifestEdits`, `ProposedRoute`,
`ProposedGrant`, `Plan`, `apply_edits`, their helpers and their ~580-line test module) is deleted from
`edit.rs` while `EditError` and `write_atomic` stay; and the `toml` dependency left
`crates/blastit-manifest/Cargo.toml` with its `Cargo.lock` packages. One reader remains:
`blastfile::read` → `Manifest::from_blastfile_str`.

Attempt 2 closes the review's MAJOR: the retired reader's ten valueless error variants (`Syntax`,
`UnknownKey`, `NotTopLevel`, `MissingKey`, `WrongType`, `Empty`, `UnknownTableOperation`,
`UnknownProtocol`, `StarredColumn`, `UnknownBodyCodec`) are deleted from `src/error.rs`, so no public type
still renders `invalid TOML:`, `after a [table] is not top-level` or the retired per-table spellings those
variants quoted. The `ManifestError` enum doc now
names surviving structural variants (`Equals`, `UnknownKeyword`, `Unexpected`) and the four
`UnknownBodyCodec` Display assertions in `tests/mutation_gaps.rs` are retargeted to `InvalidName`, keeping
the control-byte, 120-character and character-not-byte assertions. The MINOR's stale retired-format prose on
live items is reworded (`[[route]]` → `view`/route, `[proxy.<name>]` → `postgres <name>`,
`[route.fastcgi]` → `fastcgi` keyword), plus the last two false sentences in `error.rs`'s `Location` docs
("The retired reader still produces it", "A `[[route]]` entry").

The diff is deletion-dominated: **1277 lines of `parse.rs` plus 1782 lines of `edit.rs` deleted against 264
insertions** across the whole change (14 files, 4 plan files pre-modified by the dispatcher). The 600-line
budget does not apply to this leaf.

Kept as instructed: `Manifest::version()` (always `1`), `global_groups` (always empty), `PhpPolicy`,
`StaticPolicy`, `Route::static_policy`/`php` (unreachable from a `Blastfile`; follow-up below),
`Proxy::output`, `UidLayout`, `Manifest::assemble` and the `#[doc(hidden)]` builders.

Rewritten, not deleted:
- `src/lib.rs`'s crate-doc example is now a `Blastfile` (`postgres $DATABASE_URL`, one `view` with a
  `query`) and keeps every assertion about `mounts`, `php`, `name`, `sandbox` and `equivalent`.
- `src/model.rs`'s reader-built unit fixtures are now `from_blastfile_str` documents or model builders
  (`the_boot_predicates_read_every_routes_executor`, `route_lines_are_the_header_lines_in_routing_order`,
  `equivalent_ignores_only_the_route_lines`, the `EQUIVALENT` fixture).
- `src/validate.rs`'s `retired_static` helper now builds the same per-route static block through
  `Route::new(...).with_static(...)`; the case whose whole subject was the retired reader's `spa` lowering
  (`a_bad_index_beside_the_retired_flag_is_reported_once`) is deleted, the mount rules being
  `MB.21.4.5.2`'s.
- `src/blastfile/mod.rs`'s `crate::edit::ManifestEdits` intra-doc link now names `read`, the one reader.

## Deleted surface and the leaf that removed its last caller

| Deleted item | Leaf that removed its last caller |
|---|---|
| `src/parse.rs` (`parse`, `Parser`, `syntax_error`, `check_top_level_keys`, `declared_proxies`, every per-section parse fn) | call sites moved by `MB.21.4.5.1`–`.5`; last production caller was `Manifest::from_toml_str`, deleted here |
| `Manifest::from_toml_str` | `MB.21.4.5.1` (schema suite), `.3` (last manifest suites/`common`), `.4` (`for_learn`, env/sqlpolicy examples), `.5` (scheduler fixtures); the remaining callers were this leaf's own `parse_and_validate` and retired patcher |
| `Manifest::parse_and_validate` | no caller remained once `MB.21.4.5.4` moved `--learn`'s boot to `parse_blastfile`; the entry point itself was deleted here |
| `edit::ManifestEdits`, `edit::ProposedRoute`, `edit::ProposedGrant` | `MB.21.4.3` moved `dev_learn`/`check`/`audit_v1`; last caller was `edit::apply_edits`, deleted here |
| `edit::apply_edits` and its private surface (`Plan`, `GrantTarget`, `Patch`, `NewRoute`, `Multi`, `Header`, `LineInfo`, `render*`, `verify`, `expected_routes`, `patch_grant`, `append_grant_table`, `normalise_queries`, `route_index`, `valid_route_name`, `scan*`, `walk`, `parse_header`, `route_*`, `element_end`, `header_matches`, `simplify_header`, `is_*`, `trailing_comment`, `closing_bracket`, `append_to_array`, `toml_*`) plus its `#[cfg(test)]` module | `MB.21.4.3` removed the last production call sites; function and suite deleted here |
| `EditError::InlineGrant` | only the retired patcher constructed it; pruned here (no writer or suite matches it) |
| `ManifestError::Syntax` | last constructor was `parse.rs` (deleted here); no other crate or suite named it |
| `ManifestError::UnknownKey` | last constructor was `parse.rs` (deleted here); no other crate or suite named it |
| `ManifestError::NotTopLevel` | last constructor was `parse.rs` (deleted here); no other crate or suite named it |
| `ManifestError::MissingKey` | last constructor was `parse.rs` (deleted here); no other crate or suite named it |
| `ManifestError::WrongType` | last constructor was `parse.rs` (deleted here); no other crate or suite named it |
| `ManifestError::Empty` | last constructor was `parse.rs` (deleted here); no other crate or suite named it |
| `ManifestError::UnknownTableOperation` | last constructor was `parse.rs` (deleted here); no other crate or suite named it |
| `ManifestError::UnknownProtocol` | last constructor was `parse.rs` (deleted here); no other crate or suite named it |
| `ManifestError::StarredColumn` | last constructor was `parse.rs` (deleted here); no other crate or suite named it |
| `ManifestError::UnknownBodyCodec` | last constructor was `parse.rs` (deleted here); its four Display assertions in `tests/mutation_gaps.rs` were retargeted to `ManifestError::InvalidName` in this leaf |
| `ManifestError::MissingVersion`, `ManifestError::UnsupportedVersion` | assertions re-homed by `MB.21.4.5.1`/`.3`; last constructor was `parse.rs`, deleted here |
| `toml` dependency and the `toml`, `toml_datetime`, `toml_parser`, `toml_writer`, `serde_spanned`, `winnow` lock entries | last user was `parse.rs`, deleted here |

## Review findings closed in attempt 2

- **[MAJOR] dead error surface.** `git grep -nE 'ManifestError::(Syntax|UnknownKey|NotTopLevel|MissingKey|
  WrongType|Empty|UnknownTableOperation|UnknownProtocol|StarredColumn|UnknownBodyCodec)'` over `crates/`
  now matches nothing. The enum doc names `Equals`, `UnknownKeyword` and `Unexpected`; `tests/mutation_gaps.rs`
  pins clipping/escaping on `InvalidName`, a parser-produced variant.
- **[MINOR] stale retired-format prose.** `model.rs`'s `Route` ("One route of a `view`"), `Manifest::routes`
  ("The routes the `view` entries lowered to"), `Manifest::route_lines` and `with_route_lines` now say
  `view` header; `edit.rs`'s `EmptyPath` ("no endpoint pattern"), `UnknownProxy` ("no `postgres <name>`
  declares") and `MissingRoute` ("view ... neither declared nor proposed") are reworded; `lib.rs`'s doc
  says `fastcgi` is not a keyword and is refused as unknown. `error.rs`'s `Location::Group` no longer claims
  the retired reader produces it, and `Location::Route` no longer spells `[[route]]`.
- The epic gate's `mod parse` over-match of the retained `mod parser;` is the lead's; not touched.
- Attempt 1 left `edit.rs`'s `crate::error` import out of rustfmt order and two `mutation_gaps.rs`
  blocks unformatted (its RESULT's `fmt-exit=0` did not hold); `cargo fmt --all` reordered/reflowed them
  and `cargo fmt --all -- --check` is clean on this attempt.

## Follow-up: the per-route `static` / `php` pair

`Route::static_policy()`, `Route::php()` and the `pub(crate)` `with_static` / `with_php` builders stay in
the model although no `Blastfile` can set them. §4.3 schedules their removal, but it is **blocked on
`MB.21.5.1`**, which rewrites the pages that still show the retired per-route blocks
(`docs/frameworks/django.md`, `docs/frameworks/php.md`, `docs/frameworks/static-and-spa.md`) before
`docs_fences` can stop carrying those fences. Until then `check_static`, `is_all_static`,
`can_boot_without_app` and their unit coverage keep reading the pair; the doc comments on those methods now
say exactly that.

One residual names the retired format to a user, and it is that pair's: `check_static_declarations` passes
`"[route.ingress]"` and `"[route.<proxy>]"` as a `StaticDeadDeclaration` key. Neither policy can be set by
a `Blastfile`, so the branch is unreachable from a real file and moves with the pair's removal in `§4.3`
(blocked on `MB.21.5.1`); it is recorded here rather than changed under this leaf.

## Acceptance evidence

### `parse.rs` absent, both greps empty, `cargo tree` prints 0

```console
$ test ! -e crates/blastit-manifest/src/parse.rs; echo exit=$?
exit=0
$ grep -rnE 'from_toml_str|Manifest::parse_and_validate|toml::|toml_edit|^mod parse;' crates/ --include='*.rs'; echo exit=$?
exit=1
$ grep -rnE 'blastit[.]toml' crates/ --include='*.rs'; echo exit=$?
exit=1
$ cargo tree -p blastit-manifest --locked --edges normal | grep -ci toml
0
```

### Only the dependency removal in the three cargo files

```console
$ git diff --stat Cargo.toml Cargo.lock crates/blastit-manifest/Cargo.toml
 Cargo.lock                         | 55 --------------------------------------
 crates/blastit-manifest/Cargo.toml |  1 -
 2 files changed, 56 deletions(-)
```

The workspace `Cargo.toml` carries no `toml` and never carried `toml_edit` (confirmed by grep, no diff);
only `Cargo.lock` and the crate manifest changed, no version bump.

### Whole-change diff

```console
$ git diff --stat
 Cargo.lock                                         |   55 -
 crates/blastit-manifest/Cargo.toml                 |    1 -
 crates/blastit-manifest/src/blastfile/mod.rs       |   13 +-
 crates/blastit-manifest/src/edit.rs                | 1782 +-------------------
 crates/blastit-manifest/src/error.rs               |  115 +-
 crates/blastit-manifest/src/lib.rs                 |   44 +-
 crates/blastit-manifest/src/model.rs               |  220 ++-
 crates/blastit-manifest/src/parse.rs               | 1277 --------------
 crates/blastit-manifest/src/validate.rs            |   63 +-
 crates/blastit-manifest/tests/mutation_gaps.rs     |   34 +-
 plan/JOURNAL.md                                    |    8 +
 plan/nodes/MB/MB.21/LOG.md                         |    4 +
 .../MB/MB.21/MB.21.4/MB.21.4.5/MB.21.4.5.6/LOG.md  |   32 +
 .../MB/MB.21/MB.21.4/MB.21.4.5/MB.21.4.5.6/NODE.md |    7 +-
 14 files changed, 264 insertions(+), 3391 deletions(-)
```

(The four `plan/` entries are the dispatcher's pre-existing working-tree edits, not this leaf's.)

### `make check`

```console
$ timeout 3600 make check > /tmp/opencode/mb21456-check-a2b.log 2>&1; echo make-exit=$?
make-exit=0
$ grep -cE '^test result: ok' /tmp/opencode/mb21456-check-a2b.log
165
```

It includes `guard`, `critical-guard`, `cargo fmt --check`, `cargo clippy --workspace --all-targets
--locked -- -D warnings`, `cargo doc --workspace --no-deps --locked` with `RUSTDOCFLAGS=-D warnings`, the
full Rust test suite, `test-critical`, `docs-unit`, `docs-check`, `docs-test`, `seam-check`,
`platform-matrix-check`, `workflow-check`, `packaging-gates-test`, `doc-tone`, `manifest-purity` and
`plan-validate`. The host-watermark case in `critical_dev_and_fail_closed` passed on this run
(`a_memory_watermark_above_the_reading_serves_and_above_the_host_refuses_by_name ... ok`), so no rerun was
needed.

### `make critical-guard BASE=7ba6d23 NODE=MB.21.4.5.6`

```console
$ timeout 600 make critical-guard BASE=7ba6d23 NODE=MB.21.4.5.6
critical-guard: 7ba6d23..working tree, 14 changed file(s)
node MB.21.4.5.6: paths ['crates/blastit-manifest/src/parse.rs', … 'Cargo.lock']
  no change to a protected critical test
exit=0
```

### Manifest suites and doctests

```console
$ cargo test -p blastit-manifest --locked
... 32 suites, 0 failed ... (mutation_gaps 22, validate 31)
   Doc-tests blastit_manifest
running 7 tests
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Docs and formatting

```console
$ RUSTDOCFLAGS='-D warnings' cargo doc -p blastit-manifest --no-deps --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.12s
$ cargo fmt --all -- --check; echo fmt-exit=$?
fmt-exit=0
```

## Follow-ups

None. No caller compiled against `from_toml_str` at the start of this leaf, and no out-of-scope file needed
a change.
