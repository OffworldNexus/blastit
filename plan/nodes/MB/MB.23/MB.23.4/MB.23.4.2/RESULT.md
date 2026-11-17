# MB.23.4.2 — the PHP loaded-file sink: `EG(included_files)` behind an optional `dlsym` symbol

## Summary

**STATUS: complete.** Attempt 1. Base `95b0db4`.

One new **optional** exported symbol, `blastit_php_file_sink_install`, resolved with `dlsym`
on the *shim's own handle* (never `RTLD_DEFAULT`: the shim is `RTLD_LOCAL`). The C shim
snapshots `EG(included_files)` with `ZEND_HASH_FOREACH_STR_KEY` between `php_execute_script`
and `php_request_shutdown`, copies each path's bytes into a bounded Rust buffer, and returns;
the Rust side filters to absolute paths, canonicalises, dedupes and sorts, adds the template
sources its compiled artefacts name, and sends one batch through the sibling's `Reporter`
over the per-view datagram socketpair. No ABI version bump, no `_Static_assert` change,
`blastit_php_sink_v1` untouched, PHP's plain-file wrapper and `open_basedir` untouched.

What is where:

- **`c/blastit_php_sapi.c` / the header** — the sink install (process-wide, two file-scope
  statics), `bt_report_included_files()` (NULL key / zero length skipped), the call site
  after the script and before `php_request_shutdown`, the reset in `worker_shutdown`. The
  header documents it as additive and optional, which is why `BLASTIT_PHP_ABI_VERSION` stays
  `1u`.
- **`src/loader.rs`** — `FileSink` (the `Sized`, `Mutex`-guarded, `Box`ed cell the shim's
  `ctx` points at), `Loaded::install_file_sink(&FileSink) -> bool` (`false` = this shim has no
  sink: no-op forever), and the `extern "C"` callback, which never forms a slice from a NULL
  pointer or a zero length and catches panics so one never unwinds into C's `zend_try` frames.
  `MAX_RECORDED_PATHS` / `MAX_RECORDED_PATH_BYTES` bound the buffer; the bounds and the
  normalisation (`absolute`, `canonicalize`, dedupe, sort) live here so the *shim-contract*
  test can assert them without `blastit-view`.
- **`src/abi.rs`** — `FileIncludedFn` / `FileSinkInstallFn`, declarations only.
- **`crates/blastit-view/src/php.rs`** — `PhpLoadedReport` (socket + source),
  `PhpEmbedRuntime::load_reporting(…, Option<PhpLoadedReport>)` (`load` delegates with
  `None`), the per-request `begin()`/`finish()` around `blastit_php_request`, and the
  compiled→source map (`/**PATH … ENDPATH**/` for Blade, Twig's `getSourceContext()`
  literals), memoised per artefact, bounded reads (4 KiB/file, 128 misses/request, 4096 memo
  entries). The report is sent after the response is flushed and before `DONE`; the first
  batch is the `boot`.
- **`crates/blastit/src/php_view.rs`** — the production child now calls `load_reporting` with
  `None` and a comment naming `MB.23.5` as the socketpair's owner (a `PhpViewSpawn` field,
  whose literal lives in `supervise.rs`, out of this node's paths).
- **`tests/critical_file_sink.rs`** (new, protected) — six tests: ABI/optionality source pins;
  the real-pair leg (include + `.env` data file + bailout, one process-wide `libphp` startup);
  a `cc`-built shim **without** the symbol (no-op, requests still run); a stub **with** it
  (the bytes cross the boundary); a stub that passes `NULL`/`len == 0` (never a slice); the
  sink's own bounds.
- **`Makefile` / `e2e/php-sapi/Dockerfile`** — `rs-test-php` now also runs the new suite
  (`-p blastit-php-sapi`, a separate variable because `--test` resolves per package), and the
  image compiles it into `target/php-suite-executables` with the count check `2 → 3`.

Nothing here is reachable from a production boot: `load`/`serve` pass `None`, and the shim
only walks the table when a sink was installed.

## Evidence

### Acceptance, verbatim tails

