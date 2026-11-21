# Consistency review — `blastit lsp` and the Zed Blastfile extension (round 4)

- **Lens:** consistency, independent re-review of the revision that answered round 3's
  `FAIL` (this file replaces the round-3 review at the same path)
- **Document judged:** `docs/design/blastfile-lsp-zed.md` (2 167 lines, `status: In review`)
- **Decisions judged:** `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md`; the `ADR-0013`
  row of `docs/decisions/README.md`; accepted pages `docs/local.md` `## Editors` and
  `docs/reference/cli.md` `## blastit lsp`
- **Node:** MB.22.2 · **Date:** 2026-11-17

## Revision read

I read the whole design, the ADR, the README row and both accepted pages, and checked every
repo fact the revision touches: `blastfile/mod.rs:202` (`MAX_SOURCE_BYTES = 16_777_216`),
`error.rs:1403`/`1468`, `uid.rs:32`, `cli.rs:295`/`638`, `print.rs:50`/`741`,
`validate.rs:74`/`1036`, `parser.rs:41`, `lower.rs:743`, `model.rs:916`, `check.rs:136`,
`blastit-py/src/lib.rs:62`, `CRITICAL_STAGE=target/python-critical` in the `Makefile`, and
the working `editors/zed/` + `editors/tree-sitter-blastfile/` trees (`extension.toml`,
`Cargo.toml`, `config.toml`, `README.md`, `src/lib.rs`, four `.scm` files, eight corpus
files). All resolve, and the repo's `Cargo.toml`, `extension.toml`, `config.toml` and
`src/lib.rs` match §8.1–§8.4 byte for byte (the `rev` is the documented all-zero
placeholder).

## The seven MAJORs

