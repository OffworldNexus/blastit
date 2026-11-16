# MB.21.1.1 — Purge the retired manifest format from the Blastfile pages

**Status:** done (attempt 1)

## Summary

Three deletions, nothing else. Both live pages now read as if no other manifest file format
ever existed: no "coming from" sentence, no key map, no migration anchor, no nav row and no
mention of another format's name (or the format name in parentheses) anywhere in either page.

- `docs/blastfile.md`: dropped the three lines that asked the reader whether they arrive from
  the retired file and offered the key map, plus the blank line they left, so the
  "Deployments … the same `Blastfile` runs in production" paragraph and "Every keyword,
  directive and path pattern is in the [Blastfile reference]" paragraph now join with one
  blank line.
- `docs/reference/blastfile.md`: dropped the whole `## Migration` section — heading, key-map
  table, four shape bullets and the closing note — from the blank line before the heading to
  EOF. The page now ends on the last `## Errors` paragraph (the `Blastfile:3:9` example
  fence). Nothing was lost: `body raw` / `body raw unvalidated` remain in the
  `### View directives` table (`:215-216`), and the dropped per-view concurrency cap is an
  `ADR-0012` decision, not a user-facing mapping.
- `zensical.toml`: dropped the one `nav` row for the retired page. The `Reference` group is
  still alphabetical and coherent: `reference/cli.md`, the Blastfile reference,
  `reference/settings.md`, `reference/log-events.md`.

Zero added lines: `git diff --stat` on the three files is `3 files changed, 47 deletions(-)`.

## Evidence

### 0. Line counts, before → after

```console
$ wc -l docs/blastfile.md docs/reference/blastfile.md     # BEFORE
  101 docs/blastfile.md
  631 docs/reference/blastfile.md
  732 total

$ wc -l docs/blastfile.md docs/reference/blastfile.md     # AFTER
   98 docs/blastfile.md
  588 docs/reference/blastfile.md
  686 total
```

`docs/blastfile.md` 101 → 98 (≤ 150); `docs/reference/blastfile.md` 631 → 588, i.e. exactly
the 42 lines of the deleted section plus the one blank line before it.

### 1. No mention of the retired format (was: 4 matching lines)

```console
$ grep -rni 'toml' docs/blastfile.md docs/reference/blastfile.md
exit=1
```

Before (recorded for the record):

```console
$ grep -rni 'toml' docs/blastfile.md docs/reference/blastfile.md
docs/blastfile.md:97:Coming from `blastit.toml`? The [key map](reference/blastfile.md#migration) is
docs/reference/blastfile.md:592:A `blastit.toml` — or any TOML — handed to any Blastit command
docs/reference/blastfile.md:593:(`BLASTIT_MANIFEST=blastit.toml blastit dev`) fails with one sentence naming
docs/reference/blastfile.md:597:| `blastit.toml` | `Blastfile` |
```

### 2. No "coming from", no key map, no `#migration` anchor

```console
$ grep -rni 'coming from\|key map\|#migration' docs/blastfile.md docs/reference/blastfile.md
exit=1
```

### 3. The site nav no longer names the retired page

```console
$ grep -n 'reference/manifest.md' zensical.toml
exit=1
```

The group itself:

```console
$ sed -n '41,47p' zensical.toml
  { "Reference" = [
    "reference/cli.md",
    { "Blastfile reference" = "reference/blastfile.md" },
    { "Settings" = "reference/settings.md" },
    { "Log events" = "reference/log-events.md" },
  ] },
```

### 4. Strict build, docs unit suite, tone gate, assets pytest

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 1.69s
docs-check exit=0

$ make docs-unit
...
96 passed, 65 subtests passed in 1.98s
docs-unit exit=0

$ make doc-tone
uv run --no-project scripts/check_doc_tone.py
doc-tone: ok — 28 user-facing page(s), no banned phrase
doc-tone exit=0

$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
........................                                                 [100%]
24 passed in 0.48s
assets exit=0
```

A page that exists but has no `nav` row still builds under `--strict`: "No issues found".

### 5. Scope of the diff

```console
$ git diff --stat -- docs/blastfile.md docs/reference/blastfile.md zensical.toml
 docs/blastfile.md           |  3 ---
 docs/reference/blastfile.md | 43 -------------------------------------------
 zensical.toml               |  1 -
 3 files changed, 47 deletions(-)

