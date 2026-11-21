---
id: MB.35
title: "Separate cross-platform blastit-lsp binary for Linux, macOS and Windows while blastit stays Linux-only"
kind: epic
status: done
priority: 1
depends_on: [MB.32]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-18T11:58"
---

# Goal

Publish a **portable `blastit-lsp`** binary for Linux, macOS and Windows, while the `blastit`
binary stays Linux-only. A developer on a Mac or on Windows gets the full `Blastfile` editor
experience — colours, SQL diagnostics with the exact `line:col` from `MB.34`, Quick Fixes,
hover, format-on-save — because the language server is its own small, cross-platform artifact
that never links a privilege, a Python runtime, seccomp, or a Linux-only syscall. Linux keeps
`blastit lsp` working, unchanged, as a one-line delegation to the same code.

Owner mandate (`0.2.0`, 2026-11-17):

> *"it'd be nice if there were osx and windows releases for the plugin and blastit binary (even
> if they ONLY embed the LSP server and not the actual blastit mechanics, which only work in
> Linux...). Or maybe the LSP server is another different blastit-lsp binary which gets
> published on all platforms while the main blastit binary is just available for Linux"*

This is a **reversal of a recorded decision**, and it must be recorded as one. `ADR-0013` §"the
separate `blastit-lsp` binary or crate" rejects a second artifact, and
`docs/design/blastfile-lsp-zed.md:137` has the comparison table; both were written when the
editor only ran on the Linux machines Blastit runs on. The owner's mandate changes the premise,
not the reasoning: the answer is the second option in their message, and it is chosen because
the LSP's dependency surface is already portable and the main binary's is not.

The chain, in DX-first order:

1. **Research** (`MB.35.1`) — can `pg_query`/`libpg_query` build and run on `windows-latest`
  (MSVC, `LIBCLANG_PATH`) and on both Apple targets, and what does Zed's extension API do with
  `Zip` on Windows, `current_platform()` and `which()` there? Facts with citations, or the
  epic's premise changes before a line of code is written.
2. **User docs** (`MB.35.2`) — install and run `blastit-lsp` on Linux, macOS and Windows; what
  the binary can and cannot do (it edits and diagnoses `Blastfile`s, it never serves an app);
  how the editor finds it; why `blastit` itself stays Linux.
3. **Design** (`MB.35.3`) — the crate boundary and its portability invariant, the resolved
  reversals in `ADR-0013`/design §8.3, and the packaging matrix as a table of record.
4. **Extract the crate** (`MB.35.4`) — `crates/blastit-lsp` (`bin blastit-lsp`, lib
  `blastit_lsp::run()`), depending only on `blastit-manifest` and `serde_json`; `blastit lsp`
  becomes a delegation, with the 1880-line protected `critical_lsp.rs` passing **unchanged**.
5. **CI + GitHub Release packaging** (`MB.35.5`) — build and smoke each target, package
  `blastit-lsp-<target>.tar.gz` / `.zip`, reusing `scripts/platform_matrix.py` as the one table
  so `make workflow-check packaging-gates-test` stay honest.
6. **Portability guard + end-to-end proof** (`MB.35.6`) — a `make check` gate that fails if the
  crate regains a Linux-only or Python dependency, the dependency tree as evidence, and the
  documented commands replayed.

`depends_on: [MB.32]` — the chain is `MB.31` → `MB.34` → `MB.32` → this epic → `MB.33`, so the
site's own highlighter inventory is settled before this epic's docs add a block to it; `MB.33`
(the Zed extension's own resolution code) depends on this epic.

# Context

- **What is already portable.** `crates/blastit/src/lsp/mod.rs` (1174 lines) + `hover.rs` (254)
  import `std`, `blastit_manifest` and `serde_json` only; no `std::os`, no `libc`, no `pyo3`,
  no `blastit-sys`. The forbidden surface today comes from the binary that hosts it:
  `crates/blastit` links `blastit-sys`, `rustix`, seccomp and the PHP SAPI (`Cargo.toml`), and
  the crate's release story is the Linux matrix (`scripts/platform_matrix.py`, `ADR-0010` §4).
- **Why the LSP still needs `pg_query`.** `blastit-manifest` → `blastit-sqlpolicy` →
  `pg_query 6.2.1` (libpg_query built from C). That is the one real cross-platform risk and the
  reason `MB.35.1` exists; if Windows cannot build it, the honest answer is a documented
  unsupported target, not a shim that validates nothing.
- **The protected proof of equivalence.** `crates/blastit/tests/critical_lsp.rs` spawns
  `env!("CARGO_BIN_EXE_blastit") lsp` and covers framing, UTF-16, diagnostics, formatting and
  hover. It must pass **byte-for-byte unchanged** after the extraction — that is what proves
  `blastit lsp` and `blastit-lsp` are one implementation, not two.
- **Release plumbing.** `.github/workflows/publish-pypi.yml` builds the standalone archives
  (`binaries_gnu`, `binaries_musl`) and its `release` job uploads `dist/*` and then counts
  `standalone-*` archives — a new asset family must be added to that count and to
  `scripts/platform_matrix.py`'s declared/proved claims, or the release job fails on an honest
  new artifact.
- **Zed consumes it in `MB.33`.** That leaf resolves `LspSettings` → `which("blastit-lsp")` →
  `which("blastit") lsp` → a GitHub Release download by target (`GzipTar` on Linux/macOS,
  `Zip` on Windows). This epic's asset names and layouts are the contract it codes against;
  `editors/zed/src/lib.rs` is **not** touched here.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-lsp
$ cargo test -p blastit --test critical_lsp
$ make check
$ make docs-check
$ make workflow-check platform-matrix-check packaging-gates-test
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
```

plus, in the epic `RESULT.md`:

- `cargo tree -p blastit-lsp --edges normal` showing no `blastit-sys`, `rustix`, `nix`, `pyo3`
  or `libc` runtime dependency (the `pg_query`/`cc` build deps are named, not hidden);
- the five artifacts, by exact name, and the CI job/leg that produced each:
  `blastit-lsp-x86_64-unknown-linux-gnu.tar.gz`,
  `blastit-lsp-aarch64-unknown-linux-gnu.tar.gz`,
  `blastit-lsp-aarch64-apple-darwin.tar.gz`,
  `blastit-lsp-x86_64-apple-darwin.tar.gz`,
  `blastit-lsp-x86_64-pc-windows-msvc.zip`;
- one smoke transcript per platform (or `NOT RUN` with the exact CI failure) of
  `initialize` → `didOpen` of a broken `query` → the diagnostic's exact range → `shutdown`;
- the `MB.35.1` citations for `pg_query` on every target, and the `LIBCLANG_PATH` value on
  `windows-latest`.

# Notes

- **The shipped `blastit` binary is not reduced.** Linux keeps every subcommand; only the
  editor-facing name gains a portable twin. `blastit lsp` must remain documented and tested as
  an alias, because existing pages and the existing critical suite use it.
- **No second protocol.** Do not fork `lsp/mod.rs` "for portability": the extraction moves the
  file; any behavioural change is a separate bug, and the protected suite will catch it.
- **Windows path facts belong in the guard, not in a comment.** `blastit-lsp.exe`, `.zip`,
  no `chmod`, `\r\n` on stdio is byte-length-framed and therefore irrelevant, `PATH` lookup of
  `.exe` by `which`.
- **This epic does not ship the Zed extension.** `MB.33` does, and it is the only consumer that
  downloads these assets; a release without `MB.33` still leaves a usable `blastit-lsp` on
  `PATH`.
