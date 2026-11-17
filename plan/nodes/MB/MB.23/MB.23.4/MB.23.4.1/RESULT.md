# MB.23.4.1 — the codec and the Python reporter, reviewed and verified

## Summary

**STATUS: complete.** Attempt 1 of `MB.23.4` had already put the shared codec and the whole
Python leg on disk; this node reviewed that work against the parent's bullets, fixed what
verification proved wrong, and pins the claims that had no test at all. Nothing was
restructured.

What verification actually found and changed (6 edits, 5 of them test code):

1. **A flaky critical test.** `the_loaded_file_report_is_exact` dropped `view.chan` as soon
   as the boot datagram arrived, i.e. before the view's `READY` frame — under `make check`'s
   parallel load `serve` then returned `Io(BrokenPipe)` and the suite failed. It now reads
   `READY` first (30 s bound) and the `expect("serve returns")` stays strict. 25/25 clean
   runs, and 12/12 with four CPU burners running.
2. **No test proved the socket is non-blocking** — the `forbid(unsafe_code)` spelling of
   `MSG_DONTWAIT` and the entire "a request never waits for reporting" guarantee. New
   `loaded::tests::a_full_peer_buffer_drops_the_batch_instead_of_blocking`: a peer that never
   reads returns `EAGAIN`, the batch is dropped and `resync` is armed. Verified by mutation:
   deleting `socket.set_nonblocking(true)?` makes it fail after 60 s with
   `a non-blocking send returns when the peer's queue is full: Timeout`.
3. **Nothing pinned "no I/O, no import, no path resolution inside the hook"**, which is a
   measured rule (research §2.1), not a style preference. New
   `runtime::tests::the_open_hook_does_no_io_no_import_and_no_path_resolution` scans the
   `LOADED_TRACKER` source. Verified by mutation: adding `os.path.realpath(args[0])` to
   `_record_open` fails it with `the hook may not use "os.path"`.
4. **Two rows of the resolve order had no assertion**: a namespace package's search
   directory, and the standard-library drop. Both are now asserted in the critical test and
   both are mutation-verified (`if False and locations:` →
   `a namespace package's search directory is in the boot harvest: […ns_pkg absent…]`;
   `if False and _stdlib is not None:` → `no standard-library path is reported
   (/usr/lib/python3.13): […/usr/lib/python3.13/importlib/__init__.py…]`).
5. **The boot ordering had no witness.** New second `#[test]` (source scan of `stage1.rs`,
   no interpreter) pins install-before-`import_callable`, install-before-`preload`, and
   `BLASTIT_WARMUP` < harvest < `CLOSE_AND_COLLECT`. Plus one cosmetic fix: attempt 1 had
   glommed the `§9.2 #7` comment onto a closing brace in `stage1.rs`; restored, so the
   stage1 diff is behaviour plus comments only.

Everything else the parent claimed held up under review, including the honest deviations it
recorded (`serve_reporting` instead of a `ViewConfig` field, 64 KiB instead of the design's
32 KiB, `set_nonblocking` instead of `MSG_DONTWAIT`). Two deviations were *not* fixed and are
recorded under Follow-ups with the reason (the missing `origin → realpath` memo; the two
resolve-order rows that still have no fixture).

## Evidence

### Acceptance, verbatim tails

```console
$ cargo test -p blastit-view --locked loaded
test loaded::tests::a_full_peer_buffer_drops_the_batch_instead_of_blocking ... ok
…
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 59 filtered out; finished in 0.02s

$ cargo test -p blastit-view --locked --doc
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
all doctests ran in 0.15s; merged doctests compilation took 0.15s

$ cargo test -p blastit --locked --features python --test critical_loaded_files
running 2 tests
test the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup ... ok
test the_loaded_file_report_is_exact ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

$ cargo clippy -p blastit-view --all-targets --locked -- -D warnings
    Checking blastit-view v0.2.0 (…/.worktrees/release-0.2.0/crates/blastit-view)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.88s

$ git diff --stat Cargo.toml Cargo.lock
(no output: both byte-identical)

$ uv run --no-project tools/critical_guard.py --base 9e23dc5 --node MB.23.4.1
critical-guard: 9e23dc5..working tree, 7 changed file(s)
node MB.23.4.1: paths ['crates/blastit-view/src/loaded.rs', 'crates/blastit-view/src/lib.rs',
  'crates/blastit-view/src/runtime.rs', 'crates/blastit-view/src/request.rs',
  'crates/blastit/src/stage1.rs', 'crates/blastit/tests/critical_loaded_files.rs']
  no change to a protected critical test
guard exit=0
```

