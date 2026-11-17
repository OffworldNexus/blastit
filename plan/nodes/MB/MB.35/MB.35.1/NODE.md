---
id: MB.35.1
title: "Verify pg_query on Windows MSVC and both Apple targets, plus the Zed download/Zip/which facts, with citations"
kind: task
status: ready
owner: researcher
priority: 1
depends_on: [MB.32]
paths: ["docs/research/2026-11-17-cross-platform-blastit-lsp.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-17T13:18"
---

# Goal

Answer, with citations and a local build where possible, the four questions that decide whether
`MB.35` is possible at all. Do not write production code; write
`docs/research/2026-11-17-cross-platform-blastit-lsp.md`.

1. **`pg_query 6.2.1` / `libpg_query` on the three targets.**
   - `x86_64-pc-windows-msvc` on `windows-latest`: does libpg_query's C build work under MSVC,
     and what does the `pg_query` build script require (`LIBCLANG_PATH`, bindgen, a C
     compiler)? Which LLVM path does the runner image actually have?
   - `aarch64-apple-darwin` on `macos-latest` (Apple Silicon) and `x86_64-apple-darwin` (a
     `macos-15-intel` runner, or a cross-build): is a native runner available, and does
     libpg_query build there?
   - `x86_64-unknown-linux-gnu` / `aarch64-unknown-linux-gnu`: already proven by the wheel
     matrix — cite the job/leg rather than re-derive.
   - State the boundary: if a target cannot build, what is the honest fallback (documented
     unsupported target; no silent no-op validator)?
2. **Zed's extension API on macOS and Windows.** From the pinned `zed_extension_api 0.7.0`
   source: `current_platform()`'s `Os`/`Architecture` variants (is `macos-15-intel` reported as
   `X8664`?), `download_file(..., DownloadedFileType::Zip)` semantics (does it extract or keep
   the archive? where?), `make_file_executable` on Windows, and `worktree.which("blastit-lsp")`
   for a `.exe`. Cite the crate file and line.
3. **GitHub Release asset mechanics.** Does `gh release create dist/*` upload a `.zip`
   unchanged, and does the browser URL redirect to `objects.githubusercontent.com` (the two
   hosts `MB.33` must declare as download capabilities)? Cite the workflow and Zed's documented
   capability hosts.
4. **The dependency claim.** Prove or refute that the current `lsp` module's transitive normal
   dependencies are portable: `cargo tree -p blastit --edges normal -i blastit-manifest` and
   `cargo tree -p blastit-manifest --edges normal` (paste the trees), and name every crate that
   is `#[cfg(unix)]`-only.

# Context

- **Why this is research and not design.** Two of the four answers may invalidate the mandate's
  chosen shape (a portable binary). `MB.35.2`'s docs and `MB.35.3`'s design must not promise a
  Windows artifact that cannot be built; the owner asked for a real editor experience, not a
  placeholder.
- **Read locally first.** `~/.cargo/registry/src/**/pg_query-6.2.1/` (build.rs, README,
  `libpg_query` submodule), `~/.cargo/registry/src/**/zed_extension_api-0.7.0/src/`, the
  checked-in `editors/zed/Cargo.toml`/`Cargo.lock`, `.github/workflows/publish-pypi.yml`, and
  `scripts/platform_matrix.py`. Cite file + line, not memory. Use `webfetch`/`websearch` only
  for runner-image facts (LLVM path, available macOS runners) and cite the URL.
- **No network installs.** Do not `cargo add`, do not modify the workspace, do not run
  `cargo build` with a new target triple that would download a toolchain: the answer is a
  paper-and-registry finding plus, at most, a `cargo tree` that already exists.
- **Do not** edit `docs/design/**`, `docs/decisions/**`, `Cargo.toml`, or any page. Only the new
  research file.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo tree -p blastit --edges normal | grep -E 'blastit-(sys|sqlpolicy|manifest)|rustix|libc|pyo3'
$ cargo tree -p blastit-manifest --edges normal
$ make docs-check        # the research file is excluded from the site and must not break it
```

The note must contain, under headings: **pg_query per target** (a table: target, runner, build
result or cited evidence, `LIBCLANG_PATH`/toolchain value), **Zed API facts** (crate path, line,
exact enum/function behaviour), **release assets** (upload + redirect hosts), **dependency
tree** (the pasted `cargo tree` tails), and **risks** (one line each, with the decision it
forces on `MB.35.3`).

End with `VERDICT: GO`, `GO WITH DROPPED TARGETS: <list>` or `NO-GO: <reason>` — the design
leaf reads this verdict, and an over-optimistic GO is a failed node.

# Notes

- **A citation is a path and a line, or a URL and a quoted sentence.** "It should work" is not
  a finding.
- If Windows fails only at `bindgen` (LLVM missing) but the C build is fine, that is a
  `GO WITH` finding naming the env var — not a NO-GO.
- If `pg_query` has no Windows support at all, say so and propose the honest shape for
  `MB.35.3` (e.g. `blastit-lsp` Windows artifact ships without SQL parsing, with the docs
  stating exactly which diagnostics are absent). Do not silently drop the `MB.34` promise.
- Keep it under ~200 lines; this is a decision memo, not a survey.
