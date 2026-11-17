---
id: MB.23.4.2
title: "The PHP loaded-file sink: EG(included_files) between execute and request_shutdown, optional dlsym sink with no ABI bump"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.23.4.1]
paths:
  - "crates/blastit-php-sapi/c/blastit_php_sapi.c"
  - "crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h"
  - "crates/blastit-php-sapi/src/abi.rs"
  - "crates/blastit-php-sapi/src/loader.rs"
  - "crates/blastit-php-sapi/src/lib.rs"
  - "crates/blastit-php-sapi/src/request.rs"
  - "crates/blastit-view/src/php.rs"
  - crates/blastit/src/php_view.rs
  - "crates/blastit-php-sapi/tests/critical_file_sink.rs"
  - Makefile
  - "e2e/php-sapi/Dockerfile"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T11:17"
updated: "2026-11-17T11:17"
---

# Goal

Report PHP's `EG(included_files)` for every dev PHP view through a new optional `dlsym`-resolved sink feeding the shared `blastit_view::loaded` codec as it stands — no ABI version bump, no allocation owned across the boundary.

# Context

- `MB.23.4.1` already shipped the codec (`crates/blastit-view/src/loaded.rs`) and the Python leg; this child adds only the PHP half and reuses `Batch`/`Reporter` unchanged.
- The sink is a **new exported function**, not a struct member: `int32_t blastit_php_file_sink_install(void *ctx, int32_t (*included)(void *, const char *, size_t))`. Resolve it with `dlsym` through the **shim handle** the worker already opened in `loader.rs` (the same wrapper, never `RTLD_DEFAULT` — the shim is `RTLD_LOCAL`). The symbol absent is a no-op: PHP builds without it behave exactly as today.
- `blastit_php_sink_v1`, its `_Static_assert`s and `BLASTIT_PHP_ABI_VERSION` are untouched — no ABI bump. The fd lives in the Rust `ctx` (a `Sized` cell holding the fat pointer, handed to a closure, never returned). `len == 0` and a `NULL` path never form a slice.
- Iterate `ZEND_HASH_FOREACH_STR_KEY` (values are `IS_NULL`), copy bytes, filter to absolute paths in Rust (`opened_path` can be `NULL`), canonicalise, dedupe, sort. Snapshot **between** `php_execute_script` and `php_request_shutdown` so bailouts and `exit()` still report. Do **not** replace PHP's plain file wrapper; `php_resolve_path` and `open_basedir` stay. Must hold on PHP 8.3, 8.4 and 8.5. Build the compiled→source map from the `/**PATH … ENDPATH**/` / Twig `ModuleNode` paths the shim already reports.
- Bookkeeping: `Makefile`'s `PHP_SUITE_ARGS` gains the sink suite, and the `e2e/php-sapi/Dockerfile` `target/php-suite-executables` `$# -ne 2` check becomes `3`.
- No user-facing doc and no design doc work: `MB.23.1`/`MB.23.2` shipped both.

# Acceptance

```console
$ cargo test -p blastit-php-sapi --locked
$ cargo test -p blastit-view --locked loaded
$ cargo test -p blastit --locked --features python --test critical_loaded_files
$ cargo clippy -p blastit-view -p blastit-php-sapi --all-targets --locked -- -D warnings
$ make check
$ git diff --stat Cargo.toml Cargo.lock   # no output
$ uv run --no-project tools/critical_guard.py --base 9e23dc5 --node MB.23.4.2
```

1. PHP: a shim-contract test (new file `crates/blastit-php-sapi/tests/critical_file_sink.rs`; the existing protected shim test is not edited) installs a recording sink, runs a script that includes a file and reads a `.env`-style data file, and asserts the included file is reported once, absolute, sorted, and that a bailout request still reports; the data file is **not** reported by the sink (the compiled→source map is the only path for templates).
2. The symbol stays optional: a shim without it is a silent no-op, and no ABI version changed.
3. `make check` green except the one known pre-existing `docs/reference/cli.md` fence 1 failure (`--no-reload`, `MB.23.5`'s), and `Cargo.toml`/`Cargo.lock` byte-identical.

# Notes

- Do **not** edit `crates/blastit/tests/critical_loaded_files.rs`: it is protected now; the PHP assertions belong in `critical_file_sink.rs`.
- Do not touch `crates/blastit-view/src/{loaded,runtime,request}.rs`, `crates/blastit/src/stage1.rs` or the Python leg: `MB.23.4.1` owns them.
- An empty `EG(included_files)` is an empty snapshot, not an error; the reporter stays silent on any failure so a reporting bug can never fail a request.
