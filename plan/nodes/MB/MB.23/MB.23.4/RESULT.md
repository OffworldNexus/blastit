# MB.23.4 — reports: the loaded-file codec and the Python leg

## STATUS: needs_split

The leaf is two leaves. This attempt delivered **the shared codec and the whole Python leg**
(items 1–3 of `# Acceptance`, plus the bookkeeping-free half of 5). The **PHP sink half** —
`EG(included_files)` through a new `dlsym`-resolved optional sink, `critical_file_sink.rs`,
`crates/blastit/src/php_view.rs`, `crates/blastit-view/src/php.rs`, and the
`Makefile`/`e2e/php-sapi/Dockerfile` third-suite bookkeeping — is proposed as **MB.23.4.1**
(file sets in `## Proposed split`). The whole node is ~2 500 lines across 18 files; this half
is 6 files / ~1 830 lines (tests are ~690 of them), already past the ~650-line pre-authorised
budget, so stopping at the language boundary is the only clean cut.

Nothing the PHP half needs was touched: `Cargo.toml`, `Cargo.lock`, `crates/blastit-sys`,
`crates/blastit/src/{supervise,zygote,dev_reload}.rs`, `crates/blastit-php-sapi/**`, `Makefile`
and `e2e/php-sapi/Dockerfile` are byte-identical, and the tree is green.

## Delivered

