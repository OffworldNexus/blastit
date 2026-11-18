# Result MB.34

## Summary

Shipped PostgreSQL 18 SQL template validation (`$?`, `IN ($?, ...)`, `VALUES (…), ...`) with exact decoded inner `line:col` spans across `blastit-sqlpolicy`, `blastit-manifest`, `blastit check`, and `blastit lsp` / `blastit-lsp`, alongside `$?`/`...` hover documentation, the `"Parameterize SQL for Blastfile ($?)"` LSP Quick Fix (`codeAction`), and Tree-sitter SQL interior tokenization + highlighting (`editors/tree-sitter-blastfile`, `editors/zed`).

## Evidence

- Leaves `MB.34.1`–`MB.34.6` and `MB.34.8` `done`; outside-in proof `MB.34.7` `done` (`reviewer: VERDICT PASS`, recorded verbatim in `docs/testing/E2E-MB.34.md`).
- `cargo-mutants` on `crates/blastit-sqlpolicy/src/template.rs`: `122/128` viable caught (`95.3 %`, 6 survivors equivalent/unreachable, 0 timeouts); `QuerySite`/position helpers in `blastit-manifest`: `20/21` (`95.2 %` vs critical tier alone, `100 %` across standard + critical tiers, `docs/testing/AUDIT-MB.34.md`).
- `grep -rn 'pg_query' crates/blastit-manifest/Cargo.toml` is empty; `make zed-check`, `make docs-check`, `make check` (204 suites ok), and `make e2e` (411 checks ok) all pass at `9784965`.
- Live Zed GUI click/hover/screenshot explicitly recorded as `NOT RUN (no Zed binary on this host)` in `docs/testing/E2E-MB.34.md`, backed by wire-level JSON-RPC `didOpen` / `codeAction` / `hover` transcripts and `tree-sitter parse` / `query` proofs.

## Deferred Coverage (`MB.26`)

Per project gate precedent (an epic closes with zero deferred claims; non-claim test-tier and gate-hardening items move to `MB.26`), the 8 last-mile audit-coverage leaves `MB.34.7.2`–`MB.34.7.9` from `docs/testing/AUDIT-MB.34.md` are re-filed as `MB.26.48`–`MB.26.55` (`proposed`) and marked `superseded` under `MB.34.7`.

## Files

- `docs/reference/blastfile.md`, `docs/local.md`, `docs/ci.md`, `docs/troubleshooting.md`
- `docs/design/blastfile-lsp-zed.md`, `docs/design/blastfile-dsl.md`, `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md`, `docs/decisions/ADR-0003-wire-learn-and-literal-redaction.md`
- `crates/blastit-sqlpolicy/src/template.rs`, `crates/blastit-sqlpolicy/tests/critical_query_template.rs`
- `crates/blastit-manifest/src/{error,model,validate}.rs`, `crates/blastit-manifest/src/blastfile/lower.rs`, `crates/blastit-manifest/tests/critical_query_sites.rs`
- `crates/blastit-lsp/**`, `crates/blastit/tests/critical_lsp.rs`
- `editors/tree-sitter-blastfile/**`, `editors/zed/**`
- `docs/testing/E2E-MB.34.md`, `docs/testing/AUDIT-MB.34.md`

## Follow-ups

- `MB.26.48`–`MB.26.55` (`AUDIT-MB.34` test-tier promotions & `zed-check` corpus runner).
