# Result MB.21.5.1

## Summary

The site reads as if `Blastfile` were the only manifest Blastit ever had. The retired reference
page is deleted with its inventory row, every link that pointed into it now points at
`reference/blastfile.md`, and the eight pages that carried a complete retired manifest carry a
`Blastfile` whose fence `docs_fences` parses and whose shape it pins.

`ARCHITECTURE.md` §5.1 is rewritten as **stores** (`postgres $DATABASE_URL`, split slots, no
`input`/`output` tables) and §6 as the `Blastfile` — views, traits, mounts, grants, table grants,
the report's `blastfile` snippet, `fold`, `unrestricted` in a view only, validation, routing and
per-sandbox uids — with one `blastfile` fence at the top and the mechanism prose kept verbatim
wherever it was not about spelling. `docs/VISION.md`, `AGENTS.md` and the four `.opencode` skills
and agents that named the file follow the same rename.

The five hand-offs from `MB.21.4` are closed: the eight pages, the `table_grants.blastfile` key in
the reference's `--json` section, `docs/ci.md`'s example and finding fixes (`fix.to` is
`view catalog postgres`, and a `connection` trait replaced the `groups` block), `docs/local.md`'s
prose (including the `{id:int}`/`{id:uuid}` naming and the per-view denial example), and the
reference's empty-column-list sentence (a table grant takes at least one column, so the renderer
prints advice, never `select <table>`).

## Evidence

Each `# Acceptance` item, on the final tree (working tree off `1f3ffc83`).

**1. No live page, `README.md`, `ARCHITECTURE.md` or live brief names the retired file.**

```console
$ grep -rnE 'blastit[.]toml' README.md docs ARCHITECTURE.md AGENTS.md zensical.toml \
    --include='*.md' --include='*.toml' \
    | grep -vE 'docs/changelog\.md|docs/(decisions|design|research|knowledge|testing|archive)/'
(no output; exit 1)

$ for f in $(grep -rlE 'blastit[.]toml' plan/nodes --include=NODE.md); do \
    printf '%s %s\n' "$(grep -m1 -oE 'status: [a-z]+' "$f" | cut -d' ' -f2)" "$f"; done \
    | grep -vE '^(done|superseded) '
(no output)
```

Every remaining hit in the tree is a record: `LOG.md`, `RESULT.md`, `docs/decisions/`,
`docs/design/`, `docs/research/`, `docs/knowledge/`, `docs/testing/`. The live-brief sweep touched
seven briefs (`MB`, `MB.3.9`, `MB.6`, `MB.9.1`, `MB.9.1.1`, `MC`, `ME`); `MB/NODE.md:57` now says
`PAGES` row rather than the retired `count == 6`, and `MB.6`'s `[static]` became a `static` mount.

**2. The retired page, its links and its inventory row are gone.**

```console
$ test ! -e docs/reference/manifest.md && echo gone
gone

$ grep -rn 'reference/manifest\.md' README.md docs ARCHITECTURE.md --include='*.md' \
    | grep -vE 'docs/(decisions|design|research|knowledge|testing|archive)/|docs/changelog\.md'
(no output; exit 1)          # the unfiltered command prints only records, listed below

$ grep -rl 'reference/manifest\.md' README.md docs ARCHITECTURE.md --include='*.md' \
    | sed 's|/[^/]*$||' | sort -u
docs/decisions
docs/design
docs/knowledge

$ grep -c 'reference/manifest.md' python/tests/runnable_docs/inventory.toml
0                            # exit 1 is the pass

$ ls docs/reference/
blastfile.md
cli.md
log-events.md
settings.md
```

`zensical.toml`'s `Reference` group lists `reference/cli.md`, the Blastfile reference,
`reference/settings.md` and `reference/log-events.md` — unchanged, so the deleted page was never in
the nav.

**3. `zensical build --strict` is clean and every repointed anchor resolves.**

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.26s
```

`invalid_link_anchors = true` is what proves the list: `blastfile.md#stores` (`## Stores`),
`#traits` (`## Traits`), `#view-directives` (`### View directives`), `#grants` (`### Grants`),
`frameworks/django.md#manifest` (the `{#manifest}` anchor both deploy recipes link),
`ci.md#{what-it-fails-on,for-agents-json,exit-codes,your-suite,bootstrap-an-existing-project}`,
`local.md#a-denial`, `local.md#fill-the-grants-in-one-pass` (renamed; no page cites the old
anchor), `frameworks/index.md#bind-parameters-server-side`, `deploy/anything-else.md#pooler`,
`deploy/anything-else.md#root-or-rootless`, `deploy/compose-traefik.md#rootless`,
`security.md#the-mechanisms`.