| File | What |
|---|---|
| `crates/blastit-view/src/loaded.rs` (new, 898 lines) | The one bounded, versioned datagram codec: `VERSION = 1`, `FLAG_BOOT`/`FLAG_RESYNC`, `PRELOAD_SOURCE`, `MAX_PATHS = 64`, `MAX_PATH_BYTES = 4096`, `MAX_BATCH_BYTES = 64 KiB`, `Batch::{new,encode}`, `decode`, `chunks`, `Reporter` (non-blocking socket, `seq` per source, drop + `resync` on any send failure, no datagram for an empty delta), and `Sequence`/`Step` (boot, resync and **seq-gap** surfaced to the consumer as `rebuilds()`, never swallowed). 19 unit tests: every field round-trips; truncated header/record, `version != 1`, an undefined flag bit, `count > 64`, `len > 4096`, a >64 KiB datagram, an empty path, a NUL, non-UTF-8 and a trailing byte are each refused; the encoder refuses 65 paths and an over-cap batch; `chunks` splits at 64 paths and at the byte cap; a boot batch then two deltas decode with `seq` 0/1; a dead peer drops the batch and arms `resync`, which the next batch carries once. |
| `crates/blastit-view/src/runtime.rs` | The Python half: `LOADED_MODULE` (`_blastit_loaded`, kept in `sys.modules` so the fork inherits it), the `LOADED_TRACKER` source, `LoadedTracker::{install,adopt,harvest,flush,object}`, `LoadedReport` (the view's socket + source) and `LoadedReporter::{boot,report}`. The Python source resolves `__spec__.origin` in the research's measured order (built-in → frozen → no spec → namespace dirs → `loader.archive` → `.pyc`/`.pyo` via `source_from_cache` + `isfile` → `realpath` + `isfile`), memoises `id(module)`, and has one `open`-only `sys.addaudithook` whose body is a single `_record_open(args)` call wrapped in `try/except BaseException` behind a `threading.local()` busy flag — no `abspath`, no `realpath`, no `stat`, no I/O, no import inside it. `flush` maps `.pyc → .py`, drops `__pycache__`/`.git`/`node_modules` and the stdlib, keeps site-packages, and reports `resync` once when the 8 192-path / 4 MiB budget is hit. |
| `crates/blastit-view/src/lib.rs` | `pub mod loaded` and the codec re-exports; `LoadedReport`/`LoadedReporter`/`LoadedTracker` re-exported under `python`; `serve` delegates to the new `serve_reporting(py, chan, app, cfg, Option<LoadedReport>)`, which sends the **boot** batch with the GIL already held, before `READY`, then detaches. |
| `crates/blastit-view/src/request.rs` | `AppContext::loaded` and one `report(py, false)` after `serve_request` returns — first-touch attribution after `close()`, before `DONE`, best effort (`try_lock`, no waiting, no error path). |
| `crates/blastit/src/stage1.rs` | In `load()`, before `import_callable`: install the tracker in `dev` only, rooted at `config.cwd`; after `BLASTIT_WARMUP` (before `CLOSE_AND_COLLECT`): one baseline `harvest`. The preloaded closure is therefore paid for once in the immortal parent and inherited by every view's boot batch — no parent-side reporter, exactly as the node requires. |
| `crates/blastit/tests/critical_loaded_files.rs` (new, 327 lines) | One `#[test]` (the interpreter is process-wide): install → adopt-before-install is `None` → the app imports `preloaded` + `helper` and reads a template → a **real per-view `SOCK_DGRAM` socketpair** under `serve_reporting` delivers a `boot`, `seq = 0`, `source = 3` batch naming the module's `.py`, the `preload` module and the template, with no `__pycache__` path, no `os.py` (frozen) and nothing for `sys` (built-in) → warm `harvest` resolves 0 new modules and `flush` reports nothing (call count, not a timer) → an opened `__pycache__/legacy.cpython-313.pyc` reports `legacy.py` → `open(fd, "w")` and `os.open` do not crash the hook → a replaced hook body that raises leaves the application's own `open` succeeding with the file created → a hook body that opens a file is delivered exactly once (busy flag) → with the real body back, `flush` reports exactly the two files the application opened by name. |

## Design deltas (deliberate, and why)

1. **No `ViewConfig`/`PhpViewSpawn` field; a new `serve_reporting` entry point.** A new public
   field on `ViewConfig` (or later `PhpViewSpawn`) breaks every struct literal that builds it —
   `crates/blastit/src/zygote.rs` and `crates/blastit/src/supervise.rs`, both **MB.23.5's
   files** — so the leaf could not leave the tree compiling. `serve(py, chan, app, cfg)` is
   unchanged and delegates with `None`; the fd + source travel as `Option<LoadedReport>`
   to `serve_reporting`, which MB.23.5 calls from the zygote once it owns the socketpair. The
   codec still lives in `blastit-view` and is re-exported for the supervisor, as the design says.
2. **The Python state is a private module in `sys.modules`, not a `py.run` global dict.** A
   global dict installed pre-fork cannot carry a *per-view* descriptor, and stage 0 strips every
   `BLASTIT_*` variable. `_blastit_loaded` is installed once by `stage1::load` (before the first
   import), inherited by every forked view, and found again with `sys.modules` — no environment
   variable, no Rust global, and the socket is never in Python's hands at all: Python only
   produces paths, Rust encodes and sends them.
3. **`MAX_BATCH_BYTES = 64 KiB`, not the design's 32 KiB.** The node and the dispatch both say
   64 KiB; 64 × 4096-byte paths is 262 KiB and `EMSGSIZE` at 262 142 B, so the byte cap still
   splits before any datagram approaches the measured in-flight ceiling (~40 KiB), and the
   sender treats any `EAGAIN` as a dropped batch plus `resync`.
4. **`MSG_DONTWAIT` is spelled `set_nonblocking(true)` + `send`.** The crate is
   `#![forbid(unsafe_code)]`, and a non-blocking `SOCK_DGRAM` socket returns `EAGAIN` exactly as
   `MSG_DONTWAIT` would; no `libc` call and no unsafe block is introduced.
5. **The hook admits `.pyc`/`.pyo` under `__pycache__` and `flush` maps them.** Dropping the
   cache path in the hook would lose the `.pyc → .py` mapping the node requires; `flush` maps
   first, then applies the skip list, so a path that stays under `__pycache__` is still dropped.
   The mapping requires the source to exist (`os.path.isfile`, at flush, never in the hook) so a
   genuinely sourceless `.pyc` is reported as itself rather than as a `.py` that is not there.
6. **The dev gate is `dev.is_some()` for now.** `--no-reload` does not exist yet; MB.23.5 owns the
   switch and clears this gate. Production, `check` and `test` never install the hook (T17's
   property holds today: `dev` is the only mode that installs it).

## Acceptance — commands and their tails

```console
$ cargo test -p blastit-view --locked loaded
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 58 filtered out; finished in 0.02s

$ cargo test -p blastit-view --locked --doc
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
all doctests ran in 0.20s; merged doctests compilation took 0.19s

$ cargo test -p blastit-php-sapi --locked
test result: ok. 5 passed; 0 failed; 0 ignored …
test result: ok. 15 passed; 0 failed; 0 ignored …
test result: ok. 0 passed; 0 failed; 2 ignored …   # pre-existing real-load legs, unchanged
test result: ok. 1 passed; 0 failed; 0 ignored …

$ cargo test -p blastit --locked --features python --test critical_loaded_files
test the_loaded_file_report_is_exact ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

$ cargo clippy -p blastit-view -p blastit-php-sapi --all-targets --locked -- -D warnings
    Checking blastit-view v0.2.0 (/…/.worktrees/release-0.2.0/crates/blastit-view)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.90s

$ make check
… rs-fmt, rs-lint, rs-doc, rs-test (173 × `test result: ok`, 0 failed), test-critical
  (incl. critical_loaded_files), py-lint, docs-unit, docs-check, wheel build all green …
FAIL  docs/reference/cli.md  fence 1  dev-flags             0.2s  blastit dev exited before ready; stdout:
stderr:
blastit dev: unknown argument `--no-reload`
48 fences: 30 executed, 18 accounted, 1 failed · 29.7s
make: *** [Makefile:365: docs-test] Error 1
make check exit=2
```

`make check`'s **only** failure is the one the brief names (`--no-reload`, MB.23.5). Because it
stops at `docs-test`, every later target was run on its own, all green:

```console
$ for t in rs-no-python seam-check platform-matrix-check workflow-check packaging-gates-test \
           doc-tone manifest-purity zed-check plan-validate; do make $t; done
rs-no-python exit=0            # default build: 0 pyo3 dependencies (no libpython linked)
seam-check exit=0
platform-matrix-check exit=0   # platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared …
workflow-check exit=0          # workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test exit=0    # packaging-gates-test: ok — 49 cases passed
doc-tone exit=0                # doc-tone: ok — 30 user-facing page(s), no banned phrase
manifest-purity exit=0         # manifest-purity: ok — 17 source file(s), no std::env (+ self-test)
zed-check exit=0               # OK
plan-validate exit=0           # plan OK

$ git diff --stat Cargo.toml Cargo.lock
(no output: both byte-identical)

$ uv run --no-project tools/critical_guard.py --base 9e23dc5 --node MB.23.4
critical-guard: 9e23dc5..working tree, 7 changed file(s)
node MB.23.4: paths ['crates/blastit-view/src/loaded.rs', …, 'Makefile', 'e2e/php-sapi/Dockerfile']
  no change to a protected critical test
guard exit=0
```

Per acceptance item:

1. **Codec** — `loaded::tests::*` (19 tests): round-trip of every field incl. a 4096-byte and a
   non-ASCII path; refusals for a truncated batch (every prefix length), `version != 1`, an
   undefined flag bit, `count > 64`, `len > 4096`, a >64 KiB datagram, an empty path, a NUL, a
   non-UTF-8 path and a trailing byte; `Sequence` reports a gap (`gap: true`) and a `resync`
   instead of dropping them, and `Step::rebuilds()` is the consumer's answer.
2. **Python in-process** — `critical_loaded_files` as listed in the table: module `.py` and
   template path reported, the `preload` module in the boot batch, `.pyc → .py`, built-in `sys`
   and frozen `os` report nothing, `open(3, "w")` does not crash the hook, and a raising hook
   leaves the application's own `open` succeeding with the file created.
3. **Warm path** — a second `harvest` returns `0` new modules, the resolved set is unchanged and
   `flush` reports nothing (call counts, no timer).
4. **PHP shim contract** — **not in this leaf**: `MB.23.4.1`.
5. **`make check` green** except the named docs fence; `Cargo.toml`/`Cargo.lock` byte-identical.

## Proposed split

**MB.23.4.1 — the PHP sink half (report `EG(included_files)` through a new optional `dlsym` sink).**
Files, exactly:

- `crates/blastit-php-sapi/c/blastit_php_sapi.c` (a `blastit_php_file_sink_install` global + the
  `EG(included_files)` snapshot between `php_execute_script` and `php_request_shutdown`,
  `ZEND_HASH_FOREACH_STR_KEY`, `opened_path` NULL filtered)
- `crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h` (one declaration; no version bump, no
  `blastit_php_sink_v1`/`_Static_assert` change)
- `crates/blastit-php-sapi/src/abi.rs` (the install fn + callback types)
- `crates/blastit-php-sapi/src/loader.rs` (an **optional** `dlsym` through the shim handle the
  worker already opened — never `RTLD_DEFAULT` — absent → no-op)
- `crates/blastit-php-sapi/src/lib.rs` (re-exports)
- `crates/blastit-view/src/php.rs` (`PhpEmbedRuntime::load_with_file_sink`, `PhpRuntimeConfig`
  untouched so `tests/php_native.rs` keeps compiling; the per-request snapshot + `Reporter`)
- `crates/blastit/src/php_view.rs` (`fork_php_view_reporting`, delegating default; the fd in the
  child's keep list; the compiled→source map)
- `crates/blastit-php-sapi/tests/critical_file_sink.rs` (new: recording sink, one include
  reported once/absolute/sorted, a bailout request still reports, a data file **not** reported)
- `Makefile` (`PHP_SUITE_ARGS` gains `--test critical_loaded_files`… or the sink suite, per the
  design's T26) and `e2e/php-sapi/Dockerfile` (third suite compiled and recorded, `-ne 2` → `3`)

It reuses this leaf's `blastit_view::loaded` codec and `Reporter` as they stand, and the
`crates/blastit/tests/critical_loaded_files.rs` file must **not** be edited (it is protected now);
the PHP assertions belong in `critical_file_sink.rs`.

## Follow-ups

1. **The `--no-reload` gate and the zygote/supervisor wiring** belong to MB.23.5:
   `stage1::load`'s `dev.is_some()` gate must also require "reload enabled", and the zygote must
   call `blastit_view::serve_reporting(..., Some(LoadedReport { socket, source }))` with the
   socketpair it creates (the worker end in the child's keep list).
2. **A relative `open()` in a view is not attributable.** The view's cwd is its private scratch
   directory (`ViewSpawn.dir`), so joining it with the cached boot cwd never lands under the app
   root and the path is dropped (silently, by the root predicate). Absolute paths — what Flask,
   Jinja and Django use — are reported. Worth one line in `docs/local.md`/the design's limits if
   a framework is ever found to open templates relatively.
3. **`os.open()` raises its own `open` event with the string path**, so an `os.open`ed data file
   is reported as well as `open()`; the critical test pins that (it is a feature, not a leak). A
   future reader of the codec should not "fix" the fd case by scoping it away.

## Gate

Both children are `done`: **MB.23.4.1** (the codec + Python reporter, commit `95b0db4`) and
**MB.23.4.2** (the PHP sink, commit `c289f19`).

| Command | Result |
|---|---|
| `cargo test -p blastit-view --locked loaded` | `23 passed` |
| `cargo test -p blastit-php-sapi --locked` | `5 + 8 + 15 + 1 passed (2 ignored php_native)` |
| `cargo test -p blastit --features python --test critical_loaded_files` | `2 passed` |
| `make check` | `175 test targets ok, 0 FAILED`; sole failure the MB.23.5-owned `docs/reference/cli.md` fence 1 (`--no-reload`) |

`git diff -- Cargo.toml Cargo.lock` is empty across the epic, and `critical_guard` is clean for
both children. Mutation evidence is the children's reviewers — split-boot, the 32 KiB cap,
the non-blocking send, the hook ban list, the snapshot call, the absolute filter and the source
merge each turned a named test red and was restored byte-identical — so no separate epic audit
was opened. The PHP 8.3/8.5 legs are structurally covered but unproven by execution on this host:
the milestone gate must run `make php-sapi-e2e` on all three minors before MB.23.5 relies on the
report.
