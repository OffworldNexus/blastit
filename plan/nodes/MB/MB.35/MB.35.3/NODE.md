---
id: MB.35.3
title: "Amend ADR-0013 and design 8.3: blastit-lsp crate boundary, portability invariant, three-step resolution and the five-target matrix table"
kind: task
status: ready
owner: designer
priority: 1
depends_on: [MB.35.2]
paths:
  - "docs/decisions/ADR-0013-blastfile-lsp-and-zed.md"
  - "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md"
  - "docs/design/blastfile-lsp-zed.md"
  - ARCHITECTURE.md
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-18T06:51"
---

# Goal

Decide and record the architecture the extraction and packaging implement, and reverse the one
recorded decision this mandate overturns. Four artifacts:

1. **`docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` — a dated amendment.**
   - The accepted text stays as the record (the `blastit lsp` choice and its comparison table).
   - The amendment states the new premise (the owner wants the editor on macOS/Windows, while
     `blastit`'s serving mechanics stay Linux-only), the decision (a `blastit-lsp` binary in its
     own crate, `blastit lsp` kept as a delegation), and what the old table's costs became
     (a second artifact to publish is the *point*, not the cost; the second name is what the
     editor resolves first).
   - It cites `MB.35.1`'s verdict for every platform, including any dropped target and what that
     target loses.
2. **`docs/design/blastfile-lsp-zed.md` §1/§8.3 — the boundary and the resolution order.**
   - §1's boundary table gains the crate: `crates/blastit-lsp` depends on `blastit-manifest` and
     `serde_json` and **nothing else**; no `blastit-sys`, `rustix`, `nix`, `libc`, `pyo3`,
     `blastit-php-sapi`, no `std::os::unix`. State the check that enforces it (`MB.35.6`).
   - §8.3's resolution table becomes the three-step order `LspSettings` →
     `which("blastit-lsp")`/`which("blastit")` → release download, with the exact asset names
     and formats. The *implementation* is `MB.33`; this doc is the contract it codes against.
   - State the equivalence obligation: `blastit lsp` and `blastit-lsp` are one `run()`; the
     protected `critical_lsp.rs` suite runs against the former **unchanged**.
   - State the security position: the LSP is a text tool, links no privilege crate, and the
     portable artifact cannot become a serving binary by accident because the serving crates are
     not in its dependency graph.
3. **`docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4 — one table row (or a short
   note).** The packaging matrix is the table of record for artifacts; add the `lsp` family
   (five targets, two formats, no PHP shim, no Python) or point at
   `scripts/platform_matrix.py`'s new family. Do not restate the matrix in two places.
4. **`ARCHITECTURE.md` — the crate table and the trust boundary.** One row for
   `blastit-lsp` with its dependencies and its "never serves" rule; the section that lists the
   editor surface says the binary is cross-platform while serving is Linux-only.

Then four goldfish critics (`security`, `implementability`, `testability`, `consistency`) review
the diff; record each `VERDICT: PASS` or finding in `RESULT.md`.

# Context

- **What must be decided, concretely, because two leaves depend on it:**
  - the crate's public surface: `blastit_lsp::run() -> u8`, the binary `blastit-lsp`, and
    whether `crates/blastit/src/lib.rs` re-exports it as `pub use blastit_lsp as lsp;` (so
    `cli.rs` needs no edit) or `cli.rs` calls `blastit_lsp::run()` directly;
  - the asset naming and layout: `blastit-lsp-<target>.tar.gz` (Linux/macOS) and
    `blastit-lsp-<target>.zip` (Windows), each archive holding one top-level directory
    `blastit-lsp-<target>/` with the executable — the same shape the standalone archives have,
    so the extension's `make_file_executable` path stays one expression;
  - which targets are in: the five in the epic, minus anything `MB.35.1` proves impossible;
  - the runner for macOS x86_64 (`macos-15-intel` native, or a documented cross-build) and the
    `LIBCLANG_PATH` value on `windows-latest`.
- **The portability invariant is security-relevant, not cosmetic.** `blastit-sys` holds the
  seccomp/capability/`fanotify` code and `blastit-php-sapi` holds the C SAPI shims; a crate that
  links them cannot be cross-built honestly, and a language server that could acquire a
  capability would widen the editor's trust surface for no reason.
- **Do not write Rust or YAML here.** No crate, no workflow, no docs page (the user pages are
  `MB.35.2`'s).
- **One writer per doc** (`LESSONS.md`, `MB.3.14`): assemble from `.part` files.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ make docs-check
$ git diff --stat -- docs/design docs/decisions ARCHITECTURE.md
```

plus, in `RESULT.md`:

1. the four critic reports (lens, verdict, findings and resolution);
2. the amendment's exact paragraph that reverses the `blastit-lsp` rejection, quoted;
3. the resolution-order table and the five asset names, verbatim;
4. the crate boundary line a guard can enforce (`dependencies = { blastit-manifest, serde_json }`
   and the forbidden list), so `MB.35.6` does not re-decide it;
5. one sentence per target on what a developer gets there, matching what `MB.35.2` documented.

The design is accepted when `MB.35.4` (extraction) and `MB.35.5` (CI) can be written without
re-deciding a name, a format, a target or an invariant.

# Notes

- **Do not rewrite history.** `ADR-0013`'s accepted text and design §8.3's old rows stay; the
  amendment is dated and states the new premise. A reviewer must be able to see both decisions
  and why the second one wins.
- **No promise of a Windows server.** The design says explicitly what the portable binary does
  (edit and diagnose a `Blastfile`) and what stays Linux (serve, proxy, PHP workers). That
  sentence is the one an editor user must not misread.
- Keep the matrix in `scripts/platform_matrix.py` as the machine table; the ADR/design name it
  instead of copying numbers that will drift.
