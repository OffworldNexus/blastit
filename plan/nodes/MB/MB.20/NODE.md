---
id: MB.20
title: "Regroup /frameworks by connector: python/ and php/ groups over the nine slots (docs-only)"
kind: task
status: ready
owner: "doc-writer"
priority: 2
depends_on: [MB.3.65, MB.21.5.1]
paths:
  - "docs/frameworks/**"
  - "docs/design/frameworks-restructure*"
  - "docs/getting-started.md"
  - docs/troubleshooting.md
  - "docs/deploy/compose-traefik.md"
  - "docs/deploy/digitalocean-paas.md"
  - docs/reference/manifest.md
  - zensical.toml
  - python/tests/runnable_docs/inventory.toml
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T21:50"
updated: "2026-11-16T00:38"
---

# Goal

Restructure `docs/frameworks/` from one flat list of five peer pages into **groups by
connector**, with **one group index per runtime** and the **framework pages inside** it.
Owner's direction (2026-11-15):

> "it is rather flat … it should better be organized by language/connector type and then
> inside explain framework by framework what are the considerations."

`MB.19`'s nine-slot contract stays exactly as it is on framework pages. What changes is the
level above it: a reader picks a **runtime** first, learns **once** how Blastit hands a
request to that runtime, and then opens a framework page that answers only that framework's
part of the same nine slots.

# Context

- Parent: **`MB.19`** (kind `task`, `done`) built the one-intro-plus-nine-slots shape, the
  `<= 120`-line page budget and the canonical manifest shape. It is a task, not a container,
  so this node is filed under **`MB`** with `MB.19` as its parent in the plan.
- **Measured today** (`release/0.2.0`): `docs/frameworks/` is flat —
  `index.md` (117), `django.md` (118), `flask.md` (115), `php.md` (117),
  `static-and-spa.md` (118) — and `zensical.toml:34-38` lists them as four peer entries plus
  the index. Every content page uses the same nine headings (`## 1. Executor` …
  `## 9. Deployment notes`), so each page re-explains its runtime's mechanics and the reader
  cannot tell which sentences are about Python, which about PHP, and which about Blastit
  itself.

## Target tree (this is the deliverable)

```
docs/frameworks/index.md            Choose your runtime — three groups + static; no repeated mechanics
docs/frameworks/python/index.md     The Python connector (WSGI/ASGI): process model, what Blastit guarantees
docs/frameworks/python/django.md
docs/frameworks/python/flask.md     Flask + SQLAlchemy + raw WSGI
docs/frameworks/php/index.md        The PHP connector: embedded libphp SAPI, request-per-call, no pool
docs/frameworks/php/symfony.md
docs/frameworks/php/laravel.md      split out of today's shared php.md
docs/frameworks/static-and-spa.md   unchanged position (no runtime)
```

- `django.md` and `flask.md` move **unchanged in substance** (plus the de-duplication in
  acceptance 3); only their path and their cross-links change.
- `php.md` is **split for real**: the connector half becomes `php/index.md`, and Symfony and
  Laravel advice must genuinely differ — container warmup (`bin/console cache:warmup`, a
  pre-warmed `var/cache`, `APP_ENV=prod`) versus Artisan config/route/view caching
  (`php artisan config:cache|route:cache|view:cache`, a writeable `bootstrap/cache`) are the
  obvious split points, alongside `doctrine:migrations:migrate` versus `php artisan migrate`.
- The product statement **"We support supported PHP versions, which are 8.3 and 8.4."** stays
  verbatim, exactly one sentence, and lives on `php/index.md` only (it is runtime-level).
  Keep `MB.3.65`'s Alpine unversioned-`libphp` name guidance there too.
- The generic nine-slot answers that today sit on `index.md` keep **one** home each:
  the canonical manifest/ingress/grant/static/PHP text in `docs/reference/manifest.md`, or the
  language index, or a framework page. Nothing may evaporate on the way.

## Inbound references measured (all must be repointed)

| Where | What it points at | New target |
|---|---|---|
| `zensical.toml:34-38` | four flat peer entries | three groups, framework children |
| `docs/getting-started.md:67` | `frameworks/django.md`, `frameworks/flask.md` | `frameworks/python/…` |
| `docs/deploy/compose-traefik.md:169` | `../frameworks/django.md#manifest` | `../frameworks/python/django.md#manifest` |
| `docs/deploy/digitalocean-paas.md:64` | `../frameworks/django.md#manifest` | `../frameworks/python/django.md#manifest` |
| `docs/troubleshooting.md:70` | `frameworks/index.md#bind-parameters-server-side` | wherever the "parameters apart from text" rule now lives |
| `docs/reference/manifest.md:129` | `../frameworks/static-and-spa.md` | unchanged (page keeps its path) |

