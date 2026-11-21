---
id: MB.21.1.1
title: Purge the retired manifest format from the Blastfile pages
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: []
paths: [docs/blastfile.md, docs/reference/blastfile.md, zensical.toml]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T01:49"
updated: "2026-11-16T01:55"
---

# Goal

Remove the last traces of the retired v1 manifest format from the two Blastfile pages. After this
node a reader of `docs/blastfile.md` or `docs/reference/blastfile.md` cannot learn that another
manifest format ever existed: no "coming from" line, no key map, no mention of a parser or a file
format by name, no apology, no comparison.

# Context

- Owner directive (2026-11-16): the v1 format is **buried**, under the same total-erasure law the
  retired PHP transport got (`MB.3.17`, `MB.3.30`). There is no migration story, no key map and no
  detector: Blastit reads `./Blastfile` (or `BLASTIT_MANIFEST`) and nothing else is ever mentioned.
- `docs/blastfile.md:96-97` today holds exactly one offending sentence, in two lines: it asks whether
  the reader is coming from the retired file and offers an anchor into the key map. Delete both lines
  **and** the blank line they leave behind, so the "Deployments ... same `Blastfile` runs in
  production" paragraph and the "Every keyword, directive and path pattern is in the [Blastfile
  reference]" paragraph join with one blank line between them. Nothing else in the guide changes.
  Find it with `grep -rni 'toml' docs/blastfile.md` (this brief deliberately never spells the name).
- `docs/reference/blastfile.md:590` is the `## Migration` heading; the section runs to end of file
  (line 631) and holds the key-map table, four "shape" bullets and a closing migration note. Delete
  from that heading to EOF, including the blank line before it, so the file ends with the last
  `## Errors` paragraph. Nothing in the deleted section is lost: `body raw` / `body raw unvalidated`
  are already documented in the `### View directives` table (lines 215-216) and in "A complete file";
  the dropped per-view concurrency cap is an `ADR-0012` design decision, not a user-facing mapping;
  the file being deleted by `MB.21.5.1` is never named again.
- `zensical.toml:44` holds `{ "The manifest (0.1.1, retired)" = "reference/manifest.md" }`. Delete
  that one row and leave the `Reference` group as `reference/cli.md`, the Blastfile reference,
  `reference/settings.md`, `reference/log-events.md` (still alphabetical, still coherent).
  A page that exists but is not in `nav` still builds under `--strict` — verified by the planner on
  this branch with the row removed: `uvx --offline zensical build --strict` printed
  "No issues found" — so dropping the row before the page is deleted is safe.
- **Do not touch `python/tests/runnable_docs/inventory.toml`,** whatever the directive sentence said:
  its `[[page]] path = "docs/reference/manifest.md"` block (lines 309-310) must survive while the page
  exists. `python/tests/runnable_docs/fences.py:230-251` lists every Markdown file under `docs/` that
  `exclude_docs` does not exclude, and `check_inventory` fails with "the published page … has no
  entry" once an entry goes missing. `MB.21.5.1` deletes the page and the block in one commit. Say
  this in `RESULT.md` as a deliberate deviation.

# Acceptance

```console
$ wc -l docs/blastfile.md docs/reference/blastfile.md      # record this before and after
$ grep -rni 'toml' docs/blastfile.md docs/reference/blastfile.md
$ grep -rni 'coming from\|key map\|#migration' docs/blastfile.md docs/reference/blastfile.md
$ grep -n 'reference/manifest.md' zensical.toml
$ make docs-check
$ make docs-unit
$ make doc-tone
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
```

1. `grep -rni 'toml' docs/blastfile.md docs/reference/blastfile.md` exits 1 with no output (before
   this node it prints the four lines of the sentence and the table).
2. The second grep exits 1: no "coming from", no "key map", no `#migration` anchor anywhere in either
   page.
3. `zensical.toml` no longer names `reference/manifest.md`, and the `Reference` group still lists
   `reference/cli.md`, `reference/blastfile.md`, `reference/settings.md`, `reference/log-events.md`.
4. `docs/blastfile.md` is ≤ 150 lines, `docs/reference/blastfile.md` loses only the 42 lines of the
   deleted section (± 1 blank line), and the before/after `wc -l` pair is in `RESULT.md`.
5. `make docs-check` (strict build), `make docs-unit`, `make doc-tone` and the assets pytest all exit
   0, and `git diff --stat` names none of `python/tests/runnable_docs/inventory.toml`,
   `docs/reference/manifest.md` or `docs/deploy/**`.

# Notes

- Write it as if no other format ever existed: no "previously", no "instead of", no "was called", no
  "we removed", no apology and no comparison. If a paragraph only makes sense as a remark about the
  past, delete the paragraph — do not rewrite it into a history sentence.
- The deliverable is a deletion. Do not add a replacement sentence, a note, or a link to a page that
  explains the change.
- `docs/design/**` and `docs/decisions/**` keep their provenance tables: they are design records and
  are excluded from the published site. Do not touch them.
- Only the retired format leaves these pages. Keep every other sentence, example, table and fence
  exactly as it is; `MB.21.5.1` owns the rest of the site.
