# Result MB.21.3.0

## Summary

Landed the **model half** of the Blastfile cutover in `crates/blastit-manifest`, additively:
`Manifest` gained `name`/`mounts`/`php` (+ `name()`/`mounts()`/`php()` and the crate-internal
`with_name`/`with_mounts`/`with_php`), `Manifest::equivalent` compares every field except
`route_lines`, `StaticMount` and `routing::{MountTable, MountMatch}` are exported from the crate
root, `Route` gained `sandbox` (+ `with_sandbox`), and `UidLayout`/`check_uids` number by **sandbox**
(`UidLayout::sandbox(u32)`, with `view(usize)` kept as the retired reader's numbering).

`StaticPolicy` is untouched (`spa` byte for byte, no `fallback` field) and `Route::static_policy`/
`with_static`/`php`/`with_php` still compile and behave exactly as before, so the workspace stays
green: the retired TOML reader now fills **both** the per-route blocks and the new
`Manifest.mounts`/`Manifest.php` from the same document. Every bridge is listed below with its
deletion owner. No dependency, no `Cargo.toml`/`Cargo.lock` change, no protected critical test
touched (`critical-guard: no change to a protected critical test`).

## Evidence

### 1. `cargo test -p blastit-manifest --locked` (exit 0)

```console
$ cargo test -p blastit-manifest --locked
     Running unittests src/lib.rs (blastit_manifest-53e2f0661d1adbeb)
test result: ok. 53 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/critical_manifest_011.rs
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_manifest_edit.rs
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/critical_manifest_edit_executor.rs
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_php_manifest.rs
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running tests/critical_static_manifest.rs
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
     Running tests/docs_fences.rs
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running tests/edit_branches.rs
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/grants.rs
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/mutation_gaps.rs
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/parse.rs
test result: ok. 38 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/routing.rs
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s
     Running tests/uid.rs
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/validate.rs
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
all doctests ran in 0.49s; merged doctests compilation took 0.45s
```

### 2. `make rs-fmt` (exit 0), `make rs-lint` (exit 0, `-D warnings`) and `make rs-doc` (exit 0)

```console
$ make rs-fmt
cargo fmt --all -- --check
$ make rs-lint
PYTHONHOME=… PYO3_PYTHON=… LD_LIBRARY_PATH=… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit v0.2.0 (…/crates/blastit)
    Checking blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.68s
$ make rs-doc
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
    Documenting blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.23s
```

### 3. `cargo check --workspace --all-targets --locked` (exit 0)

```console
$ cargo check --workspace --all-targets --locked
    Checking blastit-view v0.2.0 (…/crates/blastit-view)
    Checking blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.66s
```

Every other crate still builds through the bridges below (`blastit-scheduler/src/route.rs`,
`blastit/src/{stage0,stage1,audit_v1,dev_learn,php_view,visibility}.rs` keep calling
`Route::static_policy()`/`Route::php()`/`UidLayout::view`).

### 4. `make check` (exit 0)

```console
$ make check
uv run --no-project tools/guard.py
guard: OK (1 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 24 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
…
     Running unittests src/lib.rs (blastit_manifest-…)
running 53 tests
…
     Running tests/uid.rs
running 8 tests
…
No issues found                       # docs-check (zensical --strict)
doc-tone: ok — 28 user-facing page(s), no banned phrase
uv run --no-project tools/plan.py validate
plan OK
```
`make check exit=0`, 131 `test result: ok` lines, zero `test result: FAILED`. Run twice: once before the
final crate-doctest addition and once after (this tail is the second run).

### 5. Acceptance items 2–4 (the new behaviour, by name)

```console
$ cargo test -p blastit-manifest --locked
test uid::tests::routes_of_one_sandbox_share_one_uid ... ok
test uid::tests::a_mount_owns_no_uid ... ok
test uid::tests::the_highest_sandbox_is_what_the_range_check_counts ... ok
test a_route_runs_under_the_uid_of_its_sandbox ... ok          # tests/uid.rs
test the_next_sandbox_takes_the_next_uid ... ok                # tests/uid.rs
test model::tests::equivalent_ignores_only_the_route_lines ... ok
test model::tests::equivalent_honours_every_other_field ... ok
test routing::tests::a_mount_matches_on_a_segment_boundary_only ... ok
test routing::tests::the_longest_prefix_wins ... ok
test routing::tests::the_root_mount_keeps_the_whole_path ... ok
test routing::tests::a_malformed_prefix_matches_nothing ... ok
test a_static_block_fills_the_mount_list_beside_the_route ... ok   # tests/parse.rs
test a_php_block_fills_the_manifest_front_controller ... ok        # tests/parse.rs
test a_plain_document_fills_no_mount_no_front_controller_and_no_name ... ok
test every_retired_route_is_its_own_sandbox ... ok                 # tests/parse.rs
$ cargo test -p blastit-manifest --doc --locked
test crates/blastit-manifest/src/uid.rs - uid::UidLayout (line 11) ... ok
test crates/blastit-manifest/src/routing.rs - routing::MountTable::lookup (line 200) ... ok
test crates/blastit-manifest/src/lib.rs - (line 45) ... ok
test crates/blastit-manifest/src/lib.rs - (line 84) ... ok
test crates/blastit-manifest/src/edit.rs - edit (line 36) ... ok
test crates/blastit-manifest/src/routing.rs - routing::RoutingTable::route (line 108) ... ok
test result: ok. 6 passed; 0 failed
```
The crate-level doctest now exercises `mounts()`/`php()`/`name()`/`route(0).sandbox()`/`equivalent()` on a
parsed manifest, so the crate docs name and run the new surface.

* **Item 2 (uid by sandbox):** `UidLayout::sandbox(u32)` is the new numbering and `view(usize)` keeps
  the retired reader's; the module doctest shows both. `tests/uid.rs` proves a parsed manifest's
  `sandbox(route.sandbox()) == view(index)`; the shared-sandbox ("two endpoints, one uid") and
  "a mount owns no uid" halves are `src/uid.rs`'s own tests, because only the crate can build a
  `Route` with a chosen sandbox (`tests/` never calls a `pub(crate)` item, design §20/TS-4).
  `check_uids` now counts the **highest sandbox**, and `the_highest_sandbox_is_what_the_range_check_counts`
  proves two routes of one sandbox fit at a base where the second sandbox overflows.
* **Item 3 (`equivalent`):** `equivalent_honours_every_other_field` changes one field at a time
  (version, app, name, preload, global_groups, proxies, groups, routes, mounts, php,
  unresolved_grants), both directions; `equivalent_ignores_only_the_route_lines` asserts a document
  moved down one line has different `route_lines()` and is still equivalent.
* **Item 4 (`MountTable`):** `/assets`, `/assets/v2`, `/assets-secret` table test both directions
  (a mount does not answer a longer word, and the longer word's mount does not answer the shorter
  path), longest prefix wins regardless of manifest order, and the root mount keeps the whole path.

### 6. Acceptance item 5 (`StaticPolicy` untouched) and item 6 (no TOML code removed)

```console
$ git diff --numstat crates/blastit-manifest/src/model.rs
353	0	crates/blastit-manifest/src/model.rs      # purely additive: no line removed
$ git diff -U0 crates/blastit-manifest/src/model.rs | grep -c '^-[^-]'
0
$ grep -n '^toml' crates/blastit-manifest/Cargo.toml
19:toml = "1.1.8"
```
`StaticPolicy` has the same four fields (`root`, `index`, `spa`, `immutable`) and the same literal
shape — the model diff removes nothing — and `critical_static_manifest.rs` (15 tests) and
`critical_php_manifest.rs` (9 tests) pass **unmodified**, which is what pins "behaves exactly as
today".

## Bridges (each with its deletion owner, `MB.21.3.7` unless stated)

| Bridge kept | Why it is here | Deleted by |
|---|---|---|
| `Route::static_policy()` + `Route::with_static` | 20 call sites across both accessors outside this leaf (`blastit-scheduler/src/route.rs`, `blastit/src/{stage0,stage1,audit_v1,dev_learn}.rs`, `tests/critical_learn_executor_blocks.rs`) still read the per-view block; `src/parse.rs` fills it **and** `Manifest.mounts` from the same `[route.static]` | `MB.21.3.7` (accessors), `MB.21.3.5` (the `edit.rs` rebuild chain) |
| `Route::php()` + `Route::with_php` | same call sites read the per-view front controller; `src/parse.rs` fills it **and** `Manifest.php` (first route wins) | `MB.21.3.7` |
| `StaticPolicy.spa` (no `fallback` field) | adding `fallback` here would leave every `StaticPolicy { … }` literal outside this leaf red (`blastit-scheduler/src/route.rs`, `static_fixture.rs`, `critical_rewrite.rs`, the two manifest suites) | `MB.21.3.7` (§9.3) |
| `UidLayout::view(usize)` | the retired reader's numbering, used by `blastit/src/{stage0,visibility}.rs`; `sandbox(u32)` is the new one | `MB.21.3.7` (call sites move to `sandbox`) |
| `Manifest::static_roots` / `check_static_roots` iterate **routes** | the mount-based audit (§9.6) needs `Location::Mount`-style error locations and the scheduler/`check` call sites, both outside this leaf; today's sentences and `Location::Route` are what `critical_static_manifest.rs` asserts | `MB.21.3.7` |
| `Manifest::is_all_static` / `can_boot_without_app` read the per-route blocks | §9.5's mount/php form (`views.is_empty() && !mounts.is_empty()`) would flip today's TOML static sites to `false` because the reader also fills `mounts` | `MB.21.3.7` |
| `edit.rs::verify` keeps its granular section names | `critical_manifest_edit.rs` / the `edit.rs` unit tests assert `"the top-level keys"` / `"the routes"`; the Blastfile editor replaces the function with one `Manifest::equivalent` call | `MB.21.3.5` |
| `parse.rs` bridge itself (`mounts_of`/`php_of`, one root mount per static route) | the retired format has no mount prefix and no single front-controller spelling, so a `[route.static]` becomes a mount at `"/"` (the responder's own URL root) and the first `[route.php]` fills `Manifest.php`; nothing reads `mounts`/`php` on the runtime path yet, so no behaviour depends on it | `MB.21.4.5` (the reader) |

## Files

* `crates/blastit-manifest/src/model.rs` — `StaticMount`; `Route.sandbox`/`with_sandbox`/`sandbox()`;
  `Manifest.name/mounts/php`, `name()`/`mounts()`/`php()`, `with_name`/`with_mounts`/`with_php`,
  `equivalent`; unit tests for `equivalent`.
* `crates/blastit-manifest/src/routing.rs` — `MountTable`, `MountMatch`, `lookup`, `strip_prefix`;
  doctest + four mount tests.
* `crates/blastit-manifest/src/uid.rs` — `UidLayout::sandbox(u32)`, `view` documented as the retired
  reader's numbering, new module doctest; three unit tests.
* `crates/blastit-manifest/src/validate.rs` — `check_uids` numbers by the highest sandbox.
* `crates/blastit-manifest/src/parse.rs` — sets each route's sandbox (its position), fills
  `Manifest.mounts`/`Manifest.php` beside the per-route blocks, `with_name(None)`.
* `crates/blastit-manifest/src/edit.rs` — `expected_routes` carries the sandbox and renumbers after an
  insertion; `verify` also compares `name`/`php`/`mounts`.
* `crates/blastit-manifest/src/lib.rs` — crate docs name the new accessors, `StaticMount`/
  `MountMatch`/`MountTable` exported.
* `crates/blastit-manifest/tests/parse.rs` — four bridge tests.
* `crates/blastit-manifest/tests/uid.rs` — two sandbox-numbering tests.
* `crates/blastit-manifest/src/error.rs` — **not modified** (nothing in this leaf needed a new error).

## Follow-ups

1. **§13.2 boot-warning re-spellings** (`group`→`trait`, `route`→`view`, `proxy`→`store`,
   `body = "raw"`→`body raw`, `egress = true`→`egress any`) are assigned to `MB.21.3.0` by the
   design but are **not** in this leaf's `# Goal`; they would also break sentence assertions in
   `crates/blastit/tests/*`, which is outside this node's paths. Needs its own leaf (with those test
   paths) or a widened `MB.21.3.7`.
2. **`validate::check_routes` still compares route names, not sandbox ids** (§10.3). Changing it
   would let two same-name routes with different sandboxes through, which the retired reader can
   produce and today's suites assert against; it belongs with the lowering leaf (`MB.21.3.8`) or
   `MB.21.3.7`, together with the suites that pin `DuplicateRouteName`.
3. **Mount prefix validation** (§9.1: `/`-anchored, no trailing `/` unless `"/"`, no `..`/empty
   segment/backslash/NUL/control/`?`/`#`/`%`, `DuplicateMount`) is not implemented here: `MountTable`
   simply never matches a malformed prefix. The rules need `ManifestError` variants (`error.rs`) and
   the Blastfile lowering (`MB.21.3.8`) to have a document that can express them.
4. **`StaticOutcome::fallback_missing`, `RouteInfo::static_forward` removal and the runtime
   precedence** are `MB.21.3.7`/`MB.21.4.2` as the node's `# Notes` says; this leaf only provides the
   model they read.
5. **Proposed `LESSONS.md` line** (outside this node's paths, so recorded here): a route field that
   is *derived from position* (`Route.sandbox` from the retired reader) must be renumbered for
   **every** route after an insertion in `edit.rs`'s postcondition, not only for the inserted one —
   otherwise `--learn` refuses every insert with `Verification { section: "the routes" }` while the
   patched routes look correct. `crates/blastit-manifest/src/edit.rs`, `MB.21.3.0`.
6. `Manifest::equivalent` has no production caller yet; `MB.21.3.4` (`fmt`), `MB.21.3.5` (`--learn`
   verify) and `MB.21.3.8` (parity oracle) are its owners.