### `make check` — the one known pre-existing failure, and nothing else

```console
$ timeout 3600 make check
… rs-fmt, rs-lint, rs-doc, rs-test, test-critical (critical_loaded_files, both tests),
  py-lint, docs-unit, docs-check … 173 × `test result: ok`, 0 × `FAILED` …
FAIL  docs/reference/cli.md  fence 1  dev-flags             0.2s  blastit dev exited before ready; stdout:

stderr:
blastit dev: unknown argument `--no-reload`
48 fences: 30 executed, 18 accounted, 1 failed · 26.2s
make: *** [Makefile:365: docs-test] Error 1
exit=2
```

`docs-test` is `--no-reload`, which is **MB.23.5's** (`MB.23.4.1`'s own gate is `dev.is_some()`
and its `paths` cannot add a `dev` flag). Because it stops the chain, every target after it was
run on its own — all green:

```console
$ for t in rs-no-python seam-check platform-matrix-check workflow-check \
           packaging-gates-test doc-tone manifest-purity zed-check plan-validate; do make $t; done
rs-no-python exit=0            # default build: 0 pyo3 dependencies (no libpython linked)
seam-check exit=0
platform-matrix-check exit=0   # platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared …
workflow-check exit=0          # workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test exit=0    # packaging-gates-test: ok — 49 cases passed
doc-tone exit=0                # doc-tone: ok — 30 user-facing page(s), no banned phrase
manifest-purity exit=0         # manifest-purity self-test: ok — 82 assertions, 37 fixtures proved
zed-check exit=0               # OK
plan-validate exit=0           # plan OK
```

