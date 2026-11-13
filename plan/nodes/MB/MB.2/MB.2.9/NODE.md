---
id: MB.2.9
title: "Document the static threat list, mechanics, CLI reference and the all-static check path"
kind: task
status: ready
owner: "doc-writer"
priority: 3
depends_on: [MB.2.2]
paths:
  - docs/security.md
  - "docs/how-it-works.md"
  - docs/ci.md
  - docs/reference/cli.md
  - docs/reference/manifest.md
  - python/tests/runnable_docs/inventory.toml
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T20:44"
updated: "2026-11-13T20:44"
---

# Goal

Make the public documentation tell the same story as the accepted `[static]` design: the threat list a reviewer looks for, the mechanics page's one section, the CI page's all-static path, the CLI reference, and the two reference rows that still contradict the DX page.

# Context

- `docs/frameworks/static-and-spa.md` (MB.2.1) states the promise and links out; `docs/design/static-and-spa.md` + `ADR-0007` (MB.2.2) hold the mechanics. This leaf is the only place those mechanics are allowed to reach the public site.
- Three known contradictions today: `docs/reference/manifest.md` still marks `app` as required (`app | yes*`) although an all-static manifest needs no `app` (DX page line 19); `docs/ci.md` and `docs/reference/cli.md` assert that `check` always runs pytest, while the all-static path (MB.2.6) runs none; and neither `docs/security.md` nor `docs/how-it-works.md` has a static-serving section.
- Lean-prose law: user pages stay concrete and under their length budgets; kernel mechanics (openat2, `RESOLVE_BENEATH`, inodes, seccomp) belong in `docs/security.md` / `how-it-works.md` and nowhere else.

# Acceptance

```console
# 1. Every public page that mentions static serving is consistent with the design.
$ grep -n 'static' docs/security.md docs/how-it-works.md | head
$ grep -n 'app' docs/reference/manifest.md | head -5          # no unconditional "app | yes"
$ grep -n 'pytest' docs/ci.md docs/reference/cli.md           # the all-static exception is stated

# 2. Docs gates: pages registered, fences accounted, strict build clean.
$ make docs-unit
$ make docs-check
$ make docs-test

# 3. The security page lists the refused classes in the DX page's own words (404 for
#    hidden existence, 400 for a malformed request) and links ADR-0007, not the reverse.
$ grep -cE 'traversal|symlink|NUL|dotfile' docs/security.md
```

# Notes

- Do not re-teach isolation or the capability model: link `deploy/` and `docs/security.md` from the DX page as it already does.
- Register every new fence in `python/tests/runnable_docs/inventory.toml`; this leaf owns `inventory.toml` edits for its own pages only (MB.2.1 registered the DX page, MB.2.3/MB.2.7 own the manifest and e2e fences).
- `docs/reference/manifest.md`'s `[route.static]` entry itself belongs to MB.2.3 (the parser change); this leaf fixes only the `app` row and the prose that the parser change makes true.
- Keep each page under its existing length budget; prefer one table row over a paragraph.