$ git diff --name-only | grep -E 'python/tests/runnable_docs/inventory.toml|docs/reference/manifest.md|docs/deploy/'
forbidden-grep exit=1
```

The deletions-only diff of the reference:

```console
$ git diff -- docs/reference/blastfile.md | head -4
@@ -586,46 +586,3 @@ view and the fix instead.
 ```text
 Blastfile:3:9: `=` is not Blastfile syntax; write the value after the keyword
 ```
-...
```

Nothing that was deleted is needed: `body raw` / `body raw unvalidated` still live in the
`### View directives` table:

```console
$ sed -n '215,216p' docs/reference/blastfile.md
| `body raw` | the bytes unchanged — the loud opt-out; `--learn` never writes it; `blastit dev` serves it with a warning, `check` reports `raw_ingress`, and a production boot refuses the file |
| `body raw unvalidated` | the same bytes, acknowledged: production serves it with a boot warning, and the app's parser receives them unchanged |
```

### 6. Reviewer MAJOR → fixed

The reviewer failed attempt 1 on one MAJOR: the traits table still carried
`| v1's global `groups` key | gone: every attach is written on the view |` at
`docs/reference/blastfile.md:338`, which named another format and said one of its keys was
"gone". That row was deleted; the two rule rows above it remain intact and the table is still
well-formed. Reference 588 → 587 lines (88 deletions total across the two pages and the nav).

```console
$ grep -n 'gone:\|v1' docs/reference/blastfile.md docs/blastfile.md
exit=1

$ wc -l docs/blastfile.md docs/reference/blastfile.md
   98 docs/blastfile.md
  587 docs/reference/blastfile.md
  685 total

$ make docs-check   → No issues found, exit 0
$ make docs-unit    → 96 passed, 65 subtests passed, exit 0
$ make doc-tone     → doc-tone: ok — 28 user-facing page(s), no banned phrase, exit 0
$ pytest python/tests/test_docs_assets.py -q → 24 passed, exit 0
```

## Files

- `docs/blastfile.md` — 3 lines deleted.
- `docs/reference/blastfile.md` — 44 lines deleted: the blank line before `## Migration`
  through EOF, plus the one traits-table row naming another format.
- `zensical.toml` — 1 nav row deleted.

## Deviations

- **`python/tests/runnable_docs/inventory.toml` deliberately untouched.** Its
  `[[page]] path = "docs/reference/manifest.md"` block must survive while that page exists:
  `python/tests/runnable_docs/fences.py` lists every Markdown file under `docs/` that
  `exclude_docs` does not exclude, and `check_inventory` fails with "the published page … has
  no entry" the moment the entry goes missing. The page and its inventory block are both
  deleted together by `MB.21.5.1`; dropping the block here would turn `make docs-unit` red.
  `make docs-unit` is green above with the block still in place.
- No replacement sentence, note, apology, comparison or link explaining the change was added
  anywhere; the deliverable is a deletion only.
- `docs/design/**`, `docs/decisions/**` and `docs/reference/manifest.md` were not touched.
  The working tree also carries uncommitted changes from other in-flight nodes (`plan/**`,
  `docs/decisions/README.md`); none of them are mine and none are staged by me.

## Follow-ups

- `MB.21.5.1` must delete `docs/reference/manifest.md` and its `[[page]]` block in
  `python/tests/runnable_docs/inventory.toml` in one commit, and point any remaining inbound
  link at the Blastfile pages.
- `MB.21.5.2` owns the permanent `make check` guard
  (`test_the_real_tree_carries_no_retired_toml_manifest`) so the retired name cannot return to
  a page, crate, script or live brief.
- `MB.21.5.2`'s repo-wide guard must also refuse `v1` and `gone:` in the published pages, not
  only the retired file name — a "gone: <retired key>" row leaks the previous format without
  ever naming it.
- Out of scope here and still to sweep in `MB.21.5.1`: every other page that names the
  retired file (frameworks, deploy, local development) — the two Blastfile pages are the only
  ones this node owns.
