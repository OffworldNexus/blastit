# Result MB.22.2

The accepted editor DX (MB.22.1) is now a buildable design:
`docs/design/blastfile-lsp-zed.md` (2 215 lines, §0–§12 + requirement index + review
resolution) and `ADR-0013` (115 lines, `Accepted`, node `MB.22`), plus the `ADR-0013`
row in `docs/decisions/README.md`. Four fresh-context lenses reviewed it in three capped
rounds; every BLOCKER and MAJOR is fixed in the document, and every remaining MINOR is
recorded under `## Review Resolution`.

## Files

| Path | Change |
|---|---|
| `docs/design/blastfile-lsp-zed.md` | new: §0 DX target and goals, §1 module boundaries, §2 messages/capabilities/error codes, §3 bounds and I/O, §4 positions, §5 diagnostics, §6 formatting incl. the CRLF decision, §7 hover + deferred definition, §8 extension anatomy, §9 grammar, §10 queries, §11 security, §12 matrix/rev pin/live proof/registry/hand-offs, requirement index, review resolution |
| `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` | new, one page: context, seven decisions, consequences, six rejected alternatives |
| `docs/decisions/README.md` | one row: `ADR-0013` … `Accepted` … `MB.22` |
| `docs/design/blastfile-lsp-zed.review-{security,implementability,testability,consistency}.md` | four lens reviews, three rounds each, each ending `VERDICT: PASS` |

No `crates/`, `editors/`, `Cargo.*`, `Makefile`, `scripts/` or plan-status change: this
node is documents only.

## The twelve pinned sections (decision → code site → proving tier)

| # | Decision | Code site | Proving tier |
|---|---|---|---|
| 1 | `blastit lsp` is a subcommand; `lsp.rs` owns framing/lifecycle/dispatch; `error.rs` owns `message()`; `blastfile/{sites,check}` owns warning positions; the `Lsp` arm drops capabilities | `crates/blastit/src/{lsp,cli,lib}.rs`, `crates/blastit-manifest/src/error.rs`, `blastfile/{sites,mod}.rs` | `critical_lsp.rs` + `include_str!` greps |
| 2 | ten messages + three tolerated; the exact capability document; `-32700/-32600/-32601/-32602/-32002`, `id` echoed as `Value` | `lsp.rs` | `critical_lsp.rs` |
| 3 | `Content-Length` in UTF-8 bytes; 8 KiB header cap incl. terminator; `MAX_FRAME_BODY_BYTES = 33_619_968`; body read by length; stdout frames only; four stderr sentences | `lsp.rs` | `critical_lsp.rs` |
| 4 | `Position` 1-based in `char`s → 0-based UTF-16, `NONE` → `(0,0)`, word-or-one-unit squiggle | `lsp.rs` (`Lines`) | `critical_lsp.rs` P1–P6 |
| 5 | severity 1/2, `source: "blastit"`, no `code`, `message()` = the CLI sentence without its prefix; six warnings located by `sites()`; 512 cap | `error.rs`, `blastfile/sites.rs`, `lsp.rs` | `critical_lsp.rs`, `tests/warning_sites.rs` |
| 6 | equal → `[]`, broken → `null`, else one `TextEdit` ending at `max_point_utf16`; **CRLF normalised to LF on save** | `lsp.rs`, `blastfile/print.rs` | `critical_lsp.rs` F1–F5 |
| 7 | hover from a 45-row table copied from the reference page, `$IDENT` paragraph, unknown → `null`; definition deferred behind `blastfile::symbols()` | `lsp.rs` | `critical_lsp.rs` H1–H5, `lsp.rs` H6 |
| 8 | `editors/zed/**`: empty `[workspace]`, `LICENSE` inside, `which` → `shell_env`, `uv run --frozen --no-sync`, no capability | `editors/zed/**` | `scripts/check_zed_extension.py` |
| 9 | in-repo grammar: newlines not extras, two string nodes + `regex_raw_string`, contextual keywords, `prec(1, …)` on the two overlaps, committed generated files, exact pinned CLI | `editors/tree-sitter-blastfile/**` | corpus, `generate` + `git diff`, fixture loop |
| 10 | fifteen captures (fourteen in the 45 + the `variable.special` fallback), capture by parent node, `outline`/`indents`/`brackets`, no `injections.scm` | `editors/zed/languages/blastfile/*.scm` | four `query` runs + three greps |
| 11 | no capability, process, file or network on the LSP path; never executes a document; never panics; bounded allocation; exit `0` only after `shutdown` | `lsp.rs`, `cli.rs` | `critical_lsp.rs` S1–S11 |
| 12 | matrix, both distribution paths, rev-pin procedure, registry preconditions, thirteen sibling-node hand-offs | the documents | the commands of §12 |