| # | Status | Evidence |
|---|---|---|
| 1 | **RESOLVED** | §12.9 (lines 2002–2015) states R53–R59 in a table with a `Proved by` column naming §12.1–§12.7 and their commands; the Requirement index row (2028) points at it. §12.1 has exactly 34 rows, matching R53. |
| 2 | **RESOLVED** | One rule remains: §2.2 item 4 ("**dropped silently**, publishes nothing") and the §12.8 `MB.22.3` row both say dropped silently for zero/>1 changes; the visible refusal is kept for `range`/unknown `uri` only (§2.5, §11.2). No "takes the first" survives. |
| 3 | **RESOLVED** | `diagnostics_are_capped` occurs once in §12.1 (line 1827), on §5.7's single 600-`GrantOnUnknownProxy` fixture, and `[511].message` = `89 more problems` follows from 600−511. |
| 4 | **RESOLVED** | §2.4's process-exit table now carries all three `1` causes (EOF/exit, unframeable stream, failed `caps::hold_none()`); §3.4 scopes the page's sentence to stream endings; §12.8's docs row adds both causes plus `--bogus` → `2`. The page itself (lines 81–82) is unchanged, as an explicit hand-off. |
| 5 | **RESOLVED** | §8.3 (line 1261) now separates the human form (`uv run blastit lsp`, the page's text) from the editor form (`run --frozen --no-sync --quiet blastit lsp`) and states the build-backend reason; the guard asserts the six argv literals and `--no-sync`; §12.8's `MB.22.4` row repeats it. `editors/zed/src/lib.rs` already carries it. |
| 6 | **RESOLVED** | §12.5 step 2 now sends `textDocument/formatting` on the **broken** buffer and asserts `null`; steps 3–4 assert `[]` and one edit on the same `uri`. The residual is the count, not the behaviour (new MINOR 3). |
| 7 | **RESOLVED** | `### Round 3, and the independent fix round` (2126–2153) says the two designer-written verdicts "**do not count**" and that both `FAIL` files are "the state of record"; the closing note says the two lenses are re-run against this revision. No `VERDICT: PASS`/consistency-PASS claim remains in the document. |

## New and leftover findings

| # | Grade | Where | Finding |
|---|---|---|---|
| 1 | MINOR | Requirement index | The `R53–R59` row appears twice (2028 and 2030) and the `§9`/`R40–R47` row is gone: the table jumps R35–R39 → R53–R59 → R48–R52 → R53–R59. The grammar's eight requirements now have no index/proving-tier line. Keep one R53–R59 row and restore `R40–R47 | §9 | §12.4, §9.4, §9.5`. |
| 2 | MINOR | §9.5, §9 R-ids | The new `qualified-function.txt` row cites `R43a`, which no `R-ids:` line defines (R43 covers only the two string nodes). Add it to R43 or drop the citation. |
| 3 | MINOR | §12.5 (1933, 1949), M18 row, Round-3 table | "one document through **four texts**" but the five steps send three texts (broken, canonical, two-space) on one `uri`. Say three, or add the fourth send. |
| 4 | MINOR | §9.5 title; §0.1 | Two stale counts the fix did not update: "seven files" over an **eight**-row corpus table (`qualified-function.txt` was added), and "five query files" where §8.1/§10 and the repo have **four**. |
| 5 | MINOR | §9.4 (1528–1536); §8.1 tree | The `LANGUAGE_VERSION`/manual-pass sentence is duplicated verbatim (a bad merge), and §9.4's "the extension README" plus the committed `Cargo.lock` are absent from §8.1's "committed tree" even though the repo now has both (`editors/zed/README.md` records the 0.25.10 pin). |
| 6 | MINOR | §11 S10 vs §2.4; §12.8 | S10 still lists only two `1` causes and the `exit_codes` test has no case for a failed `hold_none()`, so §2.4's third cause is untested and S10 reads as the complete table. Also, §12.8's page row names "a follow-up docs node" without a node id; the lead must create it or the accepted page ships stale. |
| 7 | MINOR | §2.2 (196 vs 208), §2.4 | "never answered **exactly when** … a `method` string and no `id`" conflicts with the `initialize` row ("a second `initialize` answers `-32600`") and §2.4's "`initialize` twice" for an id-less second `initialize`. Qualify both to requests carrying an `id`. |
| 8 | MINOR | §2.5 (326); §2.2/§12.8 | "The **three** visible notices" still names a `didChange` category that is two distinct messages (unknown `uri`, `range`), and the zero/>1 rule is cited as "(§2.5)" although §2.5 states only the visible ones. Say four, or drop the count and the §2.5 pointer. |
| 9 | MINOR | §7.2 vs implementability resolution (2083) | §7.2 and the closing note use three `..` for `include_str!` (correct from `crates/blastit/src/lsp.rs`); the implementability `14–28` row still prints four `..`. |
| 10 | MINOR | ADR-0013 | Still `Accepted` while the design is `In review` and its closing note says it needs the lead's acceptance; round 3 asked for alignment. `Proposed` until the design gate passes. |
| 11 | MINOR | `### Consistency` heading (2111) | It cites `review-consistency.md: 0 BLOCKER, 9 MAJOR, 16 MINOR`, but this file is round 3/4 (7 MAJORs then, 12 MINORs now); the M1–M9 source is no longer in the repo. Label it as the earlier round or archive its file name. |
| 12 | MINOR | §3.1 (`MAX_FRAME_BODY_BYTES`) | The `2×` derivation still assumes two-byte JSON escaping: `serde_json` writes six bytes for non-shortcut control characters, so a ≤16 MiB buffer of them exceeds the frame cap and ends the session with exit `1`, against §11 S9's "a broken document is not a broken server". State the six-byte bound or record the ending as accepted (prior MINOR 20). |

## What holds

- **Language law:** no `blastit.toml` anywhere in the design, ADR, README row, `local.md` or
  `cli.md`; `Blastfile` and `./Blastfile` throughout, agreeing with `VISION.md` and
  `ARCHITECTURE.md` §6. No `fpm`/FastCGI/alternative PHP transport — only the embedded
  `entrypoint php "public/index.php"` of ADR-0010.
- **ADR ↔ README:** the row is now the ADR's H1 verbatim (minus the `ADR-0013: ` prefix, as
  every row is), and decision 5 lists the four generated files (`parser.h` included),
  matching §9.1, R40 and §12.4.
- **`extension.toml` / `config.toml` / `Cargo.toml`:** nine rules, eight keys, empty
  `[workspace]`, `crate-type`, `zed_extension_api 0.7.0`, six `uv` argv literals — the
  blocks agree with the research and with the files now in `editors/`.
- **No requirement id is cited without being stated** except `R43a` (MINOR 2); every other
  cited `R`/`S` id has a defining `R-ids` line or §11.1 row.

## Verdict

No `BLOCKER` and no `MAJOR` remains: all seven round-3 MAJORs are closed in the text, and
the residual defects are count slips, two stale summary tables, one duplicated sentence, and
two traceability nits — fixable in one editing pass without re-opening the design. The
accepted page's exit-`1`/`uv run` rows remain an unowned hand-off (MINOR 6) that the lead
must schedule before 0.2.0 ships.

VERDICT: PASS
