# RESULT — MB.25.1

## Summary

Wrote the pre-code contract for the `runtime` event on the two pages MB.25.1 owns,
with no Rust and no new page, nav entry or fence.

- `docs/reference/log-events.md` (96 → 123 lines): one row beside
  `boot`/`ready`/`shutdown` in the event table, plus `## The `runtime` line` — the
  event is `info` from `component: "init"`; a 3-row table gives each entrypoint
  only its own fields (`php`: `version`, `libphp`, `php_ini`; `python`: `version`,
  `environment`; `static`: none); one sentence says a field a row does not list is
  absent, never `null`; one sentence states that `PHPRC`/`PHP_INI_SCAN_DIR` are
  stripped, so `php_ini` is the file actually opened; one sentence pins the
  `pretty` shape (entrypoint, `version`, the labels `libphp`/`php.ini`/
  `environment` two spaces apart, `php.ini none`, `static` followed by
  `serving static assets only`); one paragraph keeps `warn` for what needs
  attention (`manifest.warning` with `unrestricted`, `egress any`, a `postgres`
  store that no view or trait reaches, a `raw` body — both shapes in
  `blastit dev`, only the acknowledged `body raw unvalidated` in production — an
  unused `trait`, an `entrypoint php` that no view reaches, and
  `dev.reload.unavailable`).
- `docs/local.md` (160 → 169 lines): one lead-in and a 3-row table showing the one
  line `blastit dev` prints per shape, and one sentence that `none` means PHP's
  built-in defaults. No fence was added, so `docs_fences.rs` (`PAGES` for
  `docs/local.md` stays `Counts::new(1, 7)`) and `inventory.toml` are untouched.
- No `shim`, retired manifest filename, second PHP transport, `inotify`/
  `fanotify` or kernel internals on either page.

Final rows, `docs/local.md:22-24`:

```
| `entrypoint php "public/index.php"` | `00:11:36.867 INFO  runtime     php 8.5.4  libphp /usr/lib/libphp8.5.so  php.ini /etc/php/8.5/embed/php.ini` |
| `entrypoint python "app:app"` | `00:11:36.867 INFO  runtime     python 3.14.0  environment /home/you/shop/.venv` |
| no `entrypoint` | `00:11:36.867 INFO  runtime     static  serving static assets only` |
```

The two `dev` lines (and the static summary), 106 / 78 / 65 characters, event name
padded to the encoder's 12 columns:

```
00:11:36.867 INFO  runtime     php 8.5.4  libphp /usr/lib/libphp8.5.so  php.ini /etc/php/8.5/embed/php.ini
00:11:36.867 INFO  runtime     python 3.14.0  environment /home/you/shop/.venv
00:11:36.867 INFO  runtime     static  serving static assets only
```

Field table, `docs/reference/log-events.md:96-98`: `php` → `version` (`"8.5.4"`),
`libphp` (the resolved path), `php_ini` (the file opened, or `null` when none is);
`python` → `version` (`"3.14.0"`), `environment` (`VIRTUAL_ENV` when set, else
`sys.prefix`); `static` → `entrypoint` only, no language runtime is loaded.

Two critic lenses, both PASS: `docs/design/runtime-diagnostics.review-dx.md` and
`docs/design/runtime-diagnostics.review-ai-ergonomics.md` (`grep -c '^VERDICT'` is
1 per file, `PASS`). Round 1 found 5 MAJOR and 8 MINOR across the two lenses (dx:
an unexplained `php.ini none` and a cadence that contradicted dev reload; ai:
absent-vs-`null`, unpinned pretty labels, a false warn criterion), all fixed by
re-wording only. The dx round-2 pass returned PASS with two MINORs (unpinned
static summary, a garden-path warn item), applied. The ai round-2 pass returned
FAIL on one factual MAJOR — an unacknowledged `raw` body is a production boot
refusal, not a `warn` — which was
fixed in that reviewer's own words and confirmed by the same session; it then
ended PASS with one accepted non-blocking MINOR (the reference states "two spaces
apart" for the labelled fields only, so the single space inside `php 8.5.4` is
implicit in `docs/local.md`).

## Evidence

