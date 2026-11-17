---
id: MB.21.5
title: "Docs migration and release proof: live fences, retired-TOML guard, mutation audit, visual check"
kind: epic
status: done
priority: 2
depends_on: [MB.21.4]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-17T06:14"
---

# Goal

Finish the release in the place a developer actually reads: migrate the whole documentation site and
`README.md` to `Blastfile`, turn every example into a fence the gate really runs, keep the retired file
from creeping back, and prove it with the audit and the browser.

# Context

- `MB.21.1` wrote the two language pages; this epic rewrites the pages that *use* the language. Today 9
  files carry a complete manifest fence (`crates/blastit-manifest/tests/docs_fences.rs` `FILES`) and 23
  published pages exist with 46 accounted fences.
- Pages with a manifest or a manifest path: `README.md`, `docs/index.md`, `docs/getting-started.md`,
  `docs/local.md`, `docs/ci.md`, `docs/troubleshooting.md`, `docs/deploy/**` (four pages),
  `docs/frameworks/**` (five pages), `docs/reference/{manifest,cli,settings}.md`, `docs/how-it-works.md`,
  `docs/security.md`, and `ARCHITECTURE.md`.
- `docs/reference/manifest.md` is retired in favour of `docs/reference/blastfile.md`: its anchors are
  cited from `docs/troubleshooting.md`, `docs/frameworks/*`, `docs/deploy/*` and `docs/local.md`, so every
  jump must be repointed and then clicked (`make docs-check` only validates link targets, not intent).
- **Coordinate with `MB.20`** (frameworks regroup, held until v0.2.0 is merged): this epic rewrites the
  manifest fences inside `docs/frameworks/**` in place, and `MB.20` restructures the pages afterwards.
  `MB.20`'s `depends_on` gains `MB.21.5.1` so no writer touches those files twice.

## Children

| Node | Owner | What |
|---|---|---|
| `MB.21.5.1` | doc-writer | Site-wide migration: every page, `README.md`, `ARCHITECTURE.md`, the retired `reference/manifest.md`, the inventories |
| `MB.21.5.2` | implementer | Flip the inventory to live execution, strict `docs_fences`, and a negative-tested retired-file guard |
| `MB.21.5.3` | test-curator | Audit + `cargo-mutants` on `blastit-manifest`'s lexer/parser/CST/editor |
| `MB.21.5.4` | visual-checker | Browser proof of the two new pages, the nav and both colour schemes |

# Acceptance

```console
$ uv run --no-project tools/plan.py tree --under MB.21.5
$ make docs-check
$ make docs-unit
$ make docs-test
$ make doc-tone
$ ls docs/testing/AUDIT-MB.21.md docs/testing/VISUAL-MB.21.md
$ grep -rn 'blastit\.toml' README.md docs ARCHITECTURE.md --include='*.md' \
    | grep -vE 'docs/blastfile\.md|docs/reference/blastfile\.md|changelog\.md|/decisions/|/design/|/knowledge/|/testing/|/research/|/archive/'
$ grep -rln 'blastit\.toml' e2e python/tests/live_project --include='*.py' --include='*.sh' --include='*.yaml'
```

1. Every child is `done` with evidence; `make docs-check`, `make docs-unit`, `make docs-test` and
   `make doc-tone` all exit 0, with the two new pages' fences **executed**, not accounted.
2. The two greps print nothing: no live page and no fixture script names the retired file outside
   migration prose in the two language pages and the changelog.
3. `AUDIT-MB.21.md` records ≥ 75 % killed mutants on the parsing/editor modules with every surviving
   mutant filed as a follow-up node, and `VISUAL-MB.21.md` records both pages rendering in both schemes
   with the nav entries in place.
4. `MB.20`'s `depends_on` includes `MB.21.5.1` (`plan.py show MB.20`), so the frameworks rewrite runs
   once.

# Notes

- No prose restates the language: a page links `docs/reference/blastfile.md` instead of re-explaining a
  directive, and the guide stays the only tutorial.
- Keep `test_docs_assets.py`'s prose rule in view: its docstring names the two words a sentence must
  never pair, and `route` counts as the runtime word too.