Anchors that must survive the moves: `django.md:6` `## 1. Executor { #manifest }` and
`index.md:55` `## 3. Database { #bind-parameters-server-side }`. `MB.19`'s visual-checker
verified those three jumps land; this node must keep that true.

## Fence inventory measured

`python/tests/runnable_docs/inventory.toml:202-240` registers fences **per page path and
fence index**: `index.md` 0, `django.md` 1 (`check`, fixture `shop`), `flask.md` 1
(`install-external`), `php.md` 3 (`static`, `static`, `presets`), `static-and-spa.md` 4.
`python/tests/runnable_docs/run.py` checks the inventory **before** any fence runs: a
published page or a console fence it does not name fails `make docs-test`. Splitting and
re-indexing therefore rewrites those entries, with the same modes (and the same written
`reason` for every mode that means "not executed").

## Sequencing (hard)

- `depends_on: [MB.3.65]`. `MB.3.65` owns `docs/frameworks/php.md` on `release/0.2.0` (PHP 8.2
  descope) and rewrites the 8.2 paragraph into the single support statement. Two writers on
  that file is how a red gets attributed to the wrong change.
- **Do not execute this node until v0.2.0 is merged into `develop`.** Start from
  `develop` after the merge, not from `release/0.2.0`, so the doc-writer never races a release
  fix-up. `lead` is expected to hold it in `ready` until then.
- **Priority 2: not release-blocking.** A red here never holds the release.

## Scope supersession

This node **supersedes `MB.19` acceptance 4** (one capability matrix framework × executor ×
static × DB). The matrix is exactly the flat organization the owner is retiring: it is
replaced by one line per group on the root index, and each group's differences live on its
own pages. Say so in `RESULT.md` so a reviewer does not read the deletion as a regression.

# Acceptance

```console
$ make docs-check
$ make docs-unit
$ make docs-test
$ wc -l docs/frameworks/index.md docs/frameworks/python/*.md docs/frameworks/php/*.md docs/frameworks/static-and-spa.md
$ grep -rn 'coming soon\|for now\|currently only\|does not yet\|we are sorry' docs/frameworks/
$ uv run --no-project tools/plan.py validate
```

1. **The target tree exists exactly as specified** — the six old flat files are gone from their
   old paths, `docs/frameworks/{index.md, static-and-spa.md}` remain, `python/` holds
   `index.md`, `django.md`, `flask.md`, and `php/` holds `index.md`, `symfony.md`, `laravel.md`;
   `git status --short docs/frameworks` shows only additions/renames, no orphan page. Each
   group directory is reachable from `docs/frameworks/index.md` in one click.

2. **Each language index answers exactly one question — *how does Blastit hand a request to
   this runtime?* — in `<= 40` lines** (`python/index.md`: WSGI callable, one worker process
   per route, what Blastit guarantees around it; `php/index.md`: embedded host NTS `libphp`,
   one request at a time inside the view worker with no separate server process or socket,
   `public/index.php` as the only entry, the support statement). It states **only** what is
   identical across that language's frameworks — no `collectstatic`-vs-Flask-static advice, no
   Artisan-vs-Symfony advice, no framework-specific manifest example (a `[route.php]`/`app =`
   stub of at most ten lines is allowed to show the executor key, nothing more).

