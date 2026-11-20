---
id: MB.56.2
title: "Delete all non-§7 unit/component micro-tests and consolidate the 4 focused TESTING_PLAN.md §7 suites"
kind: epic
status: ready
priority: 1
depends_on: [MB.56.1]
paths: ["crates/**", "editors/**", Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T16:02"
updated: "2026-11-20T17:15"
---

# Goal
Enforce `TESTING_PLAN.md` §1 & §7 across all workspace crates: delete all non-§7 inline `#[cfg(test)]` / `src/**/tests*` unit tests and isolated component micro-tests, and consolidate the 4 allowed `TESTING_PLAN.md` §7 focused suites!

1. **Delete non-§7 unit & component micro-tests across 16 crates:**
   - Remove `#[cfg(test)]` modules, `src/**/tests*` files, and `tests/*.rs` files in `blastit/src/**`, `blastit-scheduler`, `blastit-dbproxy`, `blastit-view`, `blastit-view-python`, `blastit-view-php`, `blastit-view-static`, `blastit-php-sapi`, `blastit-py-sapi`, `blastit-py-shim`, `blastit-events`, `blastit-env`, `blastit-ingress`, `blastit-sys` (keep `<= 250` LOC malformed-ELF/wire boundary test ONLY if unreachable via E2E, else 0), `blastit-frame` (keep `<= 200` LOC corrupt/truncated frame boundary test ONLY), and `blastit-lsp/src/**`.
   *(Note: before deleting `blastit-scheduler`, `blastit-view-static`, `blastit-view-python`, and `blastit-view-php` tests, note any unique user-visible security/behaviour case in `RESULT.md` so `MB.56.3` ensures 100% E2E coverage in `crates/blastit/tests/`!)*

2. **Consolidate the 4 `TESTING_PLAN.md` §7 Focused Suites (tight, zero duplicate files, real assertions):**
   - **`crates/blastit-manifest` (`<= 1,500` test LOC total):** Delete `critical_manifest_011.rs` (retired TOML), `mutation_gaps.rs`, `edit_branches.rs`, and collapse the 30+ fragmented test files + `src/` tests into **1–2 clean files** (`tests/blastfile_language.rs`) covering grammar parsing, line:col diagnostics, route precedence (`exact`, prefix, `regex`), traits, comment-preserving formatting, lossless grant insertion, **plus parsing & format-checking every `Blastfile` fence across `README.md` and `docs/`**!
   - **`crates/blastit-sqlpolicy` (`<= 1,500` test LOC total):** Collapse `9.7k` LOC of split unit + integration tests into **1–2 clean files** (`tests/sql_policy.rs`) covering table/column/function allowlists, `$?` / `IN ($?, ...)` / multi-row `VALUES` matching, prepared statements, session-setup queries, and strict rejection of SQLi, multi-statement batches, unparameterized literals, and ungranted columns.
   - **`crates/blastit-http` (`<= 800` test LOC total):** Collapse `6.0k` LOC into **1 clean file** (`tests/http_framing.rs`) covering HTTP/1.1 request framing, `body` policies (`none`, `json`, `form`, `raw unvalidated`), size limits, client disconnects, and `Content-Length` / `Transfer-Encoding` smuggling & upgrade rejection.
   - **`crates/blastit-lsp` (`<= 600` test LOC total) + Zed (`make zed-check`):** Consolidate into `crates/blastit-lsp/tests/e2e_lsp.rs` driving the real `blastit-lsp` binary over stdio JSON-RPC (diagnostics on `didOpen`/`didChange`, `$?`/`...` hover, `textDocument/formatting`, and the SQL literal-to-`$?` quick-fix code action), keeping `make zed-check` compiling the Tree-sitter grammar & WASM Zed extension cleanly.

# Acceptance
- `make fast-check && make zed-check` green with zero warnings (`-D warnings`)