```console
$ cargo test -p blastit-php-sapi --locked
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.28s   # critical_file_sink
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s  # critical_shim_contract
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 ignored                      # php_native (its own --ignored harness)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ cargo test -p blastit-view --locked loaded
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 59 filtered out; finished in 0.03s

$ cargo test -p blastit-view --features php --locked --lib php::      # the new unit tests (extra)
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 82 filtered out; finished in 0.02s

$ cargo test -p blastit --locked --features python --test critical_loaded_files   # unedited
test the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup ... ok
test the_loaded_file_report_is_exact ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

$ cargo clippy -p blastit-view -p blastit-php-sapi --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.60s

$ cargo clippy -p blastit-view --features php --all-targets --locked -- -D warnings   # extra: php.rs
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.77s

$ cargo fmt --all -- --check
(no output)

$ git diff --stat 95b0db4 -- Cargo.toml Cargo.lock
(no output: both byte-identical)

$ uv run --no-project tools/critical_guard.py --base 95b0db4 --node MB.23.4.2
critical-guard: 95b0db4..working tree, 12 changed file(s)
node MB.23.4.2: paths ['crates/blastit-php-sapi/c/blastit_php_sapi.c',
  'crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h', 'crates/blastit-php-sapi/src/abi.rs',
  'crates/blastit-php-sapi/src/loader.rs', 'crates/blastit-php-sapi/src/lib.rs',
  'crates/blastit-php-sapi/src/request.rs', 'crates/blastit-view/src/php.rs',
  'crates/blastit/src/php_view.rs', 'crates/blastit-php-sapi/tests/critical_file_sink.rs',
  'Makefile', 'e2e/php-sapi/Dockerfile']
  no change to a protected critical test
guard exit=0
```

`make php-sapi` builds the shim against this host's PHP 8.4.27 headers (8.3/8.5 are the Docker
legs; this host has only 8.4):

```console
$ make php-sapi
php-sapi: cc -> …/target/php-sapi/libblastit_php84-x86_64-gnu.so (/usr/bin/php-config8.4 reports 8.4.27)
php-sapi: skipped 8.5: no php-config for PHP 8.5 …
php-sapi: skipped 8.3: no php-config for PHP 8.3 …
$ nm -D target/php-sapi/libblastit_php84-x86_64-gnu.so | grep file_sink
0000000000001900 T blastit_php_file_sink_install
```

### `make rs-test-php` — the real pair, including the new suite

```console
$ timeout 2400 make rs-test-php        # exit=0
running 6 tests   test result: ok. 6 passed; … (critical_parallel_views)
running 9 tests   … served: HTTP/1.1 200 OK  cpu_ns=20384802 …  test result: ok. 9 passed …
running 27 tests  test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.34s
running 6 tests
served: 2 file(s) after the clean request, 2 after the bailout: ["/tmp/blastit-php-sink-768706-included/bail.php",
  "/tmp/blastit-php-sink-768706-included/lib/helper.php"]
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s
```

The `served:` line is the real evidence: `index.php` + `lib/helper.php` are reported once,
absolute, sorted, the `.env` data file is **not**, and the `exit()` request reports the same
two files — the snapshot runs before `php_request_shutdown`.

### Mutation checks (temporary, reverted; each restored file is byte-identical)

```console
# 1. `if (started) { bt_report_included_files(); }` -> `if (0 && started)`, shim rebuilt
thread 'a_request_reports_what_it_included_and_never_a_file_it_read' panicked at
  critical_file_sink.rs:414: assertion `left == right` failed:
  the entry script is reported exactly once: []    left: 0  right: 1
test result: FAILED. 5 passed; 1 failed

# 2. drop `!Path::new(text).is_absolute()` from FileSink::take
thread 'the_sink_reports_only_paths_the_wire_can_carry' panicked at critical_file_sink.rs:731
test result: FAILED. 5 passed; 1 failed

# 3. drop the compiled→source merge in PhpFileReport::finish
test php::tests::a_compiled_artefact_reports_the_template_it_was_built_from ... FAILED
  left : [blade.php, compiled.php, plain.php]
  right: [blade.php, compiled.php, index.twig, plain.php, welcome.blade.php]
```

### `make check` — the one known pre-existing failure, and nothing else

```console
$ timeout 5400 make check
… 175 × `test result: ok`, 0 × `test result: FAILED` …
FAIL  docs/reference/cli.md  fence 1  dev-flags   0.2s  blastit dev exited before ready; stdout:

stderr:
blastit dev: unknown argument `--no-reload`
48 fences: 30 executed, 18 accounted, 1 failed · 37.0s
make: *** [Makefile:365: docs-test] Error 1
exit=2
```

`docs-test` is `--no-reload`, which is **MB.23.5's**. Because it stops the chain, every target
after it was run on its own — all green:

