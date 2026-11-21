# MB.35.1 — research result

**Deliverable:** `docs/research/2026-11-17-cross-platform-blastit-lsp.md` (+ `docs/research/INDEX.md`
entry). No production code, no design docs, no plan status changes.

**Verdict: `GO` — all five targets build; no target needs dropping.** Conditions are in the memo's
final section, so `MB.35.3` cannot read the GO as "nothing left to prove".

## What the four questions resolved to

1. **`pg_query 6.2.1` per target.**
   - `x86_64-pc-windows-msvc`: **proven** by `pg_query.rs`'s own CI at the `6.2.1` tag (run
     `36765631562`, all `windows-latest` jobs `success`, `cargo build` + `cargo test`). Build needs
     `cl.exe` and `libclang.dll`; no env var is required, but pin
     `LIBCLANG_PATH=C:\Program Files\LLVM\bin` (the image ships LLVM 20.1.8 at
     `C:\Program Files\LLVM`; clang-sys 1.9.1 has no registry fallback).
   - `aarch64-apple-darwin` on `macos-latest` (arm64) and `x86_64-apple-darwin` on `macos-15-intel`
     (GA, standard, free on public repos): **buildable** from source + runner docs, **unproven for
     the Rust wrapper** — upstream has zero macOS CI. No `LIBCLANG_PATH` needed on macOS.
   - Linux legs cited from `scripts/platform_matrix.py`: `x86_64-unknown-linux-gnu` **proved**
     (`MB.3.18`), `aarch64-unknown-linux-gnu` **declared, unbuilt**.
   - Boundary: any future drop goes through `dropped_because` **and** an extension message naming the
     missing target — never a stub validator.
2. **Zed `zed_extension_api 0.7.0` (binds `wit/since_v0.6.0`).** `current_platform()` = host
   `env::consts::OS`/`ARCH`, so an Intel Mac is `Os::Mac` + `X8664`; `Zip` **extracts** into the path
   passed (no archive kept, entries verbatim); `make_file_executable` is a no-op `Ok(())` off Unix;
   `which("blastit-lsp")` resolves an on-`PATH` `.exe` via `PATHEXT`, while a **downloaded** binary is
   launched only through the relative `Command.command` that `path_from_extension` joins.
3. **Release assets.** `gh release create dist/*` uploads raw bytes with an exact `Content-Length`
   (`publish-pypi.yml:807-841`). The browser URL 302s to **`release-assets.githubusercontent.com`**,
   not `objects.githubusercontent.com` (measured with `curl` here). Zed's `download_file` gate is a
   **user setting** (`extension.granted_extension_capabilities`, default `host = "*"`) checked on the
   pre-redirect URL only — so `MB.33` declares **no** capability.
4. **Dependencies.** Both `cargo tree` commands pasted in the memo; the `lsp` module imports only
   `std`, `blastit-manifest`, `blastit-sqlpolicy` and `serde_json`, and `blastit-manifest`'s normal
   closure has no `libc`/`rustix`/`nix`/`pyo3`/`blastit-sys`. Unix-only crates named: `blastit-sys`,
   `blastit-dbproxy`, `blastit-scheduler`, `blastit`, `blastit-py`, `blastit-view`,
   `blastit-php-sapi` (+ two `#[cfg(unix)]` blocks in `blastit-http`, not in the LSP path).

## Acceptance evidence

```console
$ uv run --no-project tools/plan.py validate
plan OK
```

```console
$ cargo tree -p blastit --edges normal | grep -E 'blastit-(sys|sqlpolicy|manifest)|rustix|libc|pyo3'
▸ 22 matching lines (the `blastit` binary's Linux surface: `blastit-sys` → `libc` + `rustix`, …)
$ cargo tree -p blastit-manifest --edges normal | grep -cE 'libc|rustix|nix|pyo3'
0        # exit 1: grep found no match, which is the assertion
$ cargo tree -p blastit --edges normal -i blastit-manifest
blastit-manifest v0.2.0 (…/crates/blastit-manifest)
├── blastit v0.2.0 (…/crates/blastit)
├── blastit-env v0.2.0 (…/crates/blastit-env)
│   └── blastit v0.2.0 (…/crates/blastit)
└── blastit-scheduler v0.2.0 (…/crates/blastit-scheduler)
    └── blastit v0.2.0 (…/crates/blastit)
```

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
```

## Follow-ups (out of this node's paths)

- `MB.33`'s brief (in `plan/nodes/MB/MB.33/NODE.md`) tells the implementer to declare
  `objects.githubusercontent.com` as a download capability. Both halves are wrong: the host is
  `release-assets.githubusercontent.com`, and the manifest is not consulted for `download_file` at all.
- `scripts/platform_matrix.py` has no notion of the five `blastit-lsp` artifacts yet; `MB.35.5` will
  need a `lsp` family (with the `aarch64` Linux leg marked declared-but-unbuilt).
- The memo is ~350 lines, over the node's "~200" guidance: four questions, two pasted `cargo tree`
  outputs and a five-row per-target citation table do not compress further without dropping the
  citations the same node demands. Flagged rather than silently trimmed.

## Node status

Not touched — `researcher` subagents do not mutate plan statuses.

## Post-review MINORs (reviewer: PASS, eight fixes)

Applied, in `docs/research/2026-11-17-cross-platform-blastit-lsp.md` only:

1. Verdict reworded to `GO — five targets planned; Windows and Linux x86_64 proven, both Apple legs
   unproven until MB.35.5`; the TL;DR's Apple bullet now says "buildable … unproven for the Rust
   wrapper", so no line still claims all five *build*.
2. `which`-8 citation replaced with `which-8.0.0/src/finder.rs:193-238` (the `# Append the extension`
   loop in `PathsIter`) and `src/sys.rs:163-173` (`env_windows_path_ext`, `%PATHEXT%` sample).
3. `scripts/platform_matrix.py` line numbers corrected to `ARCHES :60`, `LIBCS :65`, `wheel_tag :67`.
4. `extension_settings.rs:45-64`; `lsp/` sizes corrected to `mod.rs` 1591 / `hover.rs` 337 lines.
5. macOS libclang order corrected: `llvm-config --prefix` first (`common.rs:282-289`), then
   `xcode-select` (`:291-297`), then `DIRECTORIES_MACOS` with `/usr/local/opt/llvm*/lib` **last**
   (`:167-172`); the "do not `brew install llvm`" warning is kept, restated for that reason.
6. `Install-LLVM.ps1`: install dir `C:\Program Files\LLVM` on ARM64 (`:9`), `bin` added to `PATH` only
   in that branch (`:29`), x64 uses choco (`:31-33`) ⇒ "PATH not guaranteed — pin `LIBCLANG_PATH`".
7. gh caveats now quoted: asset renaming and duplicate-name error from
   <https://docs.github.com/en/rest/releases/assets>; `--clobber` deletion from
   <https://cli.github.com/manual/gh_release_upload>.
8. §1.1's prior-art prose folded (4 lines out; the submodule-vs-vendored-crate caveat moved into the
   Risks table, so no citation was lost).

Line count is 352, still above the node's "~200": the eight fixes added ~6 lines of quoted evidence and
the bulk is two pasted `cargo tree` outputs plus the per-target table. Flagged, not silently trimmed.

```console
$ uv run --no-project tools/plan.py validate
plan OK
```

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.18s
```
