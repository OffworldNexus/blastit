---
id: MB.35.6
title: "Enforce blastit-lsp portability in make check and prove the documented commands end to end"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.35.4, MB.35.5]
paths:
  - scripts/check_lsp_portable.py
  - scripts/test_check_lsp_portable.py
  - Makefile
  - "docs/testing/E2E-MB.35.md"
attempts: 4
max_attempts: 5
escalation: expert
created: "2026-11-17T13:17"
updated: "2026-11-18T09:18"
---

# Goal

Make the portability promise **enforced** rather than remembered, then prove the documented
commands end to end.

1. **`scripts/check_lsp_portable.py`** (stdlib, `uv run --no-project`), run by a new
   `make lsp-portable` target wired into `make check`:
   - `crates/blastit-lsp/Cargo.toml`'s `[dependencies]` is exactly
     `{ blastit-manifest, serde_json }` — any other runtime dependency is red, and the message
     names the crate and the reason class (`blastit-sys`/`rustix`/`nix`/`libc`/`pyo3`/
     `blastit-php-sapi` are the named forbidden set, not an allowlist that hides the next one);
   - no `crates/blastit-lsp/src/**` line contains `std::os::unix`, `std::os::linux`,
     `std::os::windows`, `libc::`, `rustix`, `nix::`, `pyo3`, `#[cfg(unix)]`, `#[cfg(target_os`
     or `std::process::Command` (the LSP executes nothing);
   - no `std::env` read (the same rule `check_manifest_purity.py` enforces for the manifest:
     the server reads the client's messages, never the environment);
   - `bisect` the crate's dependency tree: `cargo metadata` (offline) shows no forbidden
     package in `blastit-lsp`'s normal dependency closure, and the failure prints the path that
     brought it in.
2. **`scripts/test_check_lsp_portable.py`** — one mutation per property on a throwaway copy
   (add `rustix` → red; add `use std::os::unix::fs` → red; add a `Command::new` → red; delete a
   dependency the code needs → the compile is red, not this guard; remove the target from
   `make check` → the wiring test is red).
3. **The proof.** Replay what `MB.35.2` documented, verbatim: on Linux, `blastit lsp` and
   `blastit-lsp` answer the same session identically (`diff` of the two transcripts modulo PIDs
   and server version), the portable archive from `MB.35.5` runs from a clean directory with no
   `PATH` entry, and the page's commands are executed (not just read).
4. **`docs/testing/E2E-MB.35.md`** — the transcripts, the `cargo tree` closure, the guard's
   output, and the CI run(s) that proved macOS/Windows (URL + job names), or `NOT RUN` with the
   exact blocker. `VERDICT: PASS` only when every documented platform has evidence.

# Context

- **Why a guard and not a comment.** The portability invariant is the only reason the portable
  artifact exists; the next `use` that reaches for `rustix` (for a capability drop, say) would
  compile on Linux and break two releases silently. `make check` already runs six such guards
  (`critical-guard`, `seam-check`, `manifest-purity`, `zed-check`, `workflow-check`,
  `platform-matrix-check`) — this is the seventh and follows their shape (stdlib Python, a
  `--check` mode, a negative-twin test file).
- **`std::process::Command` matters twice.** The LSP is forbidden from spawning anything
  (`docs/design/blastfile-lsp-zed.md` §11.3, the no-child assertion in `critical_lsp.rs`); the
  same grep keeps a future "run `blastit check` for the full list" shortcut from appearing.
- **`cargo metadata` must be offline-safe.** Use `cargo metadata --offline --locked` on the
  workspace manifest; if the closure walk cannot read the graph, the guard fails closed (the
  `MB.3.64` lesson) instead of fetching or skipping.
- **The runnable-docs item is not a repeat of `MB.35.2`.** That leaf wrote the pages; this leaf
  *executes* them and records the output. If a documented command is wrong, the fix is on the
  page (list `docs/**` here) and the transcript shows both.
- **Do not touch** `crates/blastit-lsp/src/**` (report instead), `.github/**`
  (unless a count/claim in a workflow is wrong — then say why), `editors/**`.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project scripts/check_lsp_portable.py
$ UV_OFFLINE=1 PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project \
    scripts/test_check_lsp_portable.py
$ make lsp-portable
$ cargo tree -p blastit-lsp --edges normal
$ make check
$ make doc-tone docs-check docs-test
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
```

`make check` must list `lsp-portable` in its prerequisites (paste the line) and the guard must
fail if the target is removed from that list. `docs/testing/E2E-MB.35.md` ends with
`VERDICT: PASS`, and every platform in `MB.35.5`'s matrix has either a CI URL or `NOT RUN` with
the reason.

# Notes

- **Fail closed.** A manifest the guard cannot parse, a crate missing from `cargo metadata`, or
  an unreadable source file is a red guard, never a skip (the `MB.3.67` lesson: a check must not
  claim a verdict it cannot support).
- **No `--force`, no `--allow` flag.** The forbidden set is the rule; an escape hatch is the
  next regression's home.
- Keep the guard under ~200 lines and the test file's cases named after the property they
  mutate, so a failure message says which promise broke.
- The `diff` of the two Linux transcripts is the equivalence evidence `MB.35.3` promised; keep
  it in the E2E file, not in a unit test (it depends on two built binaries).