**4. Every complete manifest on the site is a `Blastfile`, and the budgets hold.**

```console
$ timeout 300 cargo test -p blastit-manifest --test docs_fences
running 6 tests
test every_documented_document_parses_and_validates ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

$ wc -l README.md docs/blastfile.md docs/getting-started.md docs/local.md
   74 README.md
   98 docs/blastfile.md
   66 docs/getting-started.md
  144 docs/local.md

$ wc -l docs/frameworks/*.md
  110 docs/frameworks/django.md
  110 docs/frameworks/flask.md
  112 docs/frameworks/index.md
  110 docs/frameworks/php.md
  114 docs/frameworks/static-and-spa.md
```

Ten pages carry one `text title="Blastfile"` document each and no other complete manifest:
`PAGES` pins `[1, 0, 0]` for each and `TOTAL` is `[10, 0, 0]`. The shapes are pinned per page —
`Application` for the eight app pages, `StaticSite` for `static-and-spa.md`, `ViewsAndMount` for
`php.md` (views plus `static "/" { root "public" }`) — so a page that silently degraded from views
to a mount fails there. `README.md` is 74 lines against 75 before (±10 holds); guides are ≤ 150,
framework pages ≤ 120, and `docs/blastfile.md` is 98.

**5. `make docs-test` executes every fence; `docs-unit`, `doc-tone`, `docs_assets` are green.**

```console
$ make docs-test
PASS  README.md  fence 2  dev                   0.1s  served on 127.0.0.1:8080
PASS  docs/local.md  fence 3  page-manifest         0.1s  the page's `postgres {}` view answered 500 with a `sql.denied` line and SQLSTATE 42501
PASS  docs/ci.md  fence 2  findings              1.5s  six documented findings, one line each
PASS  docs/ci.md  fence 3  json                  1.5s  one JSON document with 6 findings
PASS  docs/frameworks/static-and-spa.md  fence 3  dev-curl              2.1s  served and answered
PASS  docs/reference/cli.md  fence 4  usage                 0.1s  the documented fold signature exists
49 fences: 28 executed, 21 accounted, 0 failed · 30.6s

$ make docs-unit
96 passed, 65 subtests passed in 2.01s

$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
24 passed in 0.51s

$ make doc-tone
doc-tone: ok — 27 user-facing page(s), no banned phrase
```

No `console` fence was added, removed or reordered, so the inventory kept its 49 entries; the only
inventory edit after the sweep is the `docs/frameworks/php.md` fence-2 reason, which now says
"an `entrypoint php` file" and "the page's Blastfile block" instead of the retired key. The two
pages touched after the full run were re-run alone: `docs/local.md` 6 fences (5 executed, 1
accounted, 0 failed) and `docs/frameworks/php.md` 3 fences (1 executed, 2 accounted, 0 failed).

Deploy behaviour is unchanged: both recipes still show the pool URL and the direct URL, the
`SHOW TimeZone` read-back and the pooler step, and each now says in one sentence that a pooler is a
deployment concern and never a `Blastfile` directive.

## Files

- **Deleted:** `docs/reference/manifest.md`; its `[[page]]` block in
  `python/tests/runnable_docs/inventory.toml`.
- **Rewritten to teach `Blastfile` only:** `README.md`, `docs/index.md`,
  `docs/getting-started.md`, `docs/local.md`, `docs/ci.md`,
  `docs/frameworks/{index,django,flask,php,static-and-spa}.md`, `docs/reference/cli.md`,
  `docs/reference/settings.md`, `docs/troubleshooting.md`, `docs/how-it-works.md`,
  `docs/security.md`, `docs/deploy/{index,digitalocean-paas,compose-traefik,anything-else}.md`,
  `docs/VISION.md`, `ARCHITECTURE.md` (§5.1, §5.2/§5.3 prose, §6, §2.4/§2.6 prose, the §8 settings
  table), `AGENTS.md`.
- **Skill and agent guidance (name only, everything else identical):** `.opencode/agents/{architect,
  critic,doc-writer,implementer}.md`, `.opencode/skills/{design-docs,dx-first,security-invariants,
  testing}/SKILL.md`.
- **Hand-offs closed in the reference:** `docs/reference/blastfile.md` — the empty-column-list
  sentence under Grants, and the `table_grants.blastfile` paragraph in the `--json` section.