```console
$ for t in rs-no-python seam-check platform-matrix-check workflow-check \
           packaging-gates-test doc-tone manifest-purity zed-check plan-validate; do make $t; done
rs-no-python exit=0   seam-check exit=0   platform-matrix-check exit=0   workflow-check exit=0
packaging-gates-test exit=0   doc-tone exit=0   manifest-purity exit=0   zed-check exit=0
plan-validate exit=0
```

`RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-view --features php --no-deps` and
`cargo doc -p blastit-php-sapi --no-deps` are clean as well.

## Requirement → witness

| Requirement (node) | Pinned by |
|---|---|
| one **new exported** sink, not a struct member; resolved with `dlsym` through the shim handle | `critical_file_sink::the_sink_is_an_optional_addition_and_the_abi_version_is_unchanged` (header declaration + signature), `an_installed_sink_receives_the_paths_the_shim_reports` (a stub whose only export is the install function is found through the handle), `nm -D` above |
| never `RTLD_DEFAULT` | `Loaded::install_file_sink` looks the symbol up on `self.shim` only; the stub leg proves it (the stub is opened `RTLD_LOCAL`, so a default-scope lookup would find nothing) |
| absent → no-op forever; a PHP build without the symbol behaves as today | `a_shim_without_the_sink_is_a_no_op_and_never_a_failure` (`false`, request still `OUTCOME_OK`, empty sink). In `php.rs`, `install_file_sink` `false` means the runtime keeps `report: None` |
| `ZEND_HASH_FOREACH_STR_KEY`, values `IS_NULL` | the source pin asserts the exact macro; the shim skips a NULL key and a zero length |
| `len == 0` / NULL never form a slice | `a_null_or_empty_path_is_never_a_slice` (a stub calls with both) and the callback's guard |
| filter absolute, canonicalise, dedupe, sort — in Rust | `the_sink_reports_only_paths_the_wire_can_carry` (relative, non-UTF-8 and over-long dropped; the accepted path returned canonical) + mutation 2; `the_report_is_a_boot_then_one_delta_of_sorted_absolute_paths` for the ordering on the wire |
| snapshot **between** `php_execute_script` and `php_request_shutdown`; a bailout still reports | the shim's call site + the bailout half of `a_request_reports_what_it_included_and_never_a_file_it_read` (`contained == 1`, both files reported) + mutation 1 |
| compiled→source map (`/**PATH … ENDPATH**/`, Twig `getSourceContext`) | `the_source_markers_are_read_from_the_artefacts_text`, `a_compiled_artefact_reports_the_template_it_was_built_from`, `the_compiled_to_source_map_is_memoised_per_artefact`, mutation 3 |
| the plain file wrapper and `open_basedir` untouched | the source pin: no `php_plain_files_wrapper`, no `php_resolve_path` in the shim; `IniPolicy` unchanged |
| the fd/wiring lives in Rust over the per-view datagram socketpair | `PhpLoadedReport` + `PhpFileReport` (the `Box`ed `Sized` cell, the `Reporter`, `boot` then deltas) — `the_report_is_a_boot_then_one_delta_of_sorted_absolute_paths` decodes the datagrams. `MB.23.5` supplies the socket (`php_view.rs` passes `None` today) |
| no ABI bump / no `_Static_assert` change / `blastit_php_sink_v1` untouched | `ABI_VERSION == 1` on both sides, the source pin, and `git diff` shows no `_Static_assert` line touched |
| a reporting failure is silent and never fails a request | nothing in the report path returns an error: `record`/`take`/`finish` are infallible, the callback always returns `0`, panics are caught, `Reporter::send` never blocks |
| zero `unwrap`/`expect`/`panic!`/unchecked indexing outside `#[cfg(test)]`; `// SAFETY:` on every `unsafe` block | reviewed: `loader.rs`'s four new `unsafe` sites each carry a `// SAFETY:`; the only `unwrap_or_else` is the poison-recovering `lock`, which cannot panic |
| bookkeeping: `Makefile` + `e2e/php-sapi/Dockerfile` | `rs-test-php` runs `-p blastit-php-sapi --test critical_file_sink` (verified: exit 0), and the Dockerfile records 3 executables (`2 → 3`); the sed/count mechanism was replayed on this host (2 + 1 = 3 lines) |

## Files

