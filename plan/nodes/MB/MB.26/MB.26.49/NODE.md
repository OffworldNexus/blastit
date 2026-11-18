---
id: MB.26.49
title: "Assert the reference page's 9:16/11:26/13:31 sites and a non-zero UidLayout base"
kind: task
status: proposed
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit-manifest/tests/validate.rs", docs/reference/blastfile.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

The reference page's refusal table carries three sites as literals — `9:16` (`query ''`),
`11:26` (`SELECT 1; SELECT 2`), `13:31` (`SELECT id FROM ...`) — and `blastfile-dsl.md` §20's
R34 gate column plus the accepted T3–T10 resolution require them asserted "as one three-line
fixture in `tests/validate.rs`". Today only `9:16` exists (protected), `:26` appears on line 5,
and column `31` appears nowhere, so the page can drift from the truth with every test green.

Also pin §13.4.5's "`base` shifts no line or column": the same document parsed with a non-zero
`UidLayout` base must produce the same three `Position`s.

# Context

- The page's numbers imply one 8-space-indented fixture with the entries on lines 9, 11, 13.
- `Manifest::parse_blastfile(source, 20_000)` is the non-zero-base entry point used elsewhere.
- `docs/reference/blastfile.md:298-301` is the page table.

# Acceptance

```console
$ cargo test -p blastit-manifest
$ make docs-check
```

One fixture asserts `Position::at(9,16)`, `at(11,26)` and `at(13,31)` as literals (through
`ManifestError::position()`), asserts the same three through `parse_blastfile(…, 20_000)`, and
the page's table still prints `9:16`/`11:26`/`13:31`; if the code disagrees with the page, fix
the page and say so in `RESULT.md` — the page is the contract, not the test.

# Notes

- Prefer `tests/validate.rs` (per the accepted resolution); the protected `9:16` case stays.
- No new fixture file; keep the three entries in one document.

# Provenance

Re-filed from `MB.34.7.3` (`AUDIT-MB.34` G3) at the `v0.2.0` gate: single 3-line fixture pinning `9:16`/`11:26`/`13:31` and non-zero `UidLayout` base together.
