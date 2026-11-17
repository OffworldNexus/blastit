# `blastfile-lsp-zed` — implementability review (round 5)

Revision read: `docs/design/blastfile-lsp-zed.md` (2 224 lines, `status: In review`, untracked,
`release/0.2.0` @ `e7b9581`), checked against the artifacts it now prints: `grammar.js` + committed
`src/`, `editors/zed/languages/blastfile/highlights.scm`, the corpus on disk,
`plan/nodes/MB/MB.22/MB.22.4/{NODE.md,RESULT.md}` and the manifest seams (`parser.rs:74`
`is_method_word`, `:396` `end_statement`, `tokens.rs:12` `Word`, `validate.rs:932` `valid_method`).
Commands re-run here with the pinned `/tmp/opencode/bin/tree-sitter` 0.25.10.

## Previously blocking

| Item | State | Evidence |
|---|---|---|
| B1 — §9.2 did not generate | **RESOLVED** | §9.2's fence is **byte-identical** to `grammar.js:16–131` (`diff -u` rc 0; 116 lines each): `view_decl` takes the two optionals inline, `table_grant` is the four verbs with `prec(1)` on `_table`, `dotted_name` uses `repeat1`. `MB.22.4`'s `generate` rc 0 plus `git diff --exit-code -- src` rc 0 prove the printed grammar *is* the generated one. |
| M1 — §10.2 could not satisfy §10.1 / §10.4 (c) | **RESOLVED** | §10.2 prints `(table_grant (keyword) @function)`; the shipped grammar + shipped `.scm` satisfy `grep -q 'function.*select'` on `captures.Blastfile` (MATCH; the CLI echoes `text:`). |
| M2 — no pinned `src/`, `method` widening unhanded | **RESOLVED** | §9.2 now pins the grammar the committed `src/` came from; §9.3's second strictness row names `method: /[A-Z]+/` → `M-SEARCH`/`GET2` as the one known false red; §12.8:2054 hands `MB.22.4` the widening `[A-Z][A-Z0-9_-]*` (verified right: lexer `Word` = `[A-Za-z0-9_-]`, first byte uppercase ⇒ `Method`, `valid_method` allows `-`), a corpus line, `(dotted_name) @property`, then regenerate. |

## The nine carried items

| Item | State | Evidence |
|---|---|---|
| a grammar `.` token | RESOLVED | `dotted_name: seq($.name, repeat1(…))` in the block and in `_value`; `qualified-function.txt` exists on disk; §10.2 has `(dotted_name) @property`. |
| b H1's unit count | RESOLVED | §7.5 unchanged at 12. |
| c duplicated `diagnostics_are_capped` | RESOLVED | one row; `600 − 511 = 89`. |
| d R53–R59 undefined | RESOLVED | §12.9 states all seven; the duplicate index entry is gone (one `grep` hit). |
| e no-`id` rule | RESOLVED | §2.2 one rule; §12.1 `[]` → `-32600`. |
| f zero/>1 `contentChanges` | RESOLVED | "dropped silently" in §2.2, §2.5 hand-off, §12.8. |
| g `npm ci`/lockfile gate | RESOLVED | §9.4 is a pinned **release binary**; asset `v0.25.10/tree-sitter-linux-x64.gz` answers **200**; `--version` line present (1578); no npm/npx/lockfile left. Host facts: `curl`, `gunzip`, `chmod` present; `npm`/`npx` absent and not needed. |
| h `formatting` on broken buffer | RESOLVED | §12.5 asserts `null`. |
| i §12.8 page rows | RESOLVED | rows intact. |

## New findings — all MINOR, none blocks a cheap implementer

**N1 — §9.4's body is printed twice and the first copy is stale.** 1536–1563 duplicate
1566–1593: same paragraph, same ten-command fence, same rationale. But the copy at 1541–1551 is
stranded under §9.3 and **lacks** `tree-sitter --version` (1578) — the only assertion tying the
download to R46's pin. Fix: delete 1536–1563; the copy after the heading is the superset.

**N2 — corpus count vs the handed-off method line.** §9.5's heading says "nine files" (1595); its
table has 8 rows and `test/corpus/` has 8 files. The `M-SEARCH` line §12.8 orders has no named
home. Fix: say "eight", or name a ninth `methods.txt` in the table so the order is placeable.

**N3 — §9.3's "line by line" looseness misses the optional comma.** §9.2 promises every
grammar-looser case is listed, yet `_values` accepts `functions st_distance unaccent` (no comma)
as one directive with two values while `parser.rs` refuses it (`grant()` → `comma_list` ends,
`end_statement()` wants `\n`/`;`/`}` and sees the Word `unaccent`). Fix: one row, "missing comma
in a list: coloured, no red".

**N4 — §9.5/§10.4 call a bare `tree-sitter` the document just proved is absent.** 1620 and
1764–1771 run `tree-sitter parse|query`, but §9.4 installs `/tmp/opencode/tree-sitter` and never
exports it; `tree-sitter` is not on this host's PATH (verified), so those two gates exit 127 as
written, and any PATH binary would satisfy them, unpinning R46. Fix: `export
PATH=/tmp/opencode:$PATH` in the §9.4 fence (or use `$TS_BIN` everywhere).

## Verified sound beyond the items

- The §12.8 hand-offs are executable by their owner: `MB.22.4`'s `paths` include both
  `editors/tree-sitter-blastfile/**` and `editors/zed/**`.
- §10.2 vs the committed `highlights.scm`: identical rules except two comment lines and whitespace,
  plus `(dotted_name) @property` — which is exactly the line §12.8 hands to `MB.22.4`, so the doc
  is ahead of the artifact, not contradicting it.
- §10.4 (b)–(d) empirically pass: `variable.special` present, `unrestricted`→`constant`,
  `select`→`function`, and the outline `item` count equals the section count (8 = 8).

Counts: **0 BLOCKER, 0 MAJOR, 4 MINOR** (all four are one-block deletions/additions).

VERDICT: PASS
