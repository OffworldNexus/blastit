---
id: MB.56.2.5
title: "Consolidate blastit-lsp e2e_lsp.rs (<=600 LOC) over the real binary + keep zed-check"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.56.2.4]
paths: ["crates/blastit-lsp/**", "editors/**", docs/local.md, "tools/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T17:15"
updated: "2026-11-20T18:42"
---

# Goal
Give the language server its one §7 suite: `crates/blastit-lsp/tests/e2e_lsp.rs` (**≤ 600 LOC**) driving the **real `blastit-lsp` binary** over stdio JSON-RPC, and keep `make zed-check` meaningful.

1. Consolidate the existing LSP tests into `e2e_lsp.rs`: `initialize`/`shutdown`/`exit`, diagnostics on `didOpen` and `didChange`, `$?`/`...` hover documentation, `textDocument/formatting`, and the SQL-literal-to-`$?` quick-fix code action — all through the spawned binary, not the library API.
2. Delete the superseded files and every inline `#[cfg(test)]` in `crates/blastit-lsp/src/**`.
3. `make zed-check` must keep compiling/testing `editors/zed` and proving the grammar↔Zed highlight/corpus agreement (it was repaired in `MB.56.1.3`); leave it green, do not weaken it.

# Acceptance
- `make fast-check` + `make zed-check` green, zero warnings; `crates/blastit-lsp/tests/` ≤ 600 LOC in one file; no `cfg(test)` left in `crates/blastit-lsp/src`.
