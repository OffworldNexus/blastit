# Result MB.21.3.8

## Summary

`blastfile/lower.rs` lowers the CST into `Manifest` in one pass (§7.3) and the two §4.4 entry points
exist: `Manifest::from_blastfile_str` (parse + lower, no validate) and `Manifest::parse_blastfile`
(parse + lower + `validate`, returning its warnings). The nine suites the acceptance names — the
parity oracle plus `critical_blastfile_{traits,body,entrypoint,security}` and
`blastfile_{traits,grants,body,entrypoint}` — and the 15-fixture corpus prove the capability map row
by row; `blastfile_{patterns,errors}` and `critical_blastfile_{patterns,errors,parser,lexer}` land
here too, because they need this leaf's public entry point.

**The oracle is model-equality against an expected `Manifest` built inside the test.** There is no
TOML twin anywhere in this leaf: no `from_toml_str` call, no `.toml` fixture, no cross-format
comparison. `critical_blastfile_parity.rs` assembles the expected model field by field with the
crate's builders and compares it with `Manifest::equivalent`, so both sides of every row are
readable in one file. The rows the design drops (`version` header, `workers`, a per-view
`static`/`php`, the global `groups` attach) are asserted **absent**, not merely uncompared.

`make check` exits 0: 30 suites / 485 tests in the crate, 3112 tests workspace-wide, 0 failures.

## Evidence

All five commands were run serially from the working tree at `release/0.2.0` (base `46e1b80`). Its
code changes are this leaf's; the `plan/` diffs are the plan tool's own bookkeeping.

### 1. `cargo test -p blastit-manifest --locked --test critical_blastfile_parity` — PASS

```console
$ cargo test -p blastit-manifest --locked --test critical_blastfile_parity
running 20 tests
test a_view_cannot_carry_a_static_or_php_declaration ... ok
test row_a_catch_all_with_prefork_and_egress ... ok
test route_lines_carry_the_view_header_line ... ok
test row_a_mount_above_a_view ... ok
test row_a_mount_beside_a_front_controller ... ok
test row_a_multi_endpoint_view ... ok
test row_a_store_declared_by_its_url_variable ... ok
test row_an_unrestricted_grant ... ok
test row_get_implies_head ... ok
test row_several_patterns ... ok
test row_the_callable_the_name_and_the_preload_list ... ok
test row_a_regex_path ... ok
test row_a_static_only_file ... ok
test row_a_store_declared_by_its_slots ... ok
test row_the_ingress_policies ... ok
test row_the_grant_table ... ok
test row_a_shared_grant_set_attached_by_use ... ok
test the_dropped_rows_are_absent ... ok
test the_validate_warnings_are_surfaced_unchanged ... ok
test the_corpus_is_the_fifteen_rows_and_every_one_lowers ... ok

test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

One test per capability-map row, each named after its row; `the_corpus_is_the_fifteen_rows_and_every_one_lowers`
pins `CORPUS_SIZE = 15` and lowers all of them; `the_dropped_rows_are_absent` and
`a_view_cannot_carry_a_static_or_php_declaration` prove the drops (acceptance 2).

### 2. `cargo test -p blastit-manifest --locked` — PASS (30 suites, 485 tests)

```console
$ cargo test -p blastit-manifest --locked
...
     Running tests/validate.rs
running 29 tests
...
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s

   Doc-tests blastit_manifest

running 6 tests
test crates/blastit-manifest/src/lib.rs - (line 84) ... ok
test crates/blastit-manifest/src/routing.rs - routing::MountTable::lookup (line 204) ... ok
test crates/blastit-manifest/src/lib.rs - (line 45) ... ok
test crates/blastit-manifest/src/uid.rs - uid::UidLayout (line 11) ... ok
test crates/blastit-manifest/src/edit.rs - edit (line 36) ... ok
test crates/blastit-manifest/src/routing.rs - routing::RoutingTable::route (line 112) ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

Every suite green, including the nine this leaf owns:

```
unittests src/lib.rs                136 passed
tests/blastfile_body.rs               8 passed
tests/blastfile_entrypoint.rs        11 passed
tests/blastfile_errors.rs             7 passed
tests/blastfile_grants.rs            14 passed
tests/blastfile_patterns.rs          13 passed
tests/blastfile_traits.rs            11 passed
tests/critical_blastfile_body.rs      6 passed
tests/critical_blastfile_entrypoint.rs 6 passed
tests/critical_blastfile_errors.rs    6 passed
tests/critical_blastfile_lexer.rs     5 passed
tests/critical_blastfile_parity.rs   20 passed
tests/critical_blastfile_parser.rs    4 passed
tests/critical_blastfile_patterns.rs  7 passed
tests/critical_blastfile_security.rs  4 passed
tests/critical_blastfile_traits.rs    7 passed
```