3. **Every framework page keeps the same nine headings, in the same order, and says only that
   framework's answers** (`## 1. Executor`, `## 2. Static assets`, `## 3. Database`,
   `## 4. Ingress`, `## 5. Environment and secrets`, `## 6. Health`, `## 7. Migrations`,
   `## 8. Learn and check`, `## 9. Deployment notes`; `<= 120` lines each). It **must not
   restate** runtime mechanics the language index already states. Reviewer evidence: run this
   line-level duplicate check and record the output; it must print nothing, and a reworded
   restatement is the reviewer's call, recorded in `RESULT.md`:

   ```console
   $ for f in docs/frameworks/python/*.md docs/frameworks/php/*.md docs/frameworks/static-and-spa.md; do \
       grep -E '^[A-Za-z`].{80,}' "$f" | sed "s|^|$f: |"; done | cut -d: -f3- | sort | uniq -d
   ```

   Run the same check between each language index and its two children.

4. **`docs/frameworks/index.md` is a chooser in `<= 60` lines**: one line per group naming its
   runtime and the pages inside it, one line for static, and the nine-slot legend reduced to
   the **question** each slot answers (one sentence each, no answers). It carries **no**
   capability matrix and **no** runtime mechanics — the matrix is deleted on purpose (see
   Scope supersession above).

5. **The PHP split is two pages, not one page renamed.** `php/index.md` is the connector;
   `php/symfony.md` and `php/laravel.md` disagree where the frameworks disagree (warmup/cache
   story, migration command, writable directories, worker/queue note) and each keeps all nine
   headings with its own answers. Neither child restates the connector's `libphp` mechanics.
   The support statement appears **once**, verbatim, on `php/index.md`:
   `grep -rn 'We support supported PHP versions, which are 8.3 and 8.4.' docs/frameworks/php/`
   prints exactly one hit.

6. **Navigation and references**: `zensical.toml` gains the three groups with their children
   (Python, PHP, Static and SPAs), every page is reachable and none is orphaned
   (`make docs-check` is `--strict`, so a nav entry pointing at a missing file or a page
   outside the nav fails), and every inbound reference in the measured table above is
   repointed. `#manifest` and `#bind-parameters-server-side` still resolve (kept where they
   now belong, or the link is repointed to the page that carries the rule). `visual-checker`
   PASSes the new tree in light, dark and 390×844: the group drawer opens, all three bridged
   anchors land, no raw pipes or clipped fences.

7. **DX-first order, provable on disk**: before any page prose was written, `doc-writer` wrote
   `docs/design/frameworks-restructure.md` — page → headings → the one sentence each section
   will say — and two **goldfish** critics (`dx` and `ai-ergonomics`, fresh context, brief:
   "can I get from 'I use Django/Laravel' to the sentence that answers me in two clicks, and
   does any sentence appear twice?") wrote `docs/design/frameworks-restructure.review-dx.md`
   and `…review-ai-ergonomics.md`, each ending `VERDICT: PASS`. Evidence:
   `git log --oneline -- docs/design/frameworks-restructure* docs/frameworks` shows the
   outline and both reviews in an earlier commit than the first page, and `LOG.md` records both
   verdicts before the pages were written. No page was written while an outline finding was
   open.

8. **Gates and language**: `make docs-check` (strict build), `make docs-unit`, `make docs-test`
   are green, with every new/changed page and console fence registered per page path and fence
   index in `python/tests/runnable_docs/inventory.toml` (same mode and written `reason` as
   today's entry). No page exceeds its line budget (acceptance 2/3/4). The banned
   apology/hedge vocabulary guard in `python/tests/test_docs_assets.py` (as extended by
   `MB.3.65`'s guard) is green, and the grep above prints nothing: no apology, no hedge, and
   no "coming soon". Nothing was deleted without a home: `RESULT.md` carries a one-line map
   from each passage removed from `index.md`/`php.md` to the page that now states it
   (`docs/reference/manifest.md` may gain only the sentence a gap needs; it is not
   restructured). The one factual roadmap sentence about Node/Tomcat stays on the root index,
   phrased as roadmap, not as "coming soon".

# Notes

- Order of work: outline → `dx` + `ai-ergonomics` goldfish reviews → fix the outline → write
  `python/index.md` and `php/index.md` → move/split the framework pages → rewrite
  `index.md` as the chooser → `zensical.toml` + inbound links → `inventory.toml` →
  `make docs-check`/`docs-unit`/`docs-test` → `visual-checker`.
- Do not touch `docs/frameworks/node.md` / `docs/frameworks/jvm.md`: they do not exist yet and
  inherit this shape when `MB.4`/`MB.5` land (state that in one line on the root index).
- Do not touch `docs/deploy/**` beyond the two inbound-link lines in `compose-traefik.md` and
  `digitalocean-paas.md`; `MB.18` just restructured that tree.
- `docs/decisions/ADR-0007-…:11` mentions `docs/frameworks/static-and-spa.md` in backticks,
  and that path is unchanged: leave the ADR alone (historical record, not a link).
- Keep today's snippet shape and meaning (`MB.19` acceptance 6): move sentences, link to where
  a rule now lives, invent no product behaviour and no new manifest key.
- If the outline shows the PHP split needs a fact nobody has verified (e.g. exactly which
  `var/cache` subdirectories must be writeable in a pre-warmed Symfony image), record it as a
  `Follow-ups` item in `RESULT.md` and keep the page to what the current `php.md` and
  `docs/reference/manifest.md` already state — no invented numbers.