| File | Change |
|---|---|
| `crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h` | + `blastit_php_file_sink_install` declaration + docs (additive; version untouched) |
| `crates/blastit-php-sapi/c/blastit_php_sapi.c` | sink statics, install, `bt_report_included_files()`, the snapshot call site, reset in `worker_shutdown` |
| `crates/blastit-php-sapi/src/abi.rs` | `FileIncludedFn`, `FileSinkInstallFn` |
| `crates/blastit-php-sapi/src/loader.rs` | `FileSink` (+ bounds, normalisation, poison-safe `lock`), `Loaded::install_file_sink`, the `file_included` callback |
| `crates/blastit-php-sapi/src/lib.rs` | re-export `FileSink` |
| `crates/blastit-view/src/php.rs` | `PhpLoadedReport`, `PhpFileReport`/`FileReportState`, `load_reporting`, `begin`/`finish` in `serve`, the compiled→source map, 4 unit tests |
| `crates/blastit/src/php_view.rs` | `serve` calls `load_reporting(…, None)`; names `MB.23.5`'s wiring point |
| `crates/blastit-php-sapi/tests/critical_file_sink.rs` | **new** protected suite, 6 tests (8 after the polish pass: two sink-ownership witnesses) |
| `Makefile` | `PHP_SINK_SUITE_ARGS`, `rs-test-php` runs it; comments on the image's 3 executables and the `-ne 3` check |
| `e2e/php-sapi/Dockerfile` | compiles + records the new suite; `-ne 2` → `-ne 3` |

`crates/blastit-php-sapi/src/request.rs` was in the node's paths but needed no change
(`IniPolicy`/`ServerVars` are untouched by this report).

## Follow-ups

1. **[MB.23.5] The socketpair.** `php_view::serve` passes `None`; the zygote must create the
   per-view `AF_UNIX SOCK_DGRAM` socketpair, add the write end to the child's keep list, and
   hand it over as a `PhpViewSpawn` field (that literal is `supervise.rs`, outside this node's
   paths) so `serve` can call `load_reporting(…, Some(PhpLoadedReport { socket, source }))`.
   Until then the PHP half compiles, is installed and is tested, but sends nothing in a real
   boot — exactly the shape `MB.23.4.1` left `serve_reporting` in.
2. **[MB.23.5] `boot` semantics for a reload that keeps the process.** The PHP view's first
   batch is `boot` (`seq = 0`); a re-boot on a live reporter is refused by the codec
   (`BootNotFirst`), so a reload needs a fresh reporter/socketpair. Also: a view that loads
   nothing sends no boot at all — "no boot yet" is unknown, not empty (same rule as the
   Python half).
3. **[out of paths] `docs/knowledge/LESSONS.md`** deserves one line, but the file is not in
   this node's paths: **`libphp` is process-wide, so a test binary may start it exactly once**
   — two `#[test]`s that each `dlopen` a real `libphp` and `php_module_startup` abort the
   process with `zend_mm_heap corrupted` (that is why the real-load leg here is one test with
   two requests, and why the stub `libphp` deliberately exports **no** Zend symbol: it is
   `RTLD_GLOBAL`, and a stub `executor_globals` would let a real shim bind to it).
4. **PHP 8.3 / 8.5** are not installable on this host (`php-config` exists for 8.4 only), so
   `make php-sapi` here built one shim. `EG(included_files)` and
   `ZEND_HASH_FOREACH_STR_KEY` are unchanged across 8.3–8.5, and the Docker legs
   (`debian`/`ubuntu`/`alpine` at 8.3/8.4/8.5) build the same C against each minor; the image
   was not built in this session (`make php-sapi-e2e` needs Docker and ~7 minutes).
5. **The compiled→source map reads 4 KiB of each reported file** (memoised after the first
   read). That is deliberately bounded and dev-only, but if a profile ever shows it, the
   cheaper variant is to gate the scan on a cache-directory prefix the manifest already knows.

## Polish pass

Reviewer verdict was **PASS with five MINOR findings**. All five are addressed below. Nothing
was weakened to get green: `Cargo.toml`/`Cargo.lock` are byte-identical, `BLASTIT_PHP_ABI_VERSION`
is still `1u`, `blastit_php_sink_v1` and every `_Static_assert` are untouched, and the exported
set of `blastit_php_file_sink_install` is unchanged (its signature is byte-identical; only its
doc comment says more).

### 1. `MAX_MAP_ENTRIES` now bounds the memo, as the doc always claimed

