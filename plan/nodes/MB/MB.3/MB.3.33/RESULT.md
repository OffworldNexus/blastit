# RESULT MB.3.33 — static root never serves `.php`/`.phtml`/`.phar` source

**Status:** done. Base `f4bfcbe`, branch `release/0.2.0` (not committed).

## What changed

- `crates/blastit-scheduler/src/static_asset.rs`
  - `head_segments` refuses a final `.php`/`.phtml`/`.phar` segment with
    `StaticOutcome::Hidden` (404) before admission, so `POST /index.php` is
    `404`, never `405`, and a body-bearing request is `404`, never `413`.
  - `StaticRoot::probe` applies the same gate before any `openat2`/`openat`, so
    `resolve` (and therefore the directory `index` and SPA `index` candidates)
    never probes or read-opens source; a `000`-mode `.php` is `404`, not `500`.
  - `is_php_source` is the one rule: final `.`-separated extension `php`,
    `phtml` or `phar`, ASCII case-insensitive.
- `crates/blastit-scheduler/src/static_response.rs`
  - Defence in depth in `respond`: a forged `StaticFile` whose *last non-empty
    URL-path segment* is PHP source answers the existing `404` refusal
    (`Content-Length: 0`, `Cache-Control: no-store`, no body, descriptor
    dropped); a trailing `/` or query cannot hide it.
- `crates/blastit-scheduler/tests/critical_static_entry.rs` (new, protected)
  - `.php`, `.PHP`, `.Php`, `.PhAr`, `.phar`, `.phtml`, `.PHTML` -> 404 for
    `GET`, `HEAD`, `POST` and `POST` with a declared body;
  - `000`-mode `.php` -> 404, with the `resolve_with` hook proving no descriptor
    reached the read open;
  - a directory literally named `dir.php`: `/dir.php` and `/dir.php/` -> 404,
    `/dir.php/index.html` -> 200 exact bytes (`text/html`);
  - forged `url_path` rows `/x.php/`, `/x.php?y=1`, `/INDEX.PHP/`, `/x.phtml` ->
    404 with no body;
  - positive twin `/assets/app.css` -> 200 with exact bytes and `text/css`;
    `/docs/manual.php.txt` -> 200; dotfile `/.env` -> 404; traversal -> 400;
    `POST` -> 405 and declared body -> 413 on the asset;
  - directory/SPA `index = "index.php"` never answers `/`, `/docs`, `/dashboard`.
- `docs/frameworks/php.md` (150 lines) and `docs/reference/manifest.md` (400
  lines): the true sentence is back — `.php`/`.phar`/`.phtml` files under a
  static root are refused with `404`; only `entry` is executed.

No new dependency, no config knob, no boot-time directory walk, no behaviour
change for non-source files.

## Acceptance evidence

### 1. `cargo test -p blastit-scheduler --test critical_static_entry --locked`

```
$ cargo test -p blastit-scheduler --test critical_static_entry --locked
entry=0
running 6 tests
test a_source_file_is_hidden_at_resolve_without_any_read_open ... ok
test an_unreadable_php_file_is_404_not_a_read_failure ... ok
test a_source_index_never_answers_the_directory_or_the_spa_fallback ... ok
test a_normal_asset_is_still_served_exactly_and_its_gates_are_unchanged ... ok
test a_forged_source_url_path_is_never_streamed ... ok
test php_phtml_and_phar_final_segments_are_404_before_the_method_and_body_gates ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 2. `cargo test -p blastit-scheduler --locked`

```
$ cargo test -p blastit-scheduler --locked
scheduler=0
16 "test result: ok" suite/doctest lines; 0 failed
tail:
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
all doctests ran in 0.21s; merged doctests compilation took 0.20s
```

### 3. `cargo test -p blastit --locked`

```
$ cargo test -p blastit --locked
blastit=0
12 "test result: ok" suite/doctest lines; 0 failed
tail:
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 4. `make check`

```
$ make check
make-check=0
zero FAILED / `test result: FAILED` / `error` lines
tail:
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
```

### 5. `make docs-check`

```
$ make docs-check
docs=0
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.29s
```

### 6. Protected-test guard

```
$ uv run --no-project tools/critical_guard.py --base f4bfcbe --node MB.3.33
guard=0
critical-guard: f4bfcbe..working tree, 8 changed file(s)
node MB.3.33: paths [ ... 'crates/blastit-scheduler/tests/critical_static_entry.rs', ... ]
  no change to a protected critical test
```

## Mutation proof

With both gates neutralised (`is_some_and(is_php_source)` ->
`is_some_and(|_| false)` in `probe` and `head_segments`; `respond`'s defence left
intact), the protected suite fails:

```
$ cargo test -p blastit-scheduler --test critical_static_entry --locked
mutant=101
failures:
    a_normal_asset_is_still_served_exactly_and_its_gates_are_unchanged
    a_source_file_is_hidden_at_resolve_without_any_read_open
    a_source_index_never_answers_the_directory_or_the_spa_fallback
    an_unreadable_php_file_is_404_not_a_read_failure
    php_phtml_and_phar_final_segments_are_404_before_the_method_and_body_gates
test result: FAILED. 1 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out
```

Restored byte-identical (`cmp`), `cargo fmt --all -- --check` clean, suite back
to `6 passed`.

## Follow-ups

- **Pre-existing flake, not this node:** `crates/blastit-php-sapi/tests/critical_shim_contract.rs::an_unsupported_minor_is_refused_by_name`
  failed once during the first post-change `make check` (`expected
  UnsupportedVersion, got NotFound`). `FakeConfig` names its tempdir with
  `pid + SystemTime::now().as_nanos()`, so parallel test threads can collide and
  one `Drop` removes the other's `php-config8.4`. It passed on retry, 4/4
  standalone runs, and every later full `make check`. Suggest a test-curator
  fix (atomic counter in the fixture name); the file is outside this node.
- `MB.9.2.2` must keep this gate in its `plan` and keep the protected cases (or
  move them to the conformance suite unchanged).
- `MB.3.8`'s release checklist already carries the verbatim 0.2.0 certification
  line (verified in `plan/nodes/MB/MB.3/MB.3.8/NODE.md:109`).
