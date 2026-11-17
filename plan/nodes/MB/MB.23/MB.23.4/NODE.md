---
id: MB.23.4
title: "Report the preload phase's and each dev view's loaded files: Python sys.modules + open audit hook, PHP EG(included_files) via a dlsym sink, one bounded datagram codec, no route map"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.23.2]
paths:
  - "crates/blastit-view/src/loaded.rs"
  - "crates/blastit-view/src/lib.rs"
  - "crates/blastit-view/src/runtime.rs"
  - "crates/blastit-view/src/config.rs"
  - "crates/blastit-view/src/request.rs"
  - "crates/blastit-view/src/php.rs"
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/php_view.rs
  - "crates/blastit-php-sapi/c/blastit_php_sapi.c"
  - "crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h"
  - "crates/blastit-php-sapi/src/lib.rs"
  - "crates/blastit-php-sapi/src/request.rs"
  - "crates/blastit-php-sapi/src/abi.rs"
  - "crates/blastit-php-sapi/src/loader.rs"
  - "crates/blastit-php-sapi/tests/critical_file_sink.rs"
  - crates/blastit/tests/critical_loaded_files.rs
  - Makefile
  - "e2e/php-sapi/Dockerfile"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T23:12"
updated: "2026-11-17T09:17"
---

# Goal

Make the `preload` phase and **every dev view worker** report exactly the files each one
actually loaded, over the dev-only socket, for both runtimes:

- **Python:** `sys.modules` closure (`__spec__.origin`, never `__file__`) plus one
  `open`-only `sys.addaudithook`, harvested after `BLASTIT_WARMUP` (so the preloaded
  closure is reported once) and then as a per-request delta.
- **PHP:** `EG(included_files)` snapshotted between `php_execute_script` and
  `php_request_shutdown`, reported through a `dlsym`-resolved optional sink — no ABI
  version bump, no allocation owned across the boundary.

Both feed one bounded, versioned datagram codec, so the supervisor can **union** them into
the exact watched file set. There is no per-route association: the worker id in the codec
exists for logging and per-view `seq`, never for choosing which views to restart.

# Context

- Follow §2.1–§2.3 of `docs/research/2026-11-16-dev-fanotify-reload.md` exactly.
- **Python modules:** resolve `origin` in the measured order — built-in → frozen (**never**
  fall back to `__file__`: `os.__file__` is real) → `spec is None` → namespace package
  (report the search directories) → `loader.archive` (report the archive) →
  `.pyc`/`.pyo` (report it) → `realpath(origin)` + `isfile`. Memoise `id(mod)` and
  `origin → realpath`; the research measured 21 ms cold and 33 µs warm, which is the
  budget.
- **Python hook:** `event == "open"` only; drop `path` unless `isinstance(path, str)`;
  `mode` may be `None`; predicate is `startswith(root_with_trailing_separator)` +
  `endswith(suffixes)` (no `abspath`, no `realpath`, no `stat`, no I/O, no import inside
  the hook); wrap the body in `try/except BaseException` with a `threading.local()` busy
  flag; install **once**, in the dev worker, before `import_callable`, and only when
  reload is enabled. Flush maps `.pyc → .py` with `importlib.util.source_from_cache`
  (catching `ValueError`), drops `__pycache__`/`.git`/`node_modules` and the stdlib path,
  and keeps site-packages and editable installs.
- **PHP:** add `blastit_php_file_sink_install` — `int32_t (*included)(void *ctx, const
  char *path, size_t len)` — resolved with `dlsym`, absent → no-op; iterate
  `ZEND_HASH_FOREACH_STR_KEY` (values are `IS_NULL`), copy bytes, filter to absolute paths
  in Rust (`opened_path` can be `NULL`), canonicalise, dedupe, sort. Snapshot before
  `php_request_shutdown` so bailouts and `exit()` still report. Build the compiled→source
  map from the `/**PATH … ENDPATH**/` / Twig `ModuleNode` paths the shim already reports.
  Do **not** replace PHP's plain file wrapper. Must hold on 8.3, 8.4 and 8.5.