`template_sources` inserted on `!sources.is_empty() || memo.len() < MAX_MAP_ENTRIES`, so every
compiled artefact was memoised *past* the cap and a long-lived dev view's map grew with each
fresh artefact. It is now `if memo.len() < MAX_MAP_ENTRIES { … }`: once the map is full nothing
new is remembered. A miss past the bound is still **reported** — it is just rescanned next time,
and one request's rescan budget is `MAX_MAP_FILES` (128). The const's doc now states the rule in
those words (bounded, empties included), so code and doc agree. Witness:
`php::tests::the_compiled_to_source_map_memo_stops_at_its_bound` (memo pre-filled to the bound:
a compiled artefact is still reported, the memo does not grow and does not contain it; with room
it is memoised), beside the untouched `the_compiled_to_source_map_is_memoised_per_artefact`.

### 2. The sink's address: `Loaded` owns it, and "too early" is not "no symbol"

Fixed structurally, not with a comment. `install_file_sink` is now

```rust
pub fn install_file_sink(&mut self, sink: FileSink) -> Result<Installed, NoSink>
```

* **Ownership.** The sink is taken **by value** and moved into a `Box` *before* its address is
  taken; that box is stored in `Loaded`. The shim's `ctx` is the box's pointee, so it cannot move
  when `Loaded` does — into `PhpEmbedRuntime`'s `Mutex`, then its `Arc` — and no caller holds a
  reference it could drop early. A `&FileSink` that outlives the install is not expressible any
  more.
* **No stale static, ever.** `clear_file_sink()` calls `blastit_php_file_sink_install(NULL, NULL)`
  — the header's documented uninstall, the same entry point, so no second symbol and no ABI
  change — **before** the box is dropped, from both `worker_shutdown` and `Drop`. A refused
  install is uninstalled too, so even a shim that stored `ctx` and returned non-zero cannot be
  left naming freed memory.
* **Typed refusal.** `NoSink::{NotInitialised, Absent, Refused { code }}` replaces the `bool`
  that conflated a transient "nothing is running yet" with a permanent "this shim has no
  symbol". Each has a `Display` naming the fix, and `NoSink` implements `std::error::Error`.
* **The per-request cell.** `Loaded::file_sink()` borrows the same allocation for
  `clear`/`take`; it is the only way to reach it from outside, so a second owner cannot exist.
  `PhpFileReport` lost its `Box<FileSink>` and its `begin`/`finish` take `&FileSink` borrowed
  from the loader in `serve`; `load_reporting` installs `FileSink::new()` and keeps the report
  only on `Ok` (both `NoSink` cases stay silent, as the node requires).
* **C side.** Comments only: the header and the shim now state that `ctx` is the caller's memory
  and must outlive the install, and that a non-zero return leaves nothing installed. Signature,
  optionality and ABI version untouched.

Witnesses (both in the protected suite):

* `the_installed_sink_outlives_a_request_and_a_moved_loader` — install, one request reports, the
  loader is **moved into a `Box`** after the hand-off, a second request still reports into the
  same allocation, `take` drains it, and `worker_shutdown` drops the sink
  (`file_sink()` is `None` afterwards).
* `installing_before_the_worker_starts_is_not_the_same_answer_as_a_missing_symbol` —
  `Err(NoSink::NotInitialised)` before `worker_init`, the *same* handle accepts the sink after
  it, and a shim without the entry point answers `Err(NoSink::Absent)`.

`crates/blastit/src/php_view.rs` needed no change at its call site: it passes `None` (production
and `--no-reload`), which never reaches `install_file_sink` at all — the install call site is
`PhpEmbedRuntime::load_reporting` in `blastit-view/src/php.rs`, updated above.

### 3. The vacuous no-symbol assertion is now load-bearing

`sink.take().is_empty()` in `a_shim_without_the_sink_is_a_no_op_and_never_a_failure` could never
fail: the sink had been refused, so it was never handed to the shim and no request could fill
it. The line is gone (with a comment saying why that check is vacuous) and two assertions that
*can* fail took its place: the install returns `Err(NoSink::Absent)`, and `loaded.file_sink()` is
`None` — the shim was never handed an address at all. The request on that shim still returns
`OUTCOME_OK`, which is the "optional forever" half.

### 4. Bookkeeping wording

`Makefile:726` no longer says "the two PHP legs": the image records and runs **three** suites
(`-ne 3`), and the target now reads `## Docker: all three PHP suites, in the image that built
them (what CI runs)`. The overlong comment line at `e2e/php-sapi/Dockerfile:45` is re-wrapped
(and still says the image compiles the protected suites).

### 5. Reviewer finding 5, in substance and kept as a gate condition

