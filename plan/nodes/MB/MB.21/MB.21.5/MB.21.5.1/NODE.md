---
id: MB.21.5.1
title: "Purge the retired manifest from the site, README, ARCHITECTURE and live briefs, and retire the v1 reference page"
kind: task
status: ready
owner: "doc-writer"
priority: 2
depends_on: [MB.21.4.4]
paths:
  - README.md
  - "docs/**"
  - ARCHITECTURE.md
  - zensical.toml
  - python/tests/runnable_docs/inventory.toml
  - plan/nodes/NODE.md
  - "plan/nodes/**/NODE.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T01:49"
---

# Goal

Purge the retired manifest format from everything a reader can reach — the published site, `README.md`,
`ARCHITECTURE.md`, the nav, the fence inventory and every live plan brief — delete the retired
reference page, repoint the links into it, and leave the site reading as if `Blastfile` were the only
manifest Blastit ever had.

# Context

- Preceded by: `MB.21.1.1` (the two language pages and the nav row), `MB.21.4.4` (every crate, script,
  fixture and the Makefile). This node is the site-wide sweep and the proof that nothing is left.
- Delete `docs/reference/manifest.md` — a page, so three things move in the same commit: the file, its
  `[[page]]` block in `python/tests/runnable_docs/inventory.toml` (lines 309-310),
  and every inbound link. `python/tests/runnable_docs/fences.py:230-251` plus `check_inventory`
  require exactly this pairing: a page on disk without an entry, or an entry without a page, fails
  `make docs-unit`.
- Published pages that link the retired page today, and their new homes (all on
  `docs/reference/blastfile.md`): `docs/deploy/anything-else.md:110` (`#protocol-postgresql` →
  `#stores`), `docs/frameworks/django.md:76` and `:85` (`#groups` → `#traits`, `#ingress` →
  `#view-directives`), `docs/frameworks/flask.md:81` (`#groups` → `#traits`),
  `docs/reference/settings.md:154` (`#proxies` → `#stores`), `docs/troubleshooting.md:20`
  (`#ingress` → `#view-directives`) and `:34` (`#groups` → `#traits`). Rewrite each link in place;
  keep the sentence's meaning.
- Pages that carry a complete manifest fence and must show a `Blastfile` (zero `=` outside raw
  strings), keeping their line budgets: `README.md`, `docs/index.md`, `docs/getting-started.md`,
  `docs/local.md`, `docs/frameworks/{index,django,flask,php,static-and-spa}.md`. The fence opening
  itself (a `blastfile` fence titled `Blastfile`) and the strict `docs_fences` flip are `MB.21.5.2`'s;
  update
  `crates/blastit-manifest/tests/docs_fences.rs`'s `FILES` count only if this node adds or removes a
  complete document.
- `ARCHITECTURE.md` carries architecture examples in fenced blocks tagged with the retired format
  (lines 870, 1055) and prose at 1025, 1224, 1676; retag the fences `blastfile` and rewrite the prose.
  `docs/VISION.md` carries it at lines 13, 23, 24, 42.
- Live plan briefs: eleven briefs whose `status` is not `done`/`superseded` still name it (measured
  2026-11-16; find the current set with the loop in `# Acceptance`); they are instructions, so rewrite
  them. `MB.21.4`'s epic brief is one of them: its title and `# Acceptance` still describe a
  "migration refusal" — replace that with the missing-file sentence and the ordinary syntax error,
  and leave its structure alone.
- Records are never touched: `LOG.md`, `RESULT.md`, briefs whose status is `done` or `superseded`,
  `docs/{decisions,design,research,knowledge,testing,archive}/**` and `docs/changelog.md`.
- `docs/deploy/**` keeps its pooler guidance: that is a deployment concern (PgBouncer or a managed
  pooler), never a `Blastfile` knob — say so in one sentence where the pooler is introduced.
- Do not restructure `docs/frameworks/**`: only manifest fences and reference links change there;
  `MB.20` regroups those pages afterwards and depends on this node.

# Acceptance

```console
$ grep -rnE 'blastit[.]toml' README.md docs ARCHITECTURE.md --include='*.md' \
    | grep -vE 'docs/changelog\.md|docs/(decisions|design|research|knowledge|testing|archive)/'
$ for f in $(grep -rlE 'blastit[.]toml' plan/nodes --include=NODE.md); do \
    printf '%s %s\n' "$(grep -m1 -oE 'status: [a-z]+' "$f" | cut -d' ' -f2)" "$f"; done \
    | grep -vE '^(done|superseded) '
$ grep -rn 'reference/manifest\.md' README.md docs ARCHITECTURE.md --include='*.md'
$ test ! -e docs/reference/manifest.md && echo gone
$ grep -c 'reference/manifest.md' python/tests/runnable_docs/inventory.toml
$ uvx --offline zensical build --strict
$ make docs-unit
$ make doc-tone
$ wc -l README.md docs/blastfile.md docs/getting-started.md docs/local.md
```

1. The first two commands print nothing: no live page, `README.md`, `ARCHITECTURE.md` or live brief
   names the retired file, and every remaining hit anywhere in the tree is a record (`LOG.md`,
   `RESULT.md`, a finished brief, an ADR/design/research/knowledge/testing/archive page, the
   changelog).
2. `docs/reference/manifest.md` is gone, no page links it, its inventory block is gone (the `grep -c`
   prints `0`; its exit status 1 is the pass) and `zensical.toml`'s `Reference` group lists
   `reference/cli.md`, the Blastfile reference, `reference/settings.md` and `reference/log-events.md`.
3. `uvx --offline zensical build --strict` exits 0 and every anchor the changed links point at
   resolves: record the clicked list in `RESULT.md` (`blastfile.md#stores`, `#traits`,
   `#view-directives`, `#grants`).
4. Every page that shows a complete manifest shows a `Blastfile`; page budgets hold (guides ≤ 150
   lines, framework pages ≤ 120, `README.md` within ±10 lines of its current length).
5. `make docs-unit` and `make doc-tone` exit 0; the deploy recipes are unchanged in behaviour — only
   the in-app manifest lines moved, both URLs and the pooler step still shown.

# Notes

- `plan.py` cannot rewrite a node body (`edit` changes frontmatter fields only). For a live brief,
  edit only the Markdown **below** the closing `---`: never hand-edit `status`, `attempts`, `updated`
  or any other managed field, and run `uv run --no-project tools/plan.py --agent doc-writer log <ID>
  "<one line saying what changed>"` afterwards.
- Delete, never annotate. A page must not say "this replaced", "previously", "the old file", or link
  to a page about the change; if a paragraph only made sense as a remark about the past, delete it.
- The retired name may survive in a record; cleaning a record is a defect. Do not touch `LOG.md`,
  `RESULT.md`, finished briefs, ADRs, design docs or the changelog.
- Prose rule: `python/tests/test_docs_assets.py` refuses a sentence that names the runtime word
  together with the last-resort static file, and it treats `route` as that runtime word. Read its
  docstring; keep language examples in fences.
- If a page needs a capability the language does not have, open a `human` request instead of inventing
  a directive.
