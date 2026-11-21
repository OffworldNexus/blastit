---
id: MB.23.4.1
title: "The one loaded-file codec and the Python reporter: sys.modules closure + open audit hook, boot/preload harvest, per-request delta"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-view/src/loaded.rs"
  - "crates/blastit-view/src/lib.rs"
  - "crates/blastit-view/src/runtime.rs"
  - "crates/blastit-view/src/request.rs"
  - crates/blastit/src/stage1.rs
  - crates/blastit/tests/critical_loaded_files.rs
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-17T11:17"
updated: "2026-11-17T11:53"
---

# Goal

Land as **reviewed work** the half of MB.23.4 that attempt 1 already put on disk: the one bounded, versioned datagram codec and the whole Python loaded-file reporter (`sys.modules` closure + one `open`-only audit hook, boot/preload harvest, per-request delta).

# Context

- The code is **already written and green** in the working tree (base `9e23dc5`, uncommitted, ~1 830 lines over 6 files). Do **not** rewrite it: review it against this node's `# Context`, fix only what the review proves wrong, and paste evidence into `RESULT.md`.
- Files, exactly: `crates/blastit-view/src/loaded.rs` (new: codec v1, `Reporter`, `Sequence`/`Step`), `crates/blastit-view/src/runtime.rs` (`LoadedTracker`, `LoadedReport`, `LoadedReporter`), `crates/blastit-view/src/lib.rs` (`serve_reporting`), `crates/blastit-view/src/request.rs` (`AppContext::loaded`), `crates/blastit/src/stage1.rs` (install before `import_callable`, harvest after `BLASTIT_WARMUP`, dev only), `crates/blastit/tests/critical_loaded_files.rs` (new test).
- Codec: `u8 version (=1)`, `u8 flags` (bit0 `boot`, bit1 `resync`), `u16 route` (diagnostics and `seq` space only), `u32 seq`, `u16 count`, then `count × { u16 len, len bytes }`; paths ≤ 4096 bytes, ≤ 64 paths, ≤ 64 KiB per datagram; non-blocking send, `EAGAIN`/`EMSGSIZE` drops the batch and arms `resync`; reporting never blocks a request.
- Python: `__spec__.origin` order (built-in → frozen → no spec → namespace dirs → `loader.archive` → `.pyc`/`.pyo` → `realpath` + `isfile`), never `__file__`, memoised by `id(mod)`; the hook is `event == "open"` only, `str` paths only, `try/except BaseException` behind a `threading.local()` busy flag, no `abspath`/`realpath`/`stat`/I/O/import inside it; flush maps `.pyc → .py` and drops `__pycache__`/`.git`/`node_modules`/stdlib.
- The `preload` phase is covered by inheritance (modules imported before the fork are in every worker's closure, so the boot harvest reports them) — no parent-side reporter. `MB.23.5` owns `--no-reload` and the zygote wiring; this child's gate is `dev.is_some()`.
- No user-facing doc and no design doc work: `MB.23.1`/`MB.23.2` shipped both.

# Acceptance

```console
$ cargo test -p blastit-view --locked loaded
$ cargo test -p blastit-view --locked --doc
$ cargo test -p blastit --locked --features python --test critical_loaded_files
$ cargo clippy -p blastit-view --all-targets --locked -- -D warnings
$ make check
$ git diff --stat Cargo.toml Cargo.lock   # no output
$ uv run --no-project tools/critical_guard.py --base 9e23dc5 --node MB.23.4.1
```

1. Codec refusal matrix: round-trips every field, refuses a truncated batch, a `version != 1`, a `count` above the cap, a `len` above 4096 and a batch above 64 KiB, and proves `seq` gaps and `resync` are reported to the caller as such (not silently dropped).
2. Python in-process: imports a module and opens a template, harvests, and asserts the module's `.py` path and the template path are reported; a module listed in `preload` is in the boot harvest (the inherited closure); `__pycache__/*.pyc` became its `.py`; a built-in (`sys`) and a frozen (`os`) module reported nothing; an fd-based `open(3, "w")` did not crash the hook; an exception inside the hook never aborts the app's real `open` (the file is created).
3. Warm path: a second harvest over unchanged `sys.modules` reports the same set and stays inside the measured budget (assert the call count, not a timer).
4. `make check` green except the one known pre-existing `docs/reference/cli.md` fence 1 failure (`--no-reload`, `MB.23.5`'s), and `Cargo.toml`/`Cargo.lock` byte-identical.

# Notes

- `MB.23.5` calls `serve_reporting(..., Some(LoadedReport { .. }))`; do not add a `ViewConfig` field here — that file is `MB.23.5`'s.
- Do not touch `crates/blastit-view/src/{config,php}.rs`, `crates/blastit/src/php_view.rs`, `crates/blastit-php-sapi/**`, `Makefile` or `e2e/php-sapi/Dockerfile`: `MB.23.4.2` owns them.
- If a shared symbol turns out to be needed in `crates/blastit-sys`, stop and file a follow-up rather than editing it.
