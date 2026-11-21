---
id: MB.24.1
title: "Align the concurrency docs: a route serves as many requests as the budget allows (one or more view processes per route)"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: [MB.20]
paths:
  - "docs/how-it-works.md"
  - docs/frameworks/php/index.md
  - "docs/design/parallel-views.review-dx.md"
  - "docs/design/parallel-views.review-ai-ergonomics.md"
  - "crates/blastit-manifest/tests/docs_fences.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-17T01:26"
updated: "2026-11-17T05:40"
---

# Goal

Say what concurrency actually means for a route, before any scheduler code changes. One
page currently claims a view is *"one per route"* and the other never says how many
requests a PHP route serves at once. After this task a developer reading
`docs/how-it-works.md` or `docs/frameworks/php.md` knows the rule in one sentence: a route
serves as many requests as `BLASTIT_MAX_WORKERS` allows, Python by running each request on
its own thread inside one view process, PHP by forking one process per concurrent request
because NTS `libphp` is one-request-per-process.

Docs-only. No Rust, no scheduler behaviour, no new page and no new `Blastfile` key.

# Context

- `docs/how-it-works.md:13` says `view` *"one per route, forked lazily"*, and the mermaid
  label at `docs/how-it-works.md:25` repeats it. Both become "one or more per route", with
  the lazy-spawn sentence at `docs/how-it-works.md:132` kept true.
- `docs/frameworks/php.md` is the language page a PHP developer reads; it must gain the
  one paragraph that says concurrent requests to one PHP route run in parallel worker
  processes bounded by `BLASTIT_MAX_WORKERS` (`crates/blastit-view/src/php.rs:10-17` is the
  reason in one clause: NTS `libphp` is one request per process).
- `docs/frameworks/**` is owned by `MB.20` (active). Land **after** it and touch only the
  concurrency paragraph, never the page's structure or its framework sections.
- DX-First: this task also records the `dx` and `ai-ergonomics` critic verdicts that
  `MB.24.2`'s design will be derived from. `MB.24.2` owns the design itself and the other
  four lenses; keep the four filenames below disjoint from it.
- Lean prose: at most two sentences in `how-it-works.md` and one short paragraph in
  `php.md`. No kernel lecture, no scheduler internals.

# Acceptance

```console
$ grep -rn "one per route" docs/ && echo FAIL || echo ok   # no count claim survives
$ make docs-check
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/runnable_docs -q
$ uv run --no-project tools/plan.py validate
```

Then a fresh-context `critic` pass with the `dx` and `ai-ergonomics` lenses, recorded as
`docs/design/parallel-views.review-dx.md` and
`docs/design/parallel-views.review-ai-ergonomics.md`; both must end with
`VERDICT: PASS` before `MB.24.2` starts. Quote the final page sentences in `RESULT.md`.

# Notes

- Do not promise a number of workers: the cap is `BLASTIT_MAX_WORKERS` and memory-derived,
  so say "as many as the budget allows" and link `docs/reference/settings.md`.
- Never pair PHP with a second transport or a degraded mode, and never name the retired
  manifest format.