(plus `critical_manifest_011`, `critical_manifest_edit`, `critical_manifest_edit_executor`,
`critical_php_manifest`, `critical_static_manifest`, `docs_fences`, `edit_branches`, `grants`,
`mutation_gaps`, `parse`, `routing`, `uid`, `validate` — the retired reader's suites, untouched and
still green, acceptance 6.) `grep -c "test result: ok"` = 30, `grep -c "test result: FAILED"` = 0.

### 3. `make rs-fmt` — PASS

```console
$ make rs-fmt
cargo fmt --all -- --check
EXIT=0
```

### 4. `make rs-lint` — PASS

```console
$ make rs-lint
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-manifest v0.2.0 (…/crates/blastit-manifest)
   Compiling pyo3-ffi v0.30.1
   Compiling pyo3 v0.30.1
   Compiling blastit v0.2.0 (…/crates/blastit)
    Checking blastit-scheduler v0.2.0 (…)
    Checking blastit-env v0.2.0 (…)
    Checking blastit-view v0.2.0 (…)
    Checking blastit-py v0.2.0 (…)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.95s
EXIT=0
```

No lint was weakened: `--all-targets` covers the new suites, and the only `allow`s added are the
per-accessor `#[allow(dead_code)]` with the leaf that will read it named in the comment (deviation
g).

### 5. `make check` — PASS (exit 0)

```console
$ make check
uv run --no-project tools/guard.py
guard: OK (30 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 16 changed file(s)
  M  crates/blastit-manifest/tests/critical_blastfile_lexer.rs  [VIOLATION: not covered by any node path]
  report only: 1 breaking change(s) would fail an enforced run
cargo fmt --all -- --check
…
   Generated /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/doc/blastit/index.html and 13 other files
…
    Finished `test` profile [unoptimized + debuginfo] target(s) in 43.73s
…
96 passed, 65 subtests passed in 1.87s
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.24s
…
49 fences: 28 executed, 21 accounted, 0 failed · 22.7s
…
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 49 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=12, test-seam=7)
doc-tone: ok — 28 user-facing page(s), no banned phrase
manifest-purity: ok — 15 source file(s), no std::env
manifest-purity self-test: ok — 3 fixtures proved, 15 real source file(s)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

`make check` ran its own suites: 155 `test result: ok` lines, 0 `FAILED`, 3112 tests passed
workspace-wide, plus `96 passed, 65 subtests passed` (docs unit) and `49 fences … 0 failed`.

The one `critical-guard` line is `make check`'s **report-only** mode (it passes no `--node`), not a
defect. Naming the node authorizes the same file:

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.21.3.8
  M  crates/blastit-manifest/tests/critical_blastfile_lexer.rs  [authorized by crates/blastit-manifest/tests/critical_blastfile_lexer.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.21.3.8
```

### Fixture layout — the shape three later leaves read

15 files, one flat directory `crates/blastit-manifest/tests/fixtures/`, `*.blastfile` only; the
expected model is never a file (each test builds it). `common::CORPUS_SIZE = 15` pins the count.

| Fixture | Row it proves |
|---|---|
| `01-app-and-entrypoint.blastfile` | §7.1 rows 2–4 — the callable (`entrypoint python`), the application's name (`app`), the preload list (order kept, duplicates dropped) |
| `02-store-url.blastfile` | §7.1 store declaration, one-line form + §7.4 — `postgres $DATABASE_URL` → `input = {url = "DATABASE_URL"}`, the name never the value |
| `03-store-block.blastfile` | §7.1 store declaration, block form + §7.4 — every slot's `$IDENT`, plus a named second store (`analytics`) |
| `04-trait-and-use.blastfile` | §7.1 shared grant set + the dropped global attach — `trait` → `Group`, `use` → `Route.groups`, `global_groups == []` (§11.1) |
| `05-grants.blastfile` | §7.1 grant block / §13.1 — transactions, functions with and without `pg_catalog.`, select/insert/update in both spellings, delete, `query` in both string forms |
| `06-unrestricted.blastfile` | §13.1 — `unrestricted` on the default store and on a named one |
| `07-body.blastfile` | §7.1 ingress policy / §10.1 — the five `body` rows plus an endpoint that declares none, six routes in one sandbox (§10.3) |
| `08-static-only.blastfile` | §7.2 row 1 — a static-only file: one mount, no view (§9.5) |
| `09-static-and-view.blastfile` | §7.2 row 2 — a mount above a forwarding view (§9.2) |
| `10-static-and-php.blastfile` | §7.2 row 3 — a mount beside a front controller (`entrypoint php` + the catch-all view) (§12.2) |
| `11-catch-all.blastfile` | §7.2 row 4 + §7.1's `prefork`/`egress any` rows — a view matching everything |
| `12-regex-path.blastfile` | §7.2 row 5 — `regex '…'` kept verbatim (§8.1) |
| `13-several-paths.blastfile` | §7.2 row 6 — one view, several patterns → one route with two path rules |
| `14-get-implies-head.blastfile` | §7.2 row 7 / §8.4 — `GET` implies `HEAD` once, at lower time |
| `15-multi-endpoint.blastfile` | §10.3 + §7.3 step 6 — one view, two endpoints, one sandbox; `use` and the grant apply to both; `route_lines` carries the view header line twice |