```
$ grep -n "runtime" docs/reference/log-events.md | head
37:| `runtime` | what the entrypoint resolved to, at boot and after a change that alters it | `entrypoint` (`php`, `python`, `static`), and only the fields its row lists — see below |
87:## The `runtime` line
89:`runtime` is `info` (`component: "init"`), one line each time the entrypoint
92:serving. A resolved runtime is not a problem.
98:| `static` | `entrypoint` only: no language runtime is loaded |
107:`runtime` is never `warn`: `warn` is a separate event and means something needs
```

```
$ ! grep -rn "shim" docs/local.md && echo ok
ok
```

```
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.92s
```

```
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/runnable_docs -q
ERROR python/tests/runnable_docs/fixtures/findings/tests/test_app.py
ERROR python/tests/runnable_docs/fixtures/learn/tests/test_app.py
ERROR python/tests/runnable_docs/fixtures/shop/tests/test_app.py
!!!!!!!!!!!!!!!!!!! Interrupted: 3 errors during collection !!!!!!!!!!!!!!!!!!!!
3 errors in 0.34s
```

Pre-existing and structural, not caused by this node: `python/tests/runnable_docs/`
has no `conftest.py` (only `python/tests/conftest.py` ignores `project` and
`live_project`), and the `fixtures/*/tests/test_app.py` files are app *projects*
that import `app`, not harness tests — `make docs-unit` runs
`test_runnable_docs.py` and `runnable_docs/test_harness.py` by name instead. The
same directory with the fixture projects skipped:

```
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/runnable_docs -q \
    --ignore=python/tests/runnable_docs/fixtures
.........................                                                [100%]
25 passed, 46 subtests passed in 0.11s
```

`git status --porcelain python/tests/runnable_docs crates/blastit-manifest/tests/docs_fences.rs`
prints nothing, so neither the fixtures nor the fence census were touched.

```
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
...................................                                      [100%]
35 passed in 1.77s
```

```
$ uv run --no-project tools/plan.py validate
plan OK
```

```
$ make docs-unit
.......s....................                                             [100%]
106 passed, 1 skipped, 65 subtests passed in 3.87s
```

```
$ wc -l docs/local.md docs/reference/log-events.md
  169 docs/local.md
  123 docs/reference/log-events.md
  292 total
```

## Files

- `docs/reference/log-events.md` — `runtime` row + `## The `runtime` line`; one
  pre-existing sentence corrected from "the one `warn` at boot" to "a `warn` at
  boot", which the new paragraph otherwise made false.
- `docs/local.md` — lead-in, 3-row `## Start` table, one sentence on `none`.
- `docs/design/runtime-diagnostics.review-dx.md` — dx critic, round 2, PASS.
- `docs/design/runtime-diagnostics.review-ai-ergonomics.md` — ai-ergonomics
  critic, round 2, PASS.
- `plan/nodes/MB/MB.25/MB.25.1/RESULT.md` — this file.

Untouched, deliberately: `crates/blastit-manifest/tests/docs_fences.rs`
(`docs/local.md` still `Counts::new(1, 7)`), `python/tests/runnable_docs/inventory.toml`,
`zensical.toml`, every other page.

## Follow-ups

- **MB.25.2 cadence.** The reference now says the line is written each time the
  entrypoint resolves, "in `blastit dev` again after a `Blastfile` change that
  alters it", so the dev reload path must re-emit it when the entrypoint changes
  (and only then). The epic said only "once at boot"; if the reload path cannot
  re-resolve the entrypoint, the page must say a change needs a restart instead.
- **Durable `php_ini` claim.** `null` is documented as "no file opened", and
  `docs/local.md` calls that "built-in defaults". If the SAPI surfaces
  `php_ini_opened_path` only after startup RINIT, the implementer must confirm a
  `null` really means no file rather than "not read yet".
- **`runnable_docs` collection.** A `collect_ignore` for
  `runnable_docs/fixtures/*/tests` in the runnable-docs harness would make the
  documented `pytest python/tests/runnable_docs` invocation work as written; it
  predates this node and is out of its `paths`.
- **Accepted residue.** The reference pins "two spaces apart" for the labelled
  `pretty` fields and leaves the single space inside `php 8.5.4` implicit in
  `docs/local.md:22`; MB.25.2 should copy that golden line verbatim.
