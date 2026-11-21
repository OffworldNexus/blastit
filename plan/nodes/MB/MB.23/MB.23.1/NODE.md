---
id: MB.23.1
title: "Write the live-reload DX docs first: local, dev --no-reload, log events, one troubleshooting row, dx + ai-ergonomics critics"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: [MB.22.1]
paths:
  - docs/local.md
  - docs/reference/cli.md
  - "docs/reference/log-events.md"
  - docs/troubleshooting.md
  - docs/blastfile.md
  - docs/frameworks/python/index.md
  - python/tests/runnable_docs/inventory.toml
  - "docs/design/dev-reload.review-dx.md"
  - "docs/design/dev-reload.review-ai-ergonomics.md"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T23:12"
updated: "2026-11-17T06:02"
---

# Goal

Write the **user-facing live-reload documentation first**, before any design or code. A
developer learns from two sentences that `blastit dev` now reloads by default, what a save
does, and that `--no-reload` is the one way to stop it — with no kernel lecture, no new
config key, and no second way to do the same thing.

Edits, in this order:

1. `docs/local.md` — replace the sentence *"The file is read once. Restart after editing a
   view or a grant."* with the two-line truth: **"Edits reload your views, all together.
   `--no-reload` turns that off."** Add a `--no-reload` row to the flag table
   (`default: reload on`). Add **≤ 6 lines** (prose + a 3-row table) saying what a save
   does: any file the app loaded (a view, a shared module, an editable package, a
   template) reloads every view together; `./Blastfile` or a `preload` module re-reads and
   reloads the whole topology; `:8080` never drops. One clause, no more: "no restart, no
   second terminal". Never claim that only one view or only the routes that loaded a
   module reload.
2. `docs/reference/cli.md` — add `$ blastit dev --no-reload` to the **existing** `## blastit dev`
   console fence (index `1`, inventory mode `dev-flags`, fixture `hello`) so the harness
   boots it for real; add the `--no-reload` row to that page's flag table; add one clause
   to the prose under it: reload is on and needs the host kernel to allow it; where a
   container profile denies it, the boot prints one warning and keeps serving, and
   `--no-reload` is silent by design.
3. `docs/reference/log-events.md` — add `reload` to the `process.retired` `reason` list,
   and two rows to the events table:
   `dev.reload` (info; fields `routes`, `files`, `reason` = `loaded`/`preload`/`blastfile`/`overflow`)
   and `dev.reload.unavailable` (warn; fields `errno`, `filesystem`). `routes` lists every
   view route that was reloaded together (all of them, on any tracked change); it never
   names a subset.
   **These four field names and the four reason words are frozen here**; `MB.23.2` must
   design to them and `MB.23.5` must implement them, not rename them.
4. `docs/troubleshooting.md` — one row in the existing table:
   `reload disabled: fanotify is not available here (EPERM)` → the three causes
   (container security profile, kernel without it, a filesystem without file handles) in
   one clause, plus `--no-reload`.
5. `python/tests/runnable_docs/inventory.toml` — only if a fence index changes. Adding a
   `$` line to an existing fence keeps its index, so the expected edit here is **none**;
   if the implementer does add a fence, it needs an entry with a mode, a fixture, and a
   real `reason` where the mode requires one.

Then run two fresh-context `critic` subagents (`dx` and `ai-ergonomics` lenses) and commit
their `VERDICT: PASS` files.

# Context

- The accepted DX target is §"Developer Experience Implications" of
  `docs/research/2026-11-16-dev-fanotify-reload.md`: the developer types nothing new, one
  sentence changes, and the observable lines are the four in that table. Do not invent a
  new command, env var, `Blastfile` key, `--watch` alias, or config file.
- `--no-reload` is **dev-only** and default ON. `blastit check`, `blastit test` and
  production have no reload at all and gain no flag.
- `docs/local.md` is 144 lines and must stay **≤ 160**; `docs/reference/cli.md` is 108 and
  must stay **≤ 125**. Lean prose: no kernel internals, no seccomp lecture, no `fanotify`
  API names, no explanation of how handles work — those live in the design doc.
- **Never write `inotify`** on any public page (there is no second watcher to compare
  against; naming one is how a banned mechanism gets reintroduced). Never write
  `BLASTIT_DEV_RELOAD` (not in this release). Never mention the retired manifest
  filename. PHP is simply another backend that reloads; no second transport is ever
  named.
- Keep the troubleshooting row one table row, like its neighbours.
- **The one model a reader must come away with:** a save reloads every view together; it
  never claims a single view or "the routes that loaded it". The watched set is the union
  of the `preload` phase and every view worker, so an editable package or template edited
  outside the project is caught too — say that once in one clause, without naming a
  watcher.
- `make docs-test` executes fences and would fail here because `--no-reload` does not exist
  until `MB.23.5`; this task runs `make docs-check` + `make docs-unit` only, and `MB.23.6`
  runs `make docs-test` after the flag exists.

# Acceptance

```console
$ make docs-check
$ make docs-unit
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
$ grep -rniE "inotify|BLASTIT_DEV_RELOAD" docs/local.md docs/reference/cli.md docs/reference/log-events.md docs/troubleshooting.md   # no output
$ grep -rni "read once" docs/local.md   # no output
$ grep -rniE "only (that|this) (view|route)|each of those routes|reloads exactly" docs/local.md docs/reference/cli.md docs/reference/log-events.md docs/troubleshooting.md   # no output
$ wc -l docs/local.md docs/reference/cli.md
$ head -5 docs/design/dev-reload.review-dx.md; tail -2 docs/design/dev-reload.review-dx.md
$ uv run --no-project tools/plan.py validate
```

1. `docs-check`, `docs-unit` and the docs-assets suite are green, and `wc -l` shows
   `local.md` ≤ 160 and `cli.md` ≤ 125.
2. The `blastit dev` fence in `docs/reference/cli.md` now contains
   `$ blastit dev --no-reload` as a `$ ` line, and the page's flag table row says the
   default is reload on.
3. `docs/reference/log-events.md` names `dev.reload`, `dev.reload.unavailable` and the
   `reload` retire reason with exactly the field names and the four reason words listed in
   the Goal, and `routes` is described as every view route reloaded together.
4. `docs/troubleshooting.md` has one row for the `EPERM` warning, and it says what still
   works (the server serves) and the one thing to try.
5. `docs/design/dev-reload.review-dx.md` and `docs/design/dev-reload.review-ai-ergonomics.md`
   each exist, each ends `VERDICT: PASS`, and neither carries a BLOCKER or MAJOR finding.
6. The greps above are empty, and no page gained a new console fence without an
   `inventory.toml` entry.

# Notes

- A `critic` writes the review file it is given; a `doc-writer` never writes its own
  passing review. Spawn them fresh, after the pages are final, and give them only the
  pages, `docs/VISION.md`, the research note and the instruction "judge only what is on
  disk".
- The `dev.reload` field list is the contract the rest of the epic follows; say so in a
  one-line comment in the review hand-off so `MB.23.2` does not quietly redesign it.
- If the `dx` critic asks for the reload lines to be shown verbatim, prefer a prose table
  over a `console` fence: a fence that prints a live reload is a screenshot, not a test,
  and `make docs-test` would have to fake it.