Three later leaves read this corpus, which is why the shape is a contract and not an accident:
`MB.21.3.4` loops the printer's P1–P3 (idempotence + model-equality) over all 15, `MB.21.3.6`
counts the doc fences against them, and `MB.21.5.1` documents them. Each globs `fixtures/*` and
asserts the count, so a later leaf that adds or renames a file has to say so in `CORPUS_SIZE`.

### Deviations disclosed, each re-checked against the source

(a) **`#[doc(hidden)] pub` builders in `model.rs`** — `Proxy::new` (`model.rs:119–120`),
`Route::new`/`with_sandbox` (`436–437`, `470–471`), `Group::new` (`622–623`),
`PgGrant::with_unrestricted` (`212–213`), `Manifest::from_parts`/`with_app`/`with_name`/`with_mounts`/
`with_php`/`with_preload` (`992–1036`, `1147–1148`). *Why:* an integration test is a separate crate
and cannot reach `pub(crate) assemble`, so the expected model must be buildable from outside;
`#[doc(hidden)]` keeps them out of the public docs while `lower` stays the only production builder.

(b) **Mount `root`/`index` rules live in `lower.rs`** (`lower.rs:46–52` comment, `root_rule` at `881`,
`index_rule` at `896`, used at `836`/`844`). *Why:* `src/validate.rs` is not in `paths`; the phrases
are `check_static_root`/`check_static_index`'s (`validate.rs:419`, `447`) word for word, so a
developer reads one sentence either way. Follow-up: move them into `validate::check_mounts`
(`validate.rs:578`), which already takes a `Location`.

(c) **`UnrestrictedWithContent` (`error.rs:455`) is unreachable through the grammar.** *Why:*
`store_grant → 'postgres' ( 'unrestricted' | store_name? grant_body )` (`parser.rs:1251`), so
`unrestricted` can never be followed by a body. The variant stays because `validate` still raises it
on the model (`validate.rs:200`, `207`), and a lowered model cannot reach that shape.

(d) **`UnknownTrait` (`error.rs:1389`) instead of reusing `UnknownGroup`.** *Why:* `validate.rs` and
`mutation_gaps.rs` pin `UnknownGroup`'s text (`validate.rs:116`, `125`; `mutation_gaps.rs:473`+),
and `validate.rs` is out of `paths`. The new sentence names the fix — ``a trait is declared at the
top level: `trait <name> { … }` `` — and the view, which the group sentence cannot.

(e) **Two retired spellings survive, both outside `paths`.** `ManifestErrors`' `Display`
(`error.rs:1530–1539`) still prints `manifest has N error(s)` with `\n  - ` bullets, and
`raw_ingress_refusals` (`validate.rs:1002`) still says ``add `allow_raw = true` to that
[route.ingress] table``. *Why:* the join is asserted by `mutation_gaps.rs:473,497,538,554,572,598`,
`parse.rs:461,466,547` and `validate.rs:1121,1143` (all outside `paths`), and the raw-ingress
sentence is produced in `validate.rs`, also outside `paths`; acceptance 6 requires those suites
green. §18 asks for a newline join and a Blastfile spelling — follow-ups below.

(f) **`EntrypointNotReached` is not in the model.** *Why:* §12.2/§20 R30 place it in
`validate::warnings` and `MB.21.3.0` owns the model; `critical_blastfile_entrypoint.rs:100` records
that. `parse_blastfile` therefore surfaces the five warnings that exist — `UnattachedGroup`,
`UngrantedProxy`, `UnrestrictedRoute`, `RawIngress`, `EgressRoute` — unchanged, and
`the_validate_warnings_are_surfaced_unchanged` proves it.

(g) **`#[allow(dead_code)]` on accessors staged for later leaves** — `syntax.rs:129,135,158,359,379,542`
and `pattern.rs:76`. *Why:* each comment names the reader (`MB.21.3.4`'s printer, `MB.21.3.5`'s
editor), so the allow cannot hide a genuinely unused accessor from a reviewer.

