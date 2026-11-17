---
id: MB.35.4
title: "Extract crates/blastit-lsp (bin blastit-lsp + thin blastit lsp delegation) with the protected critical_lsp suite unchanged"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.35.3]
paths:
  - "crates/blastit-lsp/**"
  - "crates/blastit/src/lsp/**"
  - crates/blastit/src/lib.rs
  - crates/blastit/Cargo.toml
  - Cargo.toml
  - Cargo.lock
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-17T13:17"
---

# Goal

Move the language server into `crates/blastit-lsp` and leave `blastit lsp` as a delegation, with
no behaviour change and no new dependency.

1. **The crate.** `git mv crates/blastit/src/lsp/{mod.rs,hover.rs}` into
   `crates/blastit-lsp/src/{lib.rs,hover.rs}` (adjust the module path: `lib.rs` is the old
   `mod.rs`, `mod hover;` stays), and add:
   - `crates/blastit-lsp/Cargo.toml` — `name = "blastit-lsp"`, workspace version/edition/lints,
     `[[bin]] name = "blastit-lsp" path = "src/main.rs"`, `[lib] name = "blastit_lsp"`; the
     `main.rs` is the current binary's three-line `std::process::exit(blastit_lsp::run() as i32)`
     shape (keep the panic hook and the deny lints from `mod.rs`'s header);
   - dependencies: `blastit-manifest` and `serde_json` **only** (both already in the workspace);
     no `blastit-sys`, `rustix`, `nix`, `libc`, `pyo3`, no `blastit` itself.
2. **The delegation.** `crates/blastit/src/lib.rs` replaces `pub mod lsp;` with
   `pub use blastit_lsp as lsp;` (so `crates/blastit/src/cli.rs:572`'s `crate::lsp::run()` keeps
   compiling untouched) and `crates/blastit/Cargo.toml` adds
   `blastit-lsp = { version = "0.2.0", path = "../blastit-lsp" }`. The workspace `members =
   ["crates/*"]` already covers the new crate; `Cargo.lock` gains the package entry and nothing
   else.
3. **The tests move with the code.** `mod.rs`'s in-file unit tests move unchanged; any test that
   used `crate::...` paths is repointed. Add one new **protected** integration suite
   `crates/blastit-lsp/tests/critical_lsp_stdio.rs` that spawns
   `env!("CARGO_BIN_EXE_blastit-lsp")` and replays the minimum session: `initialize` (the
   capability document), `didOpen` of a `Blastfile` with a `query` typo (a severity-1 diagnostic
   at the exact `line:col`), `shutdown` + `exit` (status `0`), and a cleared environment (no
   `BLASTIT_*` read). It is the standalone binary's own proof; the 1880-line
   `crates/blastit/tests/critical_lsp.rs` stays **byte-identical** and keeps proving the
   delegation.
4. **Nothing else changes.** No message, no sentence, no frame bound, no capability added; the
   full protocol suite must pass against both binaries without editing a single assertion there.

# Context

- **This is a move, not a refactor.** `mod.rs` (1174 lines) + `hover.rs` (254) are already pure
  `std` + `blastit_manifest` + `serde_json` (verify with
  `grep -n 'use ' crates/blastit/src/lsp/mod.rs`). Any behavioural diff is a bug this leaf did
  not ask for; `git diff --find-renames` must show the move.
- **Protected files.** `crates/blastit/tests/critical_lsp.rs` is protected and **must not be
  edited** — it is not in `paths`, deliberately. The new
  `crates/blastit-lsp/tests/critical_lsp_stdio.rs` becomes protected from its first commit and
  `tools/critical_guard.py` will name it if it is later edited.
- **The binary name in Cargo's env var.** With a hyphenated `[[bin]]` the test env var is
  `CARGO_BIN_EXE_blastit-lsp` (a string literal, legal); confirm it in the first test run.
- **The deny lints are load-bearing.** The `#![deny(clippy::print_stdout, unwrap_used,
  expect_used, panic, todo, indexing_slicing)]` header must move to `lib.rs` (or `main.rs` for
  the binary-only lints) so `cargo clippy -D warnings` still enforces it.
- **Do not touch** `.github/**`, `scripts/**`, `editors/**`, `Makefile`, `docs/**`. CI and
  packaging are `MB.35.5`; the guard is `MB.35.6`.
- **Build collisions.** `MB.23.5.2` and other nodes may be compiling; run the acceptance
  commands when the workspace is free, sequentially.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo build -p blastit-lsp --locked
$ cargo test -p blastit-lsp
$ cargo test -p blastit --test critical_lsp
$ cargo clippy -p blastit-lsp -p blastit --all-targets -- -D warnings
$ git diff --find-renames --stat
$ cargo tree -p blastit-lsp --edges normal
```

`cargo tree` shows `blastit-manifest` + `serde_json` and their transitive crates, and **no**
`blastit-sys`, `rustix`, `nix`, `libc` or `pyo3`. `git diff --find-renames` reports
`mod.rs -> lib.rs`/`hover.rs` as renames (or the commit split is explained). Paste:

- the `cargo test -p blastit --test critical_lsp` tail, unchanged and green;
- the new stdio suite's tail and one captured diagnostic payload (`range.start` line/character);
- `git status --short` showing `crates/blastit/src/lsp/` is gone and the new crate is added;
- `cargo tree` output for the two package audits.

# Notes

- **Keep the doc comment's promises.** `mod.rs`'s header enumerates §3/§2/§4/§5/§6/§7; moving
  the file must not drop a line, and the binary's stderr sentences stay the four fixed ones.
- **No `[[bin]]` duplication.** Do not keep a `blastit-lsp` bin target inside `crates/blastit`;
  one binary, one place.
- **`crates/blastit` still builds with the `python` feature off**, and `make check`'s
  `rs-no-python`/`seam-check` must stay green — the new crate only adds a `blastit-manifest`
  edge.
- If `Cargo.lock` needs a version bump beyond the new package entry, stop: that is a
  dependency change this leaf does not need, and the difference belongs in `RESULT.md`'s
  Follow-ups.
