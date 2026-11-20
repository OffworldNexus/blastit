# MB.56.2.5 — RESULT

**Status:** done.

## What changed

- `crates/blastit-lsp/tests/e2e_lsp.rs` (588 LOC, one file, 10 tests): every case
  drives the spawned `env!("CARGO_BIN_EXE_blastit-lsp")` over stdio JSON-RPC with
  a small `Session` helper. `fresh_command()` clears the environment but forwards
  `LLVM_PROFILE_FILE`, so `make coverage`'s instrumented child keeps its profraw
  sink and no `BLASTIT_*` name can reach it.
- `crates/blastit-lsp/tests/critical_lsp_stdio.rs` deleted.
- `crates/blastit-lsp/src/**` holds **zero** `#[cfg(test)]` modules (verified by
  grep); nothing to delete there, no production logic touched.
- `crates/blastit-lsp/Cargo.toml`: `[lib] test = false` and `[[bin]] test = false`
  — both targets are empty of unit tests, and the two empty harness artefacts
  were what made `llvm-cov` spell `lib.rs`/`hover.rs` by bare file name, so the
  coverage report could not match the requested paths.
- `tools/coverage_report.py`: the objects handed to `llvm-cov` are now ordered
  **binaries first** (the same fix, at its real owner: the tool, not the crate).
  This is outside this node's `paths`; it is the only way the mandated
  `make coverage TESTS="-p blastit-lsp --tests"` gate can pass.

## Legs ported (MB.56.3 may now drop `crates/blastit/tests/critical_lsp.rs`)

| Leg | New test in `e2e_lsp.rs` |
|---|---|
| `initialize` capability document byte for byte + `shutdown`/`exit` status `0`, empty stderr | `the_standalone_binary_speaks_the_same_session` |
| Cleared vs poisoned `BLASTIT_*` environment changes nothing | `a_poisoned_environment_changes_nothing` |
| `didOpen` diagnostic: severity-1 at `4:8–4:12`, the CLI sentence, no `code`/`tags` | `the_standalone_binary_speaks_the_same_session` (via `assert_typo`) |
| `didChange` republishes the new text's diagnostic, echoes the version, clears on a clean change | `did_change_republishes_the_new_text` |
| **Every** error is published: two literal queries give two diagnostics at their own spans (not just the first) | `every_error_is_published_and_did_close_clears` |
| `didClose` publishes `[]` with **no** `version` | `every_error_is_published_and_did_close_clears` |
| Diagnostic `character` is UTF-16: `😀` puts `=` at 20 (not 19 `char`, not 22 bytes) | `diagnostics_count_utf16_units` |
| Hover `$?` inside a `query`: the page's exact paragraph over the 2-unit token span | `hover_explains_the_two_sql_tokens` |
| Hover `...`: the page's exact paragraph over the 3-dot span | `hover_explains_the_two_sql_tokens` |
| `$?` / `...` outside a `query` answer `null` | `hover_explains_the_two_sql_tokens` |
| Directive vocabulary: `postgres` and `view` paragraphs (`HOVER_TABLE`) over their own spans | `hover_explains_the_directive_vocabulary` |
| `$DATABASE_URL` env paragraph (`env_var_paragraph`), never its value | `hover_explains_the_directive_vocabulary` |
| Formatting canonical buffer: `[]` | `formatting_is_empty_edited_or_declined` |
| Formatting mis-indented buffer: one edit `0:0–5:0`, `newText` = `fmt` output | `formatting_is_empty_edited_or_declined` |
| Formatting a broken buffer: `null`, never a JSON-RPC error, session alive | `formatting_is_empty_edited_or_declined` |
| Quick fix `"…$1"` spelling: title, kind, one `TextEdit`, exact `newText`, UTF-16 range, applied doc offers `[]` | `the_quick_fix_parameterizes_both_spellings` |
| Quick fix `'…$1'` spelling: same oracle (`SINGLE_FIX`) | `the_quick_fix_parameterizes_both_spellings` |
| Caveat: a caret on `postgres $DATABASE_URL` (off every `query`) offers `[]` | `the_quick_fix_parameterizes_both_spellings` |
| `codeAction` in an editor's shape: `context: {"diagnostics": []}` plain, and the published literal diagnostic in the applied case | `the_quick_fix_parameterizes_both_spellings` |
| `only` contract: `["quickfix"]` returns the action, and `["refactor"]` also does — the server does not filter `only` (capability is the boolean `true`), asserted explicitly | `the_quick_fix_parameterizes_both_spellings` |
| The documented `query` squiggle: `query 'SELCT id FROM t'` → one severity-1 diagnostic, the CLI sentence, range over the first token (`SELCT`, 5 units) | `every_error_is_published_and_did_close_clears` |
| `--version`/`-V`: one line on stdout, `0`, empty stderr | `the_argv_surface_is_the_probe_the_refusal_and_the_session` |
| Any other argv (`--help`, `-h`, `--bogus`): one sentence on stderr, `2`, empty stdout | same test |
| No argument = session; EOF without `shutdown` = `1`, silent | same test |

### Not ported (still only in `crates/blastit/tests/critical_lsp.rs`)

The frame reader's malformed/-too-long/-too-big error table and ids/`id: null`
handling, `-$32700`–`-32002` codes, `MAX_DOCUMENTS`/oversized-document notices,
`workspace/didChangeConfiguration`, `textDocument/definition` declined,
`blastit lsp` CLI delegation, and the source-hygiene guards. MB.56.3 owns that
file; this node ports the §7 user-facing legs.

## Second pass (consolidated review)

- `codeAction` now sends the required `context` member, both plain and with the
  diagnostic actually published for the literal, plus both `only` shapes.
- The documented `query` parse squiggle is opened and asserted verbatim.
- `docs/local.md`'s editor sentence now says where the fix appears (any `query`
  literal; paste the denied statement, swap `…` for `$?`), not a `line:col` a
  denial never prints.
- `crates/blastit-lsp/src/hover.rs`'s table comment names
  `crates/blastit-lsp/tests/e2e_lsp.rs` as the parity oracle instead of the
  removed `mod tests`.

## Evidence

```
$ cargo test -p blastit-lsp --locked
running 10 tests ... test result: ok. 10 passed; 0 failed

$ make coverage TESTS="-p blastit-lsp --tests" FILES="crates/blastit-lsp/src/lib.rs crates/blastit-lsp/src/hover.rs"
coverage: 1 test executable + 1 bin object, 10 tests passed
  crates/blastit-lsp/src/lib.rs  lines 80.0%  regions 71.0%
  crates/blastit-lsp/src/hover.rs  lines 95.8%  regions 92.9%
(exit 0; no `.profraw` anywhere outside target/)

$ make zed-check
test result: ok. 11 passed; 0 failed (grammar ↔ Zed highlight/corpus agreement included)

$ make fast-check
12 files already formatted; plan OK; portable ELF OK; workspace tests + lint clean
```

## Follow-ups

- `tools/coverage_report.py`'s binaries-first object order is the durable fix for
  `llvm-cov` dropping absolute-path library rows; it is out of this node's
  `paths` and was required by the review's coverage mandate.