- **Live briefs:** `plan/nodes/MB/NODE.md`, `MB/MB.3/MB.3.9/NODE.md`, `MB/MB.6/NODE.md`,
  `MB/MB.9/MB.9.1/NODE.md`, `MB/MB.9/MB.9.1/MB.9.1.1/NODE.md`, `MC/NODE.md`, `ME/NODE.md` — Markdown
  bodies only, below the closing `---`.

## Out-of-paths edits

`crates/blastit-manifest/tests/docs_fences.rs` — now inside `paths`, and it had to move with the
pages or stay red.

It is the pages' fence inventory, not product code: `PAGES` pins one `Counts` per page in three
columns (`text title="Blastfile"`, the `blastfile` alias, the retired opening) and `TOTAL` pins the
sum. The moment the eight pages stopped carrying a retired fence, `every_documented_document_parses_
and_validates` failed twice — once on the per-page counts and once on a page with an executed
document that no `Shape` arm covered. The file's own header reserves exactly this edit for this
node ("`MB.21.5.1` moves the eight retired rows to `Blastfile` documents, `MB.21.5.2` deletes the
retired column"), so the change is:

1. the eight rows' counts move from column 2 to column 0, and `TOTAL` becomes `[10, 0, 0]`;
2. `Shape` gains `ViewsAndMount` (views **and** a mount) and `shape_of` returns it for `php.md`,
   whose document carries `static "/" { root "public" }` beside its views;
3. the per-page `expected` stays an exhaustive match with a `panic!` arm — no catch-all — listing
   the eight `Application` pages explicitly, so a page that gains a document still has to pin its
   shape.

`THE_RETIRED_OPENING_IS_GONE` stays `false` and the retired opening keeps its counter and reader:
the strict flip, the exhaustive `docs/**` scan and the retired-name guard are `MB.21.5.2`'s, and
nothing here narrows them. Nothing else under `crates/` was touched, and nothing under
`python/blastit/`, `e2e/` or `scripts/`.

## Follow-ups

### Records deliberately left alone (never cleaned, by design)

`plan/**/LOG.md`, every `RESULT.md`, briefs whose status is `done` or `superseded`,
`docs/decisions/**` (notably `ADR-0002`, `ADR-0009`, `ADR-0012`), `docs/design/**` (the `ma-lean-dx`
and `blastfile-dsl` specs and every `review-*.md`), `docs/research/**`, `docs/knowledge/LESSONS.md`,
`docs/testing/**` and `docs/changelog.md`. They are the reason the retired name can still be
recognised at all; `docs/decisions/`, `docs/design/` and `docs/knowledge/LESSONS.md` are also why
`grep -rn 'reference/manifest\.md' docs` still prints — the page's citations there are history, and
`MB.21.5.2`'s anti-vacuity test needs a record that still names it.

### Out of scope, recorded for a reader

1. `docs-site-plan.md` (repository root) still lists `reference/manifest.md` in its page table and
   its FAQ. It is a working plan, not a published page, it is outside this node's `paths`, and
   `MB.21.5.2`'s guard does not scan it. If the file is kept, a node that owns the repository root
   should either retire it or mark it a record.
2. `docs/index.md`'s closing admonition still says "0.1.1 is the first developer-experience release"
   on a 0.2.0 tree. Untouched here because it is a release-note sentence, not a manifest one.
3. `ARCHITECTURE.md` keeps the word "manifest" for the concept (`BLASTIT_MANIFEST`,
   `blastit-manifest`, the `manifest.warning` event, the report's `proxy` key). That is the
   product's own vocabulary, not the retired format, so it stays; only the file name and the
   TOML spellings moved.
4. `crates/blastit-manifest/tests/docs_fences.rs` still carries `RETIRED_OPENING`, `FRAGMENT_OPENING`
   and the retired counter, now counting zero. Deleting them is `MB.21.5.2` step 2.
5. **Retired manifest syntax in Rust doc comments** — out of this node's `paths`, so the lead needs
   an owner: `crates/blastit-ingress/src/lib.rs:4,16,85,99,101,108` and
   `crates/blastit-dbproxy/tests/postgres.rs:673` describe their inputs with `[route.ingress]`,
   `[route.db]` and `body = "raw"` + `allow_raw = true`. They are comments, never rendered to a
   reader, but they name the retired keys; the node that next owns those files should re-spell them
   as `body` and a view's `postgres { }` block.
6. **Does `ARCHITECTURE.md` §6 belong in `docs_fences`' `PAGES`?** It carries one `blastfile` fence
   that nothing parses: `PAGES` covers the ten published pages, and `ARCHITECTURE.md` lives outside
   `docs_dir`, so `make docs-check` never reads it either. `MB.21.5.2` owns that file and should
   decide whether §6's example joins `PAGES` with a pinned shape (the fence already parses and
   validates today — it is the same document the guide ships), or whether the specification keeps
   its own gate.

### For the reviewer

The only judgement calls worth a second pair of eyes are prose, not mechanism: `ARCHITECTURE.md`
§6's `Blastfile` example is one rendering of a shape the reference already fixes (a catch-all
`view fallback "/*" {}`, and a PHP file whose `public/` is a mount), and `docs/frameworks/php.md`
dropped the `workers` sentence because a `Blastfile` has no per-entrypoint concurrency knob —
`BLASTIT_MAX_WORKERS` replaced it.