`grep -c 'test result: ok'` over the `make check` log: **173**, and `grep 'test result: FAILED'`:
none. `cargo fmt --all -- --check` clean (one `cargo fmt --all` was run, and it touched only
this node's files).

### The parent's bullets, one by one — and the test that pins each

| Requirement | Pinned by |
|---|---|
| codec field round-trip (`version`/`flags`/`source`/`seq`/`count`/paths, 4096-byte and non-ASCII paths) | `loaded::tests::every_field_round_trips`, `an_empty_batch_round_trips` |
| truncated batch refused | `a_truncated_batch_is_refused` (every prefix of a valid datagram, plus a record with its `len` but not its bytes) |
| `version != 1` refused | `another_version_is_refused` (`Version(2)`), `undefined_flag_bits_are_refused` |
| `count` above the cap refused | `a_count_above_the_cap_is_refused` (65), `the_encoder_refuses_a_batch_above_the_count_cap` |
| `len` above 4096 refused | `a_len_above_the_cap_is_refused` (4097) |
| a batch above 64 KiB refused | `a_batch_above_the_byte_cap_is_refused` (decoder **and** encoder) |
| `seq` gaps and `resync` surfaced, not dropped | `a_sequence_reports_a_gap_and_a_resync_instead_of_swallowing_them` (`Step::rebuilds()` asserted), `a_first_delta_from_an_unknown_source_is_a_gap` |
| `MSG_DONTWAIT`: `EAGAIN`/`EMSGSIZE` drop the batch and set `resync` | **new** `a_full_peer_buffer_drops_the_batch_instead_of_blocking` (real `EAGAIN`, real full queue) + `a_failed_send_is_dropped_and_arms_resync` (a dead peer: any send error takes the same branch). `EMSGSIZE` is unreachable *through* `Reporter` by construction — `chunks` caps a datagram at 64 KiB — which `chunks_split_at_the_byte_cap` pins. |
| `__spec__.origin` in the measured order, never `__file__` for a frozen module | rows 1/2 in `the_loaded_file_report_is_exact`: `sys` (built-in) contributes nothing, and `!paths.ends_with("/os.py")` — `os.__file__` is a real stdlib path, so a `__file__` fallback would show up here. Row order itself is `runtime.rs::_module_paths`, one branch per measured row. |
| `.pyc` → `.py` | `the_loaded_file_report_is_exact`: an opened `__pycache__/legacy.cpython-313.pyc` reports `legacy.py` **exactly** (`assert_eq!(paths, vec![legacy.py])`). |
| namespace packages report their search directories | **new** in `the_loaded_file_report_is_exact` (`import ns_pkg` → the directory is in the boot batch) |
| `open`-only hook; a `str` path; `mode` may be `None` | The hook never reads `args[1]`, and the test exercises `mode = None` anyway: `os.open(target, …)` audits `(str path, None, flags)` — that is where the reported `target` comes from (`io.FileIO(path)` was measured to audit `'r'`, not `None`). Non-`open` events are one equality test and have no distinguishable witness (an `import` event's `args[0]` is a module name, which the suffix predicate drops anyway). |
| no `abspath`/`realpath`/`stat`/I/O/import inside the hook | **new** `runtime::tests::the_open_hook_does_no_io_no_import_and_no_path_resolution` (mutation-verified) |
| `try/except BaseException` | same source test **and** behaviour: a replaced body that raises leaves the application's own `open` creating its file |
| `threading.local()` busy flag | behaviour: a body that opens a file is delivered exactly once (`seen == 1`), plus the source test |
| installed once, before `import_callable` | **new** `the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup`; "once" also by `LoadedTracker::adopt` returning `None` before the one install, and one production call site (`stage1.rs:1067`) |
| … only when reload is enabled | Today's gate is `dev.is_some()`; `serve` passes `None`, and `serve_reporting`'s only non-test caller is the test itself. Deliberately **not** text-pinned: MB.23.5 moves the gate and a protected test it cannot edit would then be wrong. See Follow-ups. |
| boot harvest after `BLASTIT_WARMUP` so `preload` is reported by inheritance | the same source-order test (`config.warmup.as_deref()` < `loaded.harvest(py)` < `py.run(CLOSE_AND_COLLECT, …)`) **and** behaviour: `preloaded.py` is in the view's boot batch |
| warm re-harvest asserts the **call count**, not a timer | `harvest() == Some(0)`, the resolved-map length unchanged, and `flush()` empty — in both `runtime::tests::the_loaded_tracker_installs_once_…` and the critical test |
| `open(3, "w")` does not crash the hook | `the_loaded_file_report_is_exact` (an `int` path is dropped) |
| an exception in the hook never aborts the app's real `open` | `the_loaded_file_report_is_exact` (the file is created) |

### Security shape

* **A reporting failure never fails a request.** `LoadedReporter::report`, `boot`,
  `LoadedTracker::{install, adopt, harvest, flush}` are `()`/`Option<…>`, every fallible step
  is `.ok()?`, `Reporter::send` returns `Sent` and never blocks (new full-queue test). The call
  site in `request.rs` discards a `()` — there is no error type to propagate.
* **Nothing is reachable from a production boot.** `stage1::load` installs only under
  `dev.is_some()`; `serve(py, chan, app, cfg)` delegates with `None`; `serve_reporting`'s only
  non-definition caller in the tree is the test.
* **No `unwrap`/`expect`/`panic!`/unchecked indexing outside `#[cfg(test)]`** in the five
  source files: every slice read in `decode` is preceded by a `have < need` guard, and
  `encode`'s two `unwrap_or` are saturating, not panicking.
* **No new dependency**, no `unsafe`: `Cargo.toml`/`Cargo.lock` byte-identical, `loaded.rs`
  imports `std` only, and `blastit-view` stays `#![forbid(unsafe_code)]` (hence
  `set_nonblocking` + `send` as the spelling of `MSG_DONTWAIT`).
* **The hook does no I/O and no import** — now a test, not a comment.

## Files

| File | Change |
|---|---|
| `crates/blastit-view/src/loaded.rs` | review: +1 test `a_full_peer_buffer_drops_the_batch_instead_of_blocking` (the non-blocking/`EAGAIN` witness). Codec untouched. |
| `crates/blastit-view/src/runtime.rs` | review: +1 source-scan test `the_open_hook_does_no_io_no_import_and_no_path_resolution`. `LOADED_TRACKER`/tracker/Rust glue untouched. |
| `crates/blastit-view/src/lib.rs` | unchanged by this node (attempt 1's `serve_reporting` delegation reviewed, correct). |
| `crates/blastit-view/src/request.rs` | unchanged by this node (the post-`serve_request`, pre-`DONE` report site reviewed, correct). |
| `crates/blastit/src/stage1.rs` | review: restored the `§9.2 #7` comment attempt 1 had appended to a closing brace (no behaviour change; install/harvest sites untouched). |
| `crates/blastit/tests/critical_loaded_files.rs` | review: await `READY` before closing `view.chan` (flake fix); namespace-package and stdlib-drop assertions; second `#[test]` pinning `stage1.rs`'s install/harvest ordering; header updated to say one test touches the interpreter. |

Attempt 1's code is otherwise byte-identical: `git diff` for `lib.rs`, `request.rs` and the
two code hunks of `stage1.rs` is exactly what the parent's RESULT describes.

## Follow-ups

1. **`--no-reload` and the zygote wiring are MB.23.5's.** `stage1::load`'s `dev.is_some()`
   gate must also require "reload enabled", and the zygote must call
   `blastit_view::serve_reporting(py, chan, app, cfg, Some(LoadedReport { socket, source }))`
   with the socketpair it creates — the worker end in the child's keep list. Until then no
   view sends anything but the boot batch, and the supervisor has no reader.
2. **A relative `open()` in a view is not attributable.** `_record_open` joins the cwd cached
   at *install* (the boot cwd) while a view runs with `chdir(spawn.dir)` (its private scratch
   dir), so a relative path names a file under the app root rather than the one actually
   opened — and the supervisor drops the non-existent path at `Drop::Missing`. (The parent's
   note says such a path "never lands under the app root"; the cached cwd *is* the app root —
   `config.cwd` and `current_dir()` are the same directory at install — so it lands there and
   is wrong, not out of range. Same outcome: no attribution.) Absolute paths — what Flask,
   Jinja and Django use — are reported. Worth one line in `docs/local.md`/the design's limits
   if a framework is ever found to open templates relatively; the design's "join at flush with
   the cwd cached once at worker start" is what a later leaf would implement.
3. **`os.open()` raises its own `open` event with the string path**, so an `os.open`ed data
   file is reported as well as `open()`; the critical test pins that (a feature, not a leak).
   A future reader of the codec should not "fix" the fd case by scoping it away.
4. **`serve_reporting` exists because `ViewConfig` could not take a `loaded_fd`** without
   editing `crates/blastit/src/zygote.rs` (MB.23.5's file) — a new public field breaks every
   struct literal. `serve` keeps its signature and delegates with `None`.
5. **The `origin → realpath` memo is missing** (only `id(mod)` is memoised). The node asks
   for both; the budget the second memo exists to protect is met without it (a warm harvest is
   a set lookup per module, asserted as a call count), and the cold scan's only win would be
   modules sharing an origin, which each module already pays once. Left as-is rather than
   adding untested caching to reviewed code; if a profile ever shows the cold scan, add it
   with a test that counts `realpath` calls.
6. **Two rows of the resolve order still have no fixture**: row 3 (`spec is None` →
   `module.__file__`, `__main__` from a script) and row 5 (`loader.archive` → the zipapp).
   Both branches exist and both are `realpath` + `isfile`-shaped; a zipapp fixture and a
   `-c`/script `__main__` fixture would close them.
7. **For whoever writes `LESSONS.md`** (out of this node's `paths`), two unrecorded gotchas:
   * pyo3 0.30's `Bound::get_item` **raises** on a missing key and has no `Option`-returning
     form — a test that reads a Python global must go through `.get_item(..)` on a
     `Bound<PyDict>` with two `expect`s, or the "missing" case turns into a `KeyError`
     traceback that reads like a test bug.
   * The interpreter is process-wide, so a second interpreter test in one binary can only
     observe the first one's consumed flushes: keep exactly one `#[test]` per test binary that
     attaches (this file says so in its header, and `runtime.rs`'s lib test is the only one in
     that binary), and put any further leg in a source-scan test that never attaches.

---

# Attempt 2 — the review's one MAJOR and three MINORs, fixed

Base `9e23dc5`. Scope of this attempt: `crates/blastit-view/src/loaded.rs`,
`crates/blastit-view/src/runtime.rs`, `crates/blastit/tests/critical_loaded_files.rs` and this
folder. `lib.rs`, `request.rs` and `stage1.rs` were **not** touched in attempt 2 — attempt 1's
record above still describes them exactly. No new dependency, no `unwrap`/`expect` outside
`#[cfg(test)]`, no `#[ignore]`, no weakened assertion, nothing reachable from a production boot.

## 1 [MAJOR] — `boot` now marks exactly one datagram, and `boot && seq != 0` is refused

`Reporter::send` (loaded.rs:605) computes `let is_boot = boot && sent.datagrams == 0;` inside
the loop, so a boot set larger than `MAX_PATHS` goes out as **one** `boot` datagram (`seq = 0`)
followed by plain deltas whose `seq` continues the stream. A design-following consumer
("`boot` replaces the source's set") therefore keeps every path instead of the last chunk.

The wire can no longer carry an ambiguous boot either: `Batch::new` refuses
`EncodeError::BootNotFirst { seq }`, `decode` refuses `DecodeError::BootNotFirst { seq }`, and
`Sequence::accept` carries a `debug_assert!` plus a doc that states the invariant. `encode`
`debug_assert`s it too. Both refusals mean "drop this datagram", which the consumer already
treats as a gap → rebuild, i.e. conservative.

**Proof (printed from the new test, then removed; the test itself keeps the assertions):**

```console
$ cargo test -p blastit-view --locked a_boot_larger -- --nocapture
PROOF count=65 datagrams=2 boot_at=Some(0) seqs=[0, 1] union=65 input=65
PROOF count=458 datagrams=8 boot_at=Some(0) seqs=[0, 1, 2, 3, 4, 5, 6, 7] union=458 input=458
test loaded::tests::a_boot_larger_than_one_datagram_is_one_boot_followed_by_deltas ... ok
test result: ok. 1 passed; 0 failed; …
```

So a **65-path boot produces 2 datagrams; datagram 0 (the only one) carries `boot` with
`seq = 0`**, datagram 1 is a delta with `seq = 1`, `Sequence` reports no gap, and the
consumer-side union of the chunks equals the input set (65 = 65). The measured real case,
research §2.1's **458-file** Flask/Django view, is 8 datagrams, still exactly one `boot`, and
its union is 458 = 458.

Mutation check (temporary, reverted): `is_boot = boot` → the test fails at
`assert_eq!(sent.datagrams, count.div_ceil(MAX_PATHS))`, because the codec now also refuses the
second flagged chunk — defence in depth, and the test kills the mutation either way.

The invariant implies a wiring rule for `MB.23.5`, now written on `LoadedReporter::boot`: a
reload that keeps the process must adopt a **new** reporter (a boot always starts a new `seq`
stream at 0) rather than send a second boot on the old one, which the codec would drop as
`BootNotFirst` (with `resync` armed, so still safe).

## 2 [MINOR] — the batch cap is the design's 32 KiB again

`MAX_BATCH_BYTES = 32 * 1024` (was 64 KiB), matching design §4.4 ("64-path datagrams *and* at
most 32 KiB encoded, whichever comes first") and staying under the measured ~40 KB in-flight
ceiling. `chunks_split_at_the_byte_cap` now pins both the number (`assert_eq!(MAX_BATCH_BYTES,
32 * 1024)`) and its consequence, using 64 × 1000-byte paths (64 138 B): they fit 64 KiB but
must split at 32 KiB. Mutation check: setting the constant back to `64 * 1024` fails it with
`assertion left == right failed: left: 65536 right: 32768`. The refusal legs
(`a_batch_above_the_byte_cap_is_refused`, `an_empty_batch_round_trips`) are unchanged and still
green; a full 64 × 4096 batch (262 154 B) is still refused on both sides.

## 3 [MINOR] — the two source-scan tests now say what they witness

Neither test was deleted. Both doc comments were rewritten to claim only what a text scan can
claim, and to point at the behavioural test that covers the rest.

* `runtime::tests::the_open_hook_does_no_io_no_import_and_no_path_resolution` now extracts
  **one function's own text** (up to the next top-level `def`) instead of scanning everything
  after `def _record_open(`, and the ban list covers the review's named gaps plus the rest of
  the same family: `os.` (and explicitly `os.path`, `getcwd`, `chdir`, `isfile`, `islink`,
  `exists(`, `.stat(`, `listdir`, `walk(`, `glob`, `readlink`), `subprocess`, `socket`,
  `urllib`, `import `, `logging`, `traceback`, `builtins`, `sys.`, `eval(`, `exec(`,
  `compile(`, `open(`, `print(`, `Path(`. `_audit` is checked separately: its one allowed
  call is `_record_open(` (asserted once), the rest of its body is banned from the same tokens,
  and `if event != "open":`, `except BaseException`, `_busy.on` and `_busy =
  threading.local()` are asserted present. The comment states plainly that this is a
  token-level tripwire, **not behavioural proof** (a body gutted to `pass` would pass; a helper
  could still do I/O), and names `crates/blastit/tests/critical_loaded_files.rs` as the
  behavioural half. Mutation check: `_add(path)` → `_add(os.path.abspath(path))` fails it with
  `the hook may not use "os." (research §2.1)`.
* `the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup` now
  also witnesses the gate, not just the order: exactly **one** `LoadedTracker::install` site,
  lexically inside `let loaded = if dev.is_some() {`, in that block's then-branch (no `else`
  between the gate and the install), no `if false` anywhere in `stage1.rs`, exactly **one**
  `loaded.harvest(py)` site, and the existing `import_callable` / `preload` / `warmup` /
  `CLOSE_AND_COLLECT` ordering. Mutation check: pointing the gate needle at the first
  `if dev.is_some() {` in the file makes it fail with `the install is in the gate's
  then-branch, not its else-branch` — so the new assertion is not vacuous. The doc comment says
  it is a **token-and-order witness, not behavioural proof** (it cannot see control flow), and
  that `MB.23.5`'s `--no-reload` switch is deliberately not pinned there.

## 4 [MINOR] — the EAGAIN leg is cheap and asserts the right thing

`a_full_peer_buffer_drops_the_batch_instead_of_blocking`: the tautological `assert!(sends >= 1)`
is gone (the loop always runs), the channel now carries `(datagrams, resync)` only, and the
assertion is `datagrams > 0 && resync` — a send that never reached the peer would no longer be
mistaken for a full queue. The `recv_timeout` dropped from **60 s to 5 s**, so the regression it
guards (a blocking socket, whose only symptom is silence) costs five seconds of suite time, not
a minute.

## Attempt 2 verification

```console
$ cargo test -p blastit-view --locked loaded
test loaded::tests::a_boot_larger_than_one_datagram_is_one_boot_followed_by_deltas ... ok
test loaded::tests::a_boot_batch_that_does_not_start_the_stream_is_refused_on_both_sides ... ok
test loaded::tests::a_full_peer_buffer_drops_the_batch_instead_of_blocking ... ok
… (23 total)
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 59 filtered out; finished in 0.02s
  (plus runtime::tests::the_open_hook_does_no_io_no_import_and_no_path_resolution … ok —
   it does not match the `loaded` filter, so it is run separately, below)

$ cargo test -p blastit-view --locked the_open_hook
test runtime::tests::the_open_hook_does_no_io_no_import_and_no_path_resolution ... ok
test result: ok. 1 passed; 0 failed; …

$ cargo test -p blastit-view --locked --doc
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
all doctests ran in 0.17s; merged doctests compilation took 0.17s

$ cargo test -p blastit --locked --features python --test critical_loaded_files
running 2 tests
test the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup ... ok
test the_loaded_file_report_is_exact ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

$ cargo clippy -p blastit-view --all-targets --locked -- -D warnings
    Checking blastit-view v0.2.0 (…/crates/blastit-view)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.05s

$ cargo fmt --all -- --check
(no output)

$ git diff --stat 9e23dc5 -- Cargo.toml Cargo.lock
(no output: both byte-identical)

$ uv run --no-project tools/critical_guard.py --base 9e23dc5 --node MB.23.4.1
critical-guard: 9e23dc5..working tree, 7 changed file(s)
node MB.23.4.1: paths ['crates/blastit-view/src/loaded.rs', 'crates/blastit-view/src/lib.rs',
  'crates/blastit-view/src/runtime.rs', 'crates/blastit-view/src/request.rs',
  'crates/blastit/src/stage1.rs', 'crates/blastit/tests/critical_loaded_files.rs']
  no change to a protected critical test
exit=0

$ make check
… guard, critical-guard, rs-fmt, rs-lint, rs-doc, rs-test (workspace), test-critical, py-lint,
  docs-unit, docs-check all pass: 173 × `test result: ok`, 0 × `FAILED` …
FAIL  docs/reference/cli.md  fence 1  dev-flags             0.2s  blastit dev exited before ready; stdout:

stderr:
blastit dev: unknown argument `--no-reload`
48 fences: 30 executed, 18 accounted, 1 failed · 28.2s
make: *** [Makefile:365: docs-test] Error 1
exit=2
```

The single failure is the known pre-existing `--no-reload` fence, `MB.23.5`'s (this node cannot
add a `dev` flag), exactly as the parent recorded; every target that runs before it passed, and
nothing in attempt 2 touches that page.

## Attempt 2 files

| File | Change |
|---|---|
| `crates/blastit-view/src/loaded.rs` | `is_boot` per datagram; `BootNotFirst` on both sides (+ `debug_assert`s); `MAX_BATCH_BYTES` back to 32 KiB; module/`send`/`Sequence`/`LoadedReporter` docs; +2 tests (`a_boot_larger_than_one_datagram_is_one_boot_followed_by_deltas`, `a_boot_batch_that_does_not_start_the_stream_is_refused_on_both_sides`), `chunks_split_at_the_byte_cap` re-tuned to separate 32 KiB from 64 KiB, EAGAIN leg de-tautologised with a 5 s bound. |
| `crates/blastit-view/src/runtime.rs` | hook source-scan test rewritten on per-function bodies with the widened, named ban list + honest doc; `LoadedReporter::boot` doc gains the fresh-`seq`-stream rule. |
| `crates/blastit/tests/critical_loaded_files.rs` | stage1 source-scan test gains the one-install-site / then-branch / no-`if false` / one-harvest witnesses and an honest doc. |

## Attempt 2 follow-ups

1. **`MB.23.5`**: a post-reload boot must come from a fresh `LoadedReporter` (new socket pair
   or at least a fresh reporter, whose `seq` starts at 0); a second `boot()` on a live reporter
   is now a refused batch + `resync`, not a silent set replacement. Also, since `boot` is sent
   only when the flush is non-empty, a view with an empty closure sends no boot at all — the
   supervisor must treat "no boot yet" as unknown rather than empty.
2. **Nothing new for `docs/knowledge/LESSONS.md`**: the 32 KiB-vs-~40 KB `SOCK_DGRAM`
   in-flight fact is research §2.3's already, and `LESSONS.md` is outside this node's `paths`.
