# RESULT — MB.24.1

Status: **done**. Docs-only; no Rust/Python/manifest change, no fence added, no new page.
Worktree `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`, branch `release/0.2.0`,
attempt 1/3. Nothing committed.

## What landed

`docs/how-it-works.md`

- Table row: `nothing but its own channel; one or more per route, forked lazily`.
- Mermaid label: `<b>view</b> · one or more per route · uid B+1000+i`.
- Sequence-diagram spawn condition: `opt no idle view process for this route` (a busy route
  with one live worker must still spawn), `SPAWN via init` unchanged.
- Two new sentences, after the diagram caption, no scheduler internals:

  > A route serves as many requests at once as the shared `BLASTIT_MAX_WORKERS`
  > budget allows. Python threads each request inside its view process; PHP runs each
  > concurrent request in its own view process, because NTS `libphp` runs one request
  > per process ([settings](reference/settings.md#admission-and-threads)).

  The lazy-spawn sentence at `:137` ("lazy spawning and a cap on live view processes")
  is untouched and still true.

`docs/frameworks/php/index.md` — heading `## One request at a time` → `## Concurrency`
(no inbound `#one-request-at-a-time` anchor existed) and one short paragraph, before the
untouched `Route uids are …` ACL note:

  > Concurrent requests to one PHP route run in parallel in separate view processes, bounded by the
  > shared `BLASTIT_MAX_WORKERS` budget, because the distro's NTS `libphp` runs one request per process
  > ([settings](../../reference/settings.md#admission-and-threads)). `BLASTIT_VIEW_MAX_WORKERS` caps one
  > process, not the route; there is no daemon or socket between Blastit and PHP.

Single transport preserved (intro's "no PHP server beside it" plus the clause above); no degraded
mode, no retired manifest format, `Blastfile` only. Framework sections, fences and page structure
otherwise untouched.

`crates/blastit-manifest/tests/docs_fences.rs` — **not changed**: no fence was added, `PAGES`
(`docs/frameworks/php/index.md` = 1 document + 3 illustrations) and `STATEMENT_LISTS` still match.

## Critic passes (fresh context, one re-run per lens)

| Lens | File | Round 1 | Round 2 (after fixes) |
|---|---|---|---|
| `dx` | `docs/design/parallel-views.review-dx.md` | FAIL — 0 BLOCKER / 2 MAJOR / 5 MINOR | **PASS — 0 BLOCKER / 0 MAJOR** / 2 MINOR (out-of-scope follow-ups) |
| `ai-ergonomics` | `docs/design/parallel-views.review-ai-ergonomics.md` | FAIL — 0 BLOCKER / 3 MAJOR / 4 MINOR | **PASS — 0 BLOCKER / 0 MAJOR** / 5 MINOR |

Both files end with `VERDICT: PASS`. Resolved MAJORs: budget stated as the **shared** budget (not a
per-route allowance, no promised count); PHP heading no longer asserts the retired absolute; the
`BLASTIT_VIEW_MAX_WORKERS` wrong-knob sentence added. Resolved MINORs: `1..N` → "one or more",
no "PHP forks" actor claim, "no idle view process" spawn condition, rule split from the uid note.
One further page edit was forced **after** the first fix round: a suggested "no FastCGI daemon"
clause is forbidden by `test_the_real_tree_carries_no_retired_php_vocabulary`, so the page says
"no daemon or socket between Blastit and PHP" and both critics re-read that final text.

## Acceptance

`grep -rn "one per route" docs/ && echo FAIL || echo ok` → FAIL, **only** in excluded
`docs/design/**` records (no public page):

```console
docs/design/ma-lean-dx.review-dx.md:60:### F4 — … Blastit loads one WSGI callable, not one per route
docs/design/learn-single-pass.md:156:… `blastit dev` keeps `ManifestWarning::RawIngress` (one per route, unchanged) …
docs/design/learn-single-pass.md:1236:…  `dev`'s warning stays one per route
docs/design/deploy-restructure.review-dx.md:250:        view["view · one per route"]
docs/design/parallel-views.review-ai-ergonomics.md:66: … quotes this very grep …
FAIL
```

`grep -rn "one per route" docs/ README.md --exclude='*.review-*.md' && echo FAIL || echo ok` →
two remaining lines, both `docs/design/learn-single-pass.md:156` and `:1236`, about the
`RawIngress` **warning** (one warning per raw route), not view counts. No public page or README
carries the old count claim; all hits are point-in-time `docs/design/**` records excluded from the
Zensical site and outside this node's paths.

`make docs-check`:

```console
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s
```

`UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q`:

```console
35 passed in 0.64s
```

`uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/runnable_docs -q` → 3
collection errors, **pre-existing and unrelated to docs**: `python/tests/runnable_docs/fixtures/`
holds complete sample apps (`app.py`, its own `pytest.ini`) that the fence harness runs in a
subprocess, so bare pytest from the repo root cannot import `app`:

```console
ERROR python/tests/runnable_docs/fixtures/findings/tests/test_app.py
ERROR python/tests/runnable_docs/fixtures/learn/tests/test_app.py
ERROR python/tests/runnable_docs/fixtures/shop/tests/test_app.py
!!!!!!!!!!!!!!!!!!! Interrupted: 3 errors during collection !!!!!!!!!!!!!!!!!!!!
3 errors in 0.24s
```

`git status --porcelain python/` is empty (this node changed no Python). The same suite without the
sample-app data, and the repo's canonical gate, pass:

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/runnable_docs -q \
    --ignore=python/tests/runnable_docs/fixtures
25 passed, 46 subtests passed in 0.08s
$ make docs-unit
107 passed, 65 subtests passed in 6.93s
```

`uv run --no-project tools/plan.py validate` → `plan OK`.

Extra evidence: `cargo test -p blastit-manifest --test docs_fences` → `11 passed`.

## Follow-ups (out of scope)

- `docs/reference/settings.md:68` calls the budget "live request **threads**" and
  `docs/frameworks/index.md:28` links it as "threads"; the PHP page now links a process-based
  explanation to that row. Reword to request slots (a Python thread or a PHP worker process).
- `docs/troubleshooting.md:92`, `docs/reference/settings.md:115` and
  `docs/deploy/anything-else.md:27` still write `prefork = true`; the DSL directive is bare
  `prefork` (`docs/reference/blastfile.md:219`).
- `docs/deploy/anything-else.md:14` ("one route should not hold every thread") is Python-shaped
  advice for PHP routes.
- `docs/reference/blastfile.md:156` ("A view is one sandbox: one worker") and
  `docs/frameworks/python/index.md:29` ("One view, one worker") read as one worker per route once
  `MB.24.2` lands the pool; revisit with that node.
- Node acceptance command 4 collects the `runnable_docs/fixtures` sample apps; either add
  `norecursedirs`/`--ignore=…/fixtures` to the documented invocation or state `make docs-unit`
  as the runnable-docs gate (the harness itself is `python/tests/runnable_docs/run.py`).
- `docs/frameworks/php/index.md:57` "We support supported PHP versions" duplicate word (pre-existing,
  outside the paragraph this node may touch).