The first of those is **not** machine-checked here, and the earlier wording said it was: `docs_fences`
parses and validates the ten published pages in `PAGES`, and `ARCHITECTURE.md` is not one of them —
it sits outside `docs_dir`, so `make docs-check` does not read it either. The §6 example does parse
and validate when run by hand (it is the same shape the guide ships), but nothing pins it, which is
why Follow-ups 6 asks `MB.21.5.2` to decide whether §6 joins `PAGES`. `docs/frameworks/php.md` is
covered: it is in `PAGES` and `make docs-test` executes its fences; neither page invents a directive.

## Review Resolution

- **reviewer MAJOR → fixed → `ARCHITECTURE.md:223`.** "`authorize` checks the statement against the
  route's effective grants: the union of its inline grants, its groups and the global groups (§6)"
  became "… the view's effective grants: the union of its own grants and those of every trait it
  `use`s (§6)". A `Blastfile` has no group model, so the two halves of the file no longer disagree.
- **reviewer MAJOR → fixed → `ARCHITECTURE.md:708`.** The §3.2 sequence diagram's
  `alt granted to the route (inline, its groups or global groups)` became
  `alt granted to the view (its own grants, or a trait it uses)`. The whole file was then swept for
  the same three phrasings and for `groups`: every survivor is an OS group, a capability group, a
  cgroup, a process group, `setgroups`/`--init-groups`, or the report's `unused_global` JSON key
  (which §6 explains), at lines 36, 127, 814, 828, 1335, 1527, 1917, 1918 and 1177.
  `grep -n 'global groups\|its groups\|inline grants' ARCHITECTURE.md docs/*.md docs/reference/*.md`
  prints nothing, and `make docs-check` still reports `No issues found`.
- **reviewer MINOR → fixed → `plan/nodes/MB/NODE.md:73`.** The empty-grant job now reads "add an
  `e2e/` fixture whose view has an empty `postgres {}` grant (the `42501` denying listener)".
  Sweeping the same brief found two more: line 20's PHP backend now names
  `entrypoint php "public/index.php"` (a `Blastfile` has no `workers` cap), and line 78's
  connection-setup hint now asks for `add_group` whose `to` names `trait connection` instead of
  `[group.<name>.db]`.

### Pass 2 — whole-file audit of `ARCHITECTURE.md`

Eight passages — fifteen token hits — were retired-model; all are fixed. Every remaining hit of the
same tokens is justified below and in the audit table in the reply.

- **reviewer pass 2 MAJOR → fixed → `ARCHITECTURE.md:277-281`.** The scheduler's routing bullet
  said "each route's path regexes … tried in manifest order … A catch-all route (`path = ".*"`,
  listed last)". It now says "each view's patterns … tried in file order … A catch-all view (`view
  fallback "/*" {}`, listed last)".
- **reviewer pass 2 MAJOR → fixed → `ARCHITECTURE.md:464-476`.** §2.6's PHP paragraph carried
  `1..=workers`, `max_requests` recycling and "`workers` (1–64) can only lower the cap blastit
  derives" — a knob the parser refuses (`WorkersRemoved`). It now describes the scheduler's own
  budget: one process per slot, a fresh process when the scheduler allows one,
  `BLASTIT_MAX_WORKERS` for the app and `BLASTIT_VIEW_MAX_WORKERS` per view, and "nothing in the
  `Blastfile` sets either".
- **reviewer pass 2 MINOR → fixed → `ARCHITECTURE.md:78`.** The `blastit check` row now says
  "`--learn` proposes new views with their exact grants" — the `proposed.queries` key stays in the
  `--learn --json` document, so the page stops reading as if `queries` were a directive.
- **Same-family siblings fixed in the same pass:** `:574` "each proxy's inputs … (route, proxy)
  table, the manifest's grants" → "each store's inputs … (view, store) table, the file's grants";
  `:1000` "The manifest syntax doesn't change" → "The `Blastfile` syntax doesn't change" (the
  sentence wraps onto `:1001`); `:1087`
  the example's `# inline only: no trait involved` → `# the view's own block: no trait involved`;
  `:1154` "moved from exact queries to table grants" → "from exact `query` lines to table grants";
  `:1753` "the manifest's proxies" → "the file's stores".