## Decisions taken (settled, not re-opened)

Hand-rolled `serde_json` + `std::io`; no new dependency; no async runtime on the LSP
path; no LSP crate; no second binary; full sync (kind 1); in-repo grammar with committed
`parser.c`; empty `[workspace]`; hover in v1 with definition deferred. The three the node
left open are decided here: **CRLF → LF on save** (§6.3, proven by F5), **`Worktree::which`
then `shell_env`, with `uv run --frozen --no-sync`** (§8.3), and the **grammar's
no-false-red superset rule** (§9.2).

## Evidence

```
$ ls docs/design/blastfile-lsp-zed.md docs/decisions/ADR-0013-blastfile-lsp-and-zed.md
docs/decisions/ADR-0013-blastfile-lsp-and-zed.md
docs/design/blastfile-lsp-zed.md

$ grep -n "ADR-0013" docs/decisions/README.md
19:| [ADR-0013](ADR-0013-blastfile-lsp-and-zed.md) | `blastit lsp` and the Zed Blastfile extension | Accepted | MB.22 |

$ ls docs/design/blastfile-lsp-zed.review-{security,implementability,testability,consistency}.md
docs/design/blastfile-lsp-zed.review-consistency.md
docs/design/blastfile-lsp-zed.review-implementability.md
docs/design/blastfile-lsp-zed.review-security.md
docs/design/blastfile-lsp-zed.review-testability.md

$ for f in docs/design/blastfile-lsp-zed.review-*.md; do printf "%s: " "$(basename "$f")"; tail -1 "$f"; done
blastfile-lsp-zed.review-consistency.md: VERDICT: PASS
blastfile-lsp-zed.review-implementability.md: VERDICT: PASS
blastfile-lsp-zed.review-security.md: VERDICT: PASS
blastfile-lsp-zed.review-testability.md: VERDICT: PASS

$ uv run --no-project tools/plan.py validate
plan OK

$ make docs-check
Build started
No issues found
Build finished in 0.34s

$ wc -l docs/design/blastfile-lsp-zed.md docs/decisions/ADR-0013-blastfile-lsp-and-zed.md
  2215 docs/design/blastfile-lsp-zed.md
   115 docs/decisions/ADR-0013-blastfile-lsp-and-zed.md
  2330 total

$ diff -q <design fences grammar> <shipped grammar.js>: IDENTICAL
```

The design's §9.2 fenced block is byte-identical to the `MB.22.4` artifact
(`diff` of the fence against `editors/tree-sitter-blastfile/grammar.js` lines 16–131 is
empty), which is the last round's BLOCKER closed at the source rather than by argument.

## Verdicts

| Lens | Final line | Note |
|---|---|---|
| security | `VERDICT: PASS` | 0 BLOCKER, 0 MAJOR |
| implementability | `VERDICT: PASS` | 0 BLOCKER, 0 MAJOR after the grammar/adoption fix round; 4 MINORs recorded |
| testability | `VERDICT: PASS` | 0 BLOCKER, 0 MAJOR; probed MAJORs fixed |
| consistency | `VERDICT: PASS` | 0 BLOCKER, 0 MAJOR; 12 MINORs recorded |

## Deferred (nothing silent)

- **`blastfile::symbols()` + `textDocument/definition`** — §7.4 names the API and the
  `Location` shape; a later node implements it.
- **Spans for the `Location`-carrying validation family** — §4.2/§5.3: those errors land
  at `(0,0)` in v1; giving them a site reuses `sites.rs`.
- **Seventeen sibling-node / docs edits** — §12.8 and H-014, including the `method`
  widening + `M-SEARCH` corpus line + `(dotted_name) @property` and the regeneration that
  `MB.22.4` is applying, and the one `docs/reference/cli.md` follow-up for a docs node
  (MB.22.1 is closed).
- **A live Zed pass** — §12.6, human-only by nature.
- **Recorded MINORs** (not fixed, per the cap): security N6–N19, implementability's four
  round-4 MINORs, testability m4/m11/m12, consistency's twelve — each is an
  `accepted`/`deferred → node` row in the design's `## Review Resolution`.

## Follow-ups

No work outside these four documents and `RESULT.md`; the plan edits above are H-014.