## Files

Added:

- `crates/blastit-manifest/src/blastfile/lower.rs` (912 lines)
- `crates/blastit-manifest/tests/blastfile_body.rs`, `blastfile_entrypoint.rs`, `blastfile_errors.rs`,
  `blastfile_grants.rs`, `blastfile_patterns.rs`, `blastfile_traits.rs`
- `crates/blastit-manifest/tests/critical_blastfile_body.rs`, `critical_blastfile_entrypoint.rs`,
  `critical_blastfile_errors.rs`, `critical_blastfile_parity.rs`, `critical_blastfile_parser.rs`,
  `critical_blastfile_patterns.rs`, `critical_blastfile_security.rs`, `critical_blastfile_traits.rs`
- `crates/blastit-manifest/tests/fixtures/01-app-and-entrypoint.blastfile` … `15-multi-endpoint.blastfile` (15 files)

Modified:

- `crates/blastit-manifest/src/blastfile/mod.rs` (+55: the two §4.4 entry points, and the deletion of
  the module-level `#![cfg_attr(not(test), allow(dead_code))]` that had staged the parser — no
  blanket dead-code allow remains)
- `crates/blastit-manifest/src/blastfile/parser.rs` (+81: `pub(crate) line_starts`, the keyword list,
  `is_block_directive` so §6.7 row 3 reads "not a directive here")
- `crates/blastit-manifest/src/blastfile/pattern.rs` (+1: staged accessor)
- `crates/blastit-manifest/src/blastfile/syntax.rs` (+25: accessors `lower` reads, staged for 3.4/3.5)
- `crates/blastit-manifest/src/error.rs` (+96: `DuplicateProxyName`, `UnknownTrait`,
  `PhpEntrypointInvalid`, `StaticRootMissing`, `DuplicateStaticDirective`, `Location::View`)
- `crates/blastit-manifest/src/model.rs` (+84: `equivalent`, the `#[doc(hidden)]` builders)
- `crates/blastit-manifest/tests/common/mod.rs` (+56: the fixture loader, `CORPUS_SIZE`)
- `crates/blastit-manifest/tests/critical_blastfile_lexer.rs` (+49: the hostile-document corpus)
- `docs/knowledge/LESSONS.md` (+1: the `pg_catalog.unaccent` three-token gotcha)

## Follow-ups

- **Mount `root`/`index` into `validate::check_mounts`** — deletes the copy in `lower.rs:46–52`/`881`/`896`
  once `validate.rs` is in a leaf's `paths`; the phrases are already identical.
- **`EntrypointNotReached`** — `MB.21.3.0`'s model warning (§12.2, §20 R30); `parse_blastfile` will
  surface it unchanged once it exists.
- **`InvalidApp` (`error.rs:219–224`) and `PhpEntryInvalid` (`error.rs:636–644`) still echo their
  value** (`quoted(.app)`, `quoted(.entry)`) — §18 says they must name the position and the expected
  shape instead, because a pasted DSN or token can land in either slot. Both variants are shared with
  the retired reader, so the change belongs to a leaf that owns `parse.rs`'s suites too.
- **`ManifestErrors` should join with newlines** (§18) instead of `manifest has N error(s)` + bullets;
  `mutation_gaps.rs`/`parse.rs`/`validate.rs` pin the current text and are outside `paths`.
- **`raw_ingress_refusals`' sentence is still the retired spelling** (``[route.ingress]``,
  `allow_raw = true`) in `validate.rs:1002`; a Blastfile reader sees a TOML table that does not exist.
- **§18's `<path>` prefix stays the caller's** — `from_blastfile_str` takes a `&str` and has no path,
  so `Display` renders `<line>:<col>: <message>` and the caller prefixes the path exactly once. Any
  future caller must keep that split.
- **Design sentences this leaf had to interpret:**
  - The fixtures are **one flat directory**, not "one directory per row" as `NODE.md` phrases it:
    §20 pins "15 fixtures" and `MB.21.3.4`/`3.6`/`5.1` glob `fixtures/*` and assert the count, so a
    directory per row would recurse and lose the number. Recorded in `common/mod.rs:120–130`.
  - `index` defaults to `index.html` when a `static` block omits it (`DEFAULT_INDEX`, `lower.rs:44`,
    `843`); §9.1 says `root` is required but does not say `index` is.
  - An unnamed store is `db` (`DEFAULT_STORE`, `lower.rs:41`); a second `db` is `DuplicateProxyName`.
  - `functions pg_catalog.unaccent` is three CST siblings (`Word Dot Word`), so lowering walks the
    block in source order with a `joined` flag rather than one name per `Word` (the `LESSONS.md` line).