```console
$ grep -nE 'workers|max_requests|path =|\[route\.|\[route\]|\[proxy\.|\[group\.|allow_raw|body = |\bspa\b|version = |toml|queries = |tables =|\[\[route\]\]|\.toml' ARCHITECTURE.md
1438:  `fork`, `vfork` and a `clone` without `CLONE_THREAD` with `EPERM`. Its workers
1602:| `process.spawned` / `process.exited` / `process.retired` | … `reason` (`idle`, `max_requests`, …) |
1813:   cost of each process.** On Linux, 100 forked workers of a Django +

$ grep -rnE 'workers|max_requests|path = "|allow_raw|\[route\.|\[group\.' ARCHITECTURE.md docs/**/*.md README.md \
    | grep -vE 'docs/(decisions|design|research|knowledge|testing|archive)/'
ARCHITECTURE.md:1438 · ARCHITECTURE.md:1602 · ARCHITECTURE.md:1813
docs/deploy/index.md:24 · docs/deploy/index.md:74 · docs/reference/settings.md:159      # sizing prose: request workers
docs/reference/blastfile.md:437 · :472                                                    # the report's `fix.allow_raw` key
docs/reference/log-events.md:33                                                           # the `process.retired` reason
docs/VISION.md:20 · :33 · :47                                                             # "route workers" / "view workers": OS processes
```

Three hits in `ARCHITECTURE.md` and nine in `docs/` survive, all product vocabulary: worker
processes counted for sizing (`docs/VISION.md`'s "route workers" and "view workers" are OS
processes too), the `process.retired` event's `max_requests` reason, and the report's
`fix.allow_raw` key. No `path = "`, `[route.`, `[group.`, `[[route]]`, `version =`, `toml`,
`queries =`, `tables =`, `body = ` or `spa` remains in either tree.

```console
$ make docs-check       → No issues found (exit 0)
$ make docs-unit        → 96 passed, 65 subtests passed
$ make doc-tone         → doc-tone: ok — 27 user-facing page(s), no banned phrase
$ make docs-test        → 49 fences: 28 executed, 21 accounted, 0 failed · 21.9s (exit 0)
$ pytest python/tests/test_docs_assets.py -q → 24 passed
```

### Pass 3 — five MINORs from the audit-closure review

- **reviewer pass 2 MINOR → fixed → `ARCHITECTURE.md:572`.** Row ④ of the §3.1 table now reads
  "named Unix sockets, one per (view, store) with a grant", so the table agrees with row ⑤ and the
  `secrets.pipe` row below it. `route` stays the vocabulary elsewhere, as `:432-433` defines it.
- **reviewer pass 2 MINOR → fixed → this file, `:574`, `:1000`, `:1087`.** The three citations were
  re-derived from `ARCHITECTURE.md`: the `(view, store)` row is `:574` (not `:571`), "The
  `Blastfile` syntax doesn't change" starts at `:1000` and wraps onto `:1001` (not `:997`), and the
  example's comment is `:1087` (not `:1084`).
- **reviewer pass 2 MINOR → fixed → the docs-hit list above.** `docs/VISION.md:20,33,47` are added
  with the OS-process verdict ("route workers" / "view workers", the same sense as
  `ARCHITECTURE.md:1438,1813`), so the list is complete at nine `docs/` hits and three in
  `ARCHITECTURE.md`.
- **reviewer pass 2 MINOR → fixed → "For the reviewer".** The false claim that `docs_fences` checks
  §6 is gone: `PAGES` covers the ten published pages, `ARCHITECTURE.md` is outside `docs_dir`, and
  the note now says so plainly and points at Follow-ups 6.
- **reviewer pass 2 MINOR → fixed → Follow-ups 5.** `crates/blastit-ingress/src/lib.rs:108` joins the
  doc-comment list, with `body = "raw"` + `allow_raw = true` named as what it documents.
