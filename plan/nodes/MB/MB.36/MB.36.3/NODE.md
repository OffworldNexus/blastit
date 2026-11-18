---
id: MB.36.3
title: "Extract crates/blastit-view-python: environ, input, request, response and the serve worker pool out of blastit-view, wired through blastit's python feature"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.36.2]
paths:
  - "crates/blastit-view-python/**"
  - "crates/blastit-view/src/**"
  - "crates/blastit-view/tests/**"
  - "crates/blastit-view/Cargo.toml"
  - crates/blastit/src/zygote.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/supervise.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/lib.rs
  - crates/blastit/tests/critical_loaded_files.rs
  - crates/blastit/Cargo.toml
  - Cargo.lock
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-17T14:07"
updated: "2026-11-18T13:55"
---

# Goal

Create `crates/blastit-view-python` and move the WSGI worker into it: the PEP 3333
`environ`, `wsgi.input`, `request`/`response`, the lazily-grown worker pool and the `serve`
I/O loop. `crates/blastit-view` keeps only what every view speaks; `crates/blastit` wires the
crate through its `python` feature.

**Behaviour-preserving move.** Same frames, same credits, same `DONE`/`RESP_END_COST`.

# Context

- Design of record: `MB.36.1`'s `view-crates-architecture.md` (graph, seam decision 2, feature
  decision 6) and `ADR-0016`.
- Files moving out of `crates/blastit-view/src/`: `environ.rs` (570), `input.rs` (568),
  `request.rs` (487), `response.rs` (536), and the `serve` half of `lib.rs` (the 1234-line
  crate root whose doc comment is the §2.5/§3.1/§9.3 contract). The `serve` tests in
  `crates/blastit-view/tests/{serve,environ,body,flow,concurrency,gil}.rs` move beside them —
  `gil.rs` is the one test that owns the interpreter (`pyo3` `auto-initialize`).
- What **stays** in `blastit-view` for now: `wire.rs`, `flow.rs`, `loaded.rs`'s codec,
  `config.rs`, `error.rs`, `sync.rs`, and — until `MB.36.5` deletes the trait —
  `runtime.rs` including `OutputStream`/`RequestIo`/`RequestEnd`/`OUTPUT_LINE_CAP`. The Python
  `LoadedTracker`/`LoadedReporter` do **not** stay (they install a Python audit hook): they and
  `crates/blastit/tests/critical_loaded_files.rs` move into `blastit-view-python` **in this
  leaf**, which is what keeps its acceptance green. Say so in the crate docs: this leaf moves
  the *interpreter*, not the *wire*.
- `pyo3` rules: `blastit-view-python` owns `pyo3` **without** `auto-initialize` in
  `[dependencies]` (a view is forked from an interpreter `init` already initialised) and
  **with** `auto-initialize` in `[dev-dependencies]` (the tests attach themselves); keep
  `blastit-view`'s `python` feature and its default off the table — `default = ["python"]`
  must be deleted here, because Cargo unifies features and a default is how `libpython` walks
  back into the standalone archive (`docs/knowledge/LESSONS.md`).
- Call sites to update: `crates/blastit/src/stage1.rs` (the `serve` call and `LoadedTracker`
  install), `supervise.rs`, `cli.rs`, `lib.rs` (`#[cfg(feature = "python")]` arms already
  exist), and `crates/blastit/Cargo.toml`'s `python = [...]` list. `blastit-py` needs no
  change: it turns `python` on for `blastit`.
- Do **not** delete the `WorkerRuntime` trait here: `php.rs` still implements it. `MB.36.5`
  deletes it once no sibling uses it.

# Acceptance

```console
$ cargo test -p blastit-view-python --locked
$ cargo test -p blastit-view --locked
$ cargo test -p blastit --lib --locked
$ cargo test -p blastit-view-python --test critical_loaded_files --locked
$ make critical-guard NODE=MB.36.3
$ test -d crates/blastit-view-python && echo "crate exists"
$ test -f crates/blastit-view-python/src/serve.rs && echo "serve moved"
$ test ! -e crates/blastit-view/src/environ.rs && test ! -e crates/blastit-view/src/input.rs && echo "no WSGI left behind"
$ grep -q 'blastit-view-python' crates/blastit/Cargo.toml && echo "feature wired"
$ test -z "$(grep -n 'default' crates/blastit-view/Cargo.toml | grep -i python)" && echo "python is no longer a default feature"
$ make check
```
- Every moved test keeps its assertions; `crates/blastit-view/tests/` never loses a file to
  deletion (moves are visible as renames in `git diff --stat -M`).
- `cargo test -p blastit --lib` proves the standalone (`php`, no `python`) build still
  compiles with the new crate absent.
- `RESULT.md` pastes the `make check` tail plus the rename summary.

# Notes

- The `blastit-view` crate root's long doc comment is the architecture contract: split it, do
  not delete it — the `serve` half belongs on `serve.rs`, the framing/credit half stays where
  the code is.
- `Cargo.lock` is touched here (new member). Leaves run strictly one at a time; no parallel
  Cargo edit.
- If a moved module still needs a `blastit-view` item, import it explicitly; do not
  `pub use` it back through `blastit-view` "for compatibility".