- **Codec (shared, versioned, bounded):** `u8 version (=1)`, `u8 flags` (bit0 `boot`,
  bit1 `resync`), `u16 route` (the reporting view's id; diagnostics and `seq` space only),
  `u32 seq`, `u16 count`, then `count × { u16 len, len bytes }`. `len` excludes the NUL;
  paths are capped at 4096 bytes; a batch is capped at 64 paths and 64 KiB; `seq` is
  monotonic per view so a gap rebuilds that view's contribution; `boot` replaces it;
  `resync` means "treat every path this view ever reported as changed", which can only
  lead to one conservative all-view reload. Send with `MSG_DONTWAIT`; on
  `EAGAIN`/`EMSGSIZE` drop the batch and set `resync` — never block a request on
  reporting.
- **Channel:** one `AF_UNIX SOCK_DGRAM` socketpair per view, the worker end added to the
  child's keep list so `close_fds_except` leaves it alone, the fd reaching Python through
  the loader's `py.run` global scope exactly like `--dev-obs-fd`. **No environment
  variable** (stage 0 strips every `BLASTIT_*` from a view). Opened only when reload is
  enabled.
- **The `preload` phase is covered by inheritance, not a second socket:** modules imported
  before the fork are in every worker's `sys.modules` closure, so the boot harvest reports
  them; the supervisor's union is therefore exactly "preload + every worker". Do not add a
  separate parent-side reporter.
- Every counter is bounded and every failure is silent-by-construction on the worker side
  (a reporting error must never fail a request). A view that reports more than the cap
  keeps the newest paths and sets `resync`, which is conservative.

# Acceptance

```console
$ cargo test -p blastit-view --locked loaded
$ cargo test -p blastit-view --locked --doc
$ cargo test -p blastit-php-sapi --locked
$ cargo test -p blastit --locked --features python --test critical_loaded_files
$ cargo clippy -p blastit-view -p blastit-php-sapi --all-targets --locked -- -D warnings
$ make check
$ git diff --stat Cargo.toml Cargo.lock   # no output
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.23.4
```

1. A unit test round-trips every codec field, refuses a truncated batch, a `version != 1`, a
   `count` above the cap, a `len` above 4096 and a batch above 64 KiB, and proves `seq`
   gaps and `resync` are reported to the caller as such (not silently dropped).
2. Python: an in-process test (the `python` feature, same harness the other interpreter
   tests use) imports a module and opens a template, harvests, and asserts the module's
   `.py` path and the template path are reported, that a module listed in `preload` is in
   the boot harvest (the inherited closure), that `__pycache__/*.pyc` became its `.py`,
   that a built-in (`sys`) and a frozen (`os`) module reported nothing, that an fd-based
   `open(3, "w")` did not crash the hook, and that **an exception raised inside the hook
   never aborts the app's real `open`** (the file is created).
3. The warm path is asserted: a second harvest over unchanged `sys.modules` reports the
   same set and stays inside the measured budget (assert the call count, not a timer).
4. PHP: a shim-contract test (new file `critical_file_sink.rs`; the existing protected shim
   test is not edited) installs a recording sink, runs a script that includes a file and
   reads a `.env`-style data file, and asserts the included file is reported once,
   absolute, sorted, and that a bailout request still reports; the data file is **not**
   reported by the sink (the compiled→source map is the only path for templates).
5. `make check` is green and the root `Cargo.toml`/`Cargo.lock` are byte-identical.

# Notes

- `MB.23.3` and this task run in parallel and share no file. If a shared symbol turns out
  to be needed in `crates/blastit-sys`, stop and file a follow-up rather than editing it.
- Keep the PHP sink optional forever: PHP builds without the symbol must behave exactly as
  today.
- The hook and the sink are dev-only code paths. A production build must not even install
  the hook; `MB.23.5` owns the switch and `MB.23.6` audits it.
- The report is a **set of files per view**, never a route map: do not add a payload field
  that names a route to reload, and do not assume the consumer knows which views share a
  module. The consumer unions; this task only reports the truth.