For PHP 8.3 and 8.5 the sink is **claimed-but-unproven by execution, structurally sound**: this
host has `php-config` for 8.4 only, so `make php-sapi` built one shim (8.3/8.5 skipped — output
below), and no `make php-sapi-e2e` image was built in either session. `EG(included_files)` and
`ZEND_HASH_FOREACH_STR_KEY` are unchanged across those minors and the Docker legs compile this
same C against each, but that is an argument, not a run. **The milestone gate must run
`make php-sapi-e2e` on all three minors (debian/ubuntu/alpine at 8.3, 8.4, 8.5) before `MB.23.5`
relies on this report.** Until then the PHP half is proven by execution on 8.4 only.

### Verification after the polish pass

All commands run on this tree, in this order; `make check`'s only failure is the known
pre-existing `docs/reference/cli.md` fence 1 (`--no-reload`, `MB.23.5`'s).

```console
$ cargo test -p blastit-php-sapi --locked
running 5 tests           test result: ok. 5 passed; 0 failed; 0 ignored; …
running 8 tests           test result: ok. 8 passed; 0 failed; 0 ignored; … finished in 0.27s   # critical_file_sink (was 6)
running 15 tests          test result: ok. 15 passed; 0 failed; 0 ignored; …                  # critical_shim_contract
running 2 tests           test result: ok. 0 passed; 0 failed; 0 ignored; … 2 ignored          # php_native (its own --ignored harness)
running 1 test            test result: ok. 1 passed; 0 failed; 0 ignored; …                   # doctest

$ cargo test -p blastit-view --locked loaded
running 23 tests          test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 59 filtered out

$ cargo test -p blastit-view --features php --locked --lib php::      # extra: the new memo-bound witness
running 22 tests          test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 82 filtered out

$ cargo test -p blastit --locked --features python --test critical_loaded_files   # unedited
test the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup ... ok
test the_loaded_file_report_is_exact ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

$ cargo clippy -p blastit-php-sapi -p blastit-view -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.03s

$ cargo fmt --all -- --check
(no output)

$ make rs-test-php        # exit=0; the shim was rebuilt after the header comment change
php-sapi: cc -> …/target/php-sapi/libblastit_php84-x86_64-gnu.so (/usr/bin/php-config8.4 reports 8.4.27)
php-sapi: skipped 8.5: no php-config for PHP 8.5 … ; skipped 8.3: no php-config for PHP 8.3 …
running 6 tests           test result: ok. 6 passed; 0 failed; 0 ignored; … (critical_parallel_views)
running 9 tests           test result: ok. 9 passed; 0 failed; 0 ignored; … (critical_php_backend)
running 27 tests          test result: ok. 27 passed; 0 failed; 0 ignored; … (critical_php_embed)
running 8 tests           served: 2 file(s) after the clean request, 2 after the bailout: ["…/bail.php", "…/lib/helper.php"]
                          test result: ok. 8 passed; 0 failed; 0 ignored; … (critical_file_sink)

$ git diff --stat 95b0db4 -- Cargo.toml Cargo.lock
(no output: both byte-identical)

$ uv run --no-project tools/critical_guard.py --base 95b0db4 --node MB.23.4.2
critical-guard: 95b0db4..working tree, 12 changed file(s)
node MB.23.4.2: paths ['crates/blastit-php-sapi/c/blastit_php_sapi.c', …]
  no change to a protected critical test
guard exit=0
```

`make check` (175 test binaries ok, 0 FAILED, and the one known failure):

```console
$ timeout 5400 make check
… 175 × `test result: ok`, 0 × `test result: FAILED` …
FAIL  docs/reference/cli.md  fence 1  dev-flags   4.3s  blastit dev exited before ready; stdout:

stderr:
blastit dev: unknown argument `--no-reload`
48 fences: 30 executed, 18 accounted, 1 failed · 34.6s
make: *** [Makefile:365: docs-test] Error 1
make check exit=2
```

`docs-test` is `--no-reload`, which is **MB.23.5's**. Because it stops the chain, every target
after it was run on its own on this tree — all green:

```console
$ for t in rs-no-python seam-check platform-matrix-check workflow-check \
           packaging-gates-test doc-tone manifest-purity zed-check plan-validate; do make $t; done
rs-no-python exit=0   seam-check exit=0   platform-matrix-check exit=0   workflow-check exit=0
packaging-gates-test exit=0   doc-tone exit=0   manifest-purity exit=0   zed-check exit=0
plan-validate exit=0

$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-php-sapi --no-deps            # clean
$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-view --features php --no-deps # clean
```

