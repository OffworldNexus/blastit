# Result MB.21.4.4

**`STATUS: done`** — every `e2e/**` fixture, the two `python/tests` project
fixtures 4.7 left, the scripts' staged manifests and the Makefile's docs-serve
stage are `Blastfile`s; `make check` and `make e2e` both exit `0`;
`blastit fmt --check` passes on all nine fixtures; `pytest python/tests` (CI's own
command, `DATABASE_URL` set) is `399 passed, 1 skipped`; nothing under `e2e/` or
`python/tests/` names the retired file any more.

## 1. What landed

| Old | New | Notes |
|---|---|---|
| `e2e/demo/blastit.toml` | `e2e/demo/Blastfile` | `[proxy.db]` → `postgres $DATABASE_URL`; the two groups → `trait connection`/`trait session` attached by `use`; eight routes → eight views; `deny`'s `[route.db] queries = []` → an empty `postgres { }` with no trait (the empty allowlist assertion); `egress = true` → `egress any` |
| `e2e/php/fixture/blastit.toml` | `e2e/php/fixture/Blastfile` | the two `[route.php]` entries → one `entrypoint php "public/index.php"`; the per-route `[route.static] root` → one `static "/" { root "public" }` mount; two views (`api` above the catch-all); `workers = 1` gone (§12.3 of `ADR-0012` removed the keyword) |
| `e2e/sqlalchemy/blastit.toml` | `e2e/sqlalchemy/Blastfile` | the global `groups = ["connection"]` → `use connection` on each view; the group's `transaction_control = true` → `transactions` in the trait |
| `e2e/static_site/blastit.toml` | `e2e/static_site/Blastfile` | route + `[route.static] root/spa/immutable` → `static "/" { root; immutable; fallback "index.html" }` (`spa = true` is one fact with `fallback`) |
| `e2e/static_site/nospa/blastit.toml` | `e2e/static_site/nospa/Blastfile` | `static "/" { root "public" }`, no `fallback` |
| `e2e/venv-smoke/blastit.toml` | `e2e/venv-smoke/Blastfile` | one view, one store, one grant; `BLASTIT_APP` still supplies the callable |
| `python/tests/live_project/blastit.toml` | `python/tests/live_project/Blastfile` | the top-level `groups = ["django"]` → `trait django` + `use django`; both views keep their half-written grants |
| `python/tests/packaged_project/blastit.toml` | `python/tests/packaged_project/Blastfile` | the same shape, one view |
| `e2e/php/fixture/api/index.php` | *deleted* | it was the second route's `[route.php] entry`; a `Blastfile` declares one entrypoint, and the `api` view reaches `public/index.php` like every other view |
| `e2e/php-sapi/run-unsupported-minor.sh` | staged heredoc → `BLASTFILE`, `entrypoint php "public/index.php"` + `view app regex '.*'` | |
| `scripts/standalone-runtime-check.sh` | both heredocs retagged `BLASTFILE`: `static "/" { root "site" }` and `view web regex '.*'` | |
| `scripts/wheel-smoke.sh` | heredoc → `BLASTFILE`, `static "/" { root "site" }` | |
| `Makefile` | `DOCS_SERVE_MANIFEST := $(DOCS_SERVE_STAGE)/Blastfile`; the stage writes the comment + `static "/" { root "site" }` | `docs-serve`/`docs-serve-check` read the variable, so both kept working |
| `e2e/{Dockerfile,Dockerfile.prebuilt,rootless/Dockerfile,rootless/Dockerfile.prebuilt}` | `BLASTIT_MANIFEST=/app/Blastfile` | the images' demo app |
| `e2e/compose.yaml` | `/sqlalchemy/Blastfile`, `/site/Blastfile` (both static services) | |
| `e2e/rootless/run.py` | `MISSING_MANIFEST = "./Blastfile: no such file or directory"`; `MANIFEST_WITHOUT_APP = 'view home "/" {\n}\n'`; writes `Blastfile` | both sentences re-verified against the binary (§3) |
| `e2e/rootless/php_entry_swap.py` | `rm -f /app/Blastfile`; the refusal assertion pins `entrypoint php /app/public/index.php` instead of `[route.php] entry` | the worker's own sentence 4.3 re-spelled; the path is one fact more than before |
| `e2e/venv-smoke/run.sh` | copies `/fixture/Blastfile`, `BLASTIT_MANIFEST=$work/Blastfile` (3 sites) | |
| `python/tests/test_live_server.py` | `--blastit-manifest Blastfile` (4 sites) | |
| `python/tests/test_check.py` | `MANIFEST`/`DJANGO_MANIFEST` are Blastfiles; writes `Blastfile`; ini value `Blastfile`; `fix.to` is `view home postgres`; learn asserts `view extra "/extra"`; `with_transactions()` replaces the `transaction_control = true` suffix | |
| `python/tests/test_project_manifest.py` | the retired-name absence check became `PROJECT.glob("Blastfile*") == ["Blastfile"]` | 4.7's file, kept meaningful |
| `python/tests/test_runnable_docs.py` | the heredoc sample is `cat > Blastfile <<'BLASTFILE'`; the `toml` title is `pyproject.toml` | parser samples only |
| `e2e/grants.py` | **rewritten** to emit the `Blastfile` (views, traits, canonical layout) | regenerating `e2e/demo/Blastfile` from a harvest reproduces it byte for byte (§3) |
| `e2e/php2shell.py`, `e2e/static_assets.py` | read/assert the `Blastfile` (§2); `assert_exchange` prints one sentence per outcome (`the answer is 200`, not `200, not 200`); `banner_line` compares the fixed words and the live count, and `replay` pins the page's own `1 route` and the live `0 routes` | |
| `e2e/check_logs.py`, `e2e/scenario.py` | the deny view's gate message says "a view granting an empty `postgres { }`" / "the view with an empty `postgres { }`" and keeps SQLSTATE 42501 | the retired `queries = []` spelling is gone from a failure a reader sees |
| `python/tests/project/Blastfile` | `blastit fmt` (one blank line before `postgres`), so acceptance 4 holds for all nine fixtures | 4.7 wrote it; `fmt --check` was the only thing wrong with it |
| READMEs (`e2e/`, `e2e/php/`, `e2e/rootless/`, `python/tests/`) | the paths, the tree, the sentences | |

## 2. The two changes the language forced, and why neither is weaker

### (a) One `entrypoint php`, so `/api/…` is the same front controller

`ADR-0012` §12.3 removed the per-route `[route.php] entry`/`workers`: a
`Blastfile` declares one `entrypoint php`. Consequences, each asserted:

| Probe | Was | Now | Why not weaker |
|---|---|---|---|
| `E9` (`/api/__inspect?…`) | `script_filename == {root}/api/index.php` | `== {root}/public/index.php` | the assertion's subject — a pinned `.php` inside `root`, not a header-named file — is unchanged; there is one entrypoint now |
| `E23` (`/api/../../etc/passwd`) | `200`, `path_info` echoed, entry pinned | `400` | the file's `static "/"` mount claims every path, and §9.2 step 0 refuses `..` before a view sees it; measured by hand (§3). The pinned-entry half lives in `E1`/`E9` on the same path shape |
| `check_learn` | "one `EmbeddedView` worker set per `[route.php]` route" | "one worker per view" | the counts are identical (`routes == ["api","app"]`, `>= 2` pids); only the words that named the removed keyword changed |
| `E23`'s `path_info` echo | asserted the app received `..` | dropped with the `200` | nothing reaches a PHP view with `..` any more; `E9` asserts `script_filename` for the same route |

`e2e/php/probes.md` row 23 is unchanged: it is the verbatim §4.7 record, and its
own header says the `Blastit handling`/`Outcome`/`Owner` columns are kept for the
record (`E24`'s `502` is stale in the same way).

### (b) A `static` mount is not a route, so the static leg counts zero

`docs/frameworks/static-and-spa.md`'s fence says `… · 1 route` for its fixture,
which was a route. A mount is not a route (§9.5), so the live banner says
`0 routes` and `check --json` reports `routes.total == 0`. The page itself is
`MB.21.5.1`'s and this leaf does not edit a page:

* `e2e/static_assets.py::banner_line` compares the summary's fixed words
  verbatim against the live line, and `replay` then asserts **both** counts
  separately: the page's own line must still say `1 route` (the fixture it was
  written for) and the live banner must say `0 routes` (a mount is not a
  route). The page's stale count is therefore compared, not ignored: when
  `MB.21.5.1` rewrites it to `0 routes`, this check fails and the page edit and
  the harness move together in one deliberate diff;
* the `check --json` assertion is now `routes.total == 0` with the reason (a
  mount is not a route), not `1`.

### (c) Two `python/tests` assertions 4.7 left disagreeing with the cutover

Both were red at the base commit (`git archive 621626c python`, same venv:
`2 failed, 37 deselected`), and `python/tests/**` is this leaf's, so they are
fixed here — each on the side that was wrong, not by bending the count.

**1. An unparsable `Blastfile` is a usage error — the *test* was wrong.**
`test_an_invalid_manifest_fails_the_session` asserted exit `1`, `the audit
failed` on stdout and the word `invalid`; that is the retired reader's shape (a
file parsed late, then failed as a session). The plugin's documented contract is
`pytest.UsageError` — exit `4` — wherever it cannot read the file it was pointed
at: `_manifest_path` ("the configured file is not there, because the alternative
is a report that silently diffs against nothing"), `_blastfile_model` (the same
sentence for a file that does not parse), a `--blastit-proxy` typo, and two
stores with no mapping. The file's own other four refusal cases already assert
`4` (`:672`, `:706`, `:716`, `:1174`), so this one was the odd one out. Measured
on a copy of `python/tests/project/` with `Blastfile-broken`
(`view notes regex '(' {`): exit `4`, stdout empty,
``ERROR: blastit: the Blastfile does not parse: the manifest is invalid: 3:18:
regex `(` does not compile: error: unclosed group`` on stderr, no report. The
renamed `test_an_invalid_manifest_is_a_usage_error` asserts all four facts — two
more than before (empty stdout, and the reader's own `does not compile`
sentence).

**2. The view the repair creates did not `use` the trait it wrote — the *fixture
helper* was wrong.**
`repaired_manifest` (`python/tests/test_pytest_plugin.py`) puts the grantable
connection-setup statements in `trait connection` and gives **every** view
`use connection` — its docstring says so, and the report's `add_group` fix says
"`use connection` in every view that opens the store". It appended the view for
the unrouted path afterwards, with `"use": []`, so the one view the repair itself
created opened the store without the trait: on the next run the report named
`SELECT set_config('TimeZone', $?, false)` ungranted there. Measured with a
three-run replay of the test's own steps:

```console
# before the fix — third run
"SELECT set_config('TimeZone', $?, false)"  granted_in=13  ungranted_in=1
"SET application_name = 'blastit-test'"     granted_in=0   ungranted_in=14
totals: {… "connection_setup_ungranted": 2 …}       # repaired2 had 14 views, 13 `use`

# after the fix
"SELECT set_config('TimeZone', $?, false)"  granted_in=14  ungranted_in=0
"SET application_name = 'blastit-test'"     granted_in=0   ungranted_in=14
totals: {… "connection_setup_ungranted": 1 …}       # every view uses it
```

`1` is right in both server zones: the only statement a grant may not hold is
`SET application_name = 'blastit-test'` (an inlined literal, `R24`), and
`test_connection_setup_names_the_routes_that_do_not_grant_it` already derives
whether Django also sends the timezone statement instead of assuming it. Nothing
else in the file depended on the old number: the baseline totals
(`test_a_grant_nothing_matched_is_unused`: `unused == 2`) are the first run and
did not move; the third run's `unused` is not one of the asserted totals; and
`test_project_manifest.py`'s companion assertion, which had excluded `fallback`
from "every view uses the trait", now includes it and pins `fallback`'s
`use == ["connection"]`.

## 3. Acceptance evidence

All commands were run in
`/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0` on the final tree.

### `make check` — PASS

```console
$ timeout 5400 make check > /tmp/opencode/make-check-final5.log 2>&1; echo EXIT=$?
EXIT=0
$ grep -c 'test result: ok' /tmp/opencode/make-check-final5.log
166
$ grep -E 'test result: FAILED|make: \*\*\*' /tmp/opencode/make-check-final5.log
(no output)
$ tail -2 /tmp/opencode/make-check-final5.log
uv run --no-project tools/plan.py validate
plan OK
$ uv run --no-project tools/guard.py
guard: OK (24 staged/untracked file(s) checked)
```

### `make e2e` — PASS

```console
$ timeout 9000 make e2e > /tmp/opencode/make-e2e-final3.log 2>&1; echo EXIT=$?
EXIT=0
$ grep -cE '^FAIL' /tmp/opencode/make-e2e-final3.log
0
```

This is the run **after** the reviewer's four minors, so it is the evidence for
them as well:

```console
$ grep -nE 'one-route banner|counts no routes|replayed against|P1 E1:|P1 E23:|one worker per view|view with an empty|view granting an empty|docs_deploy:' \
    /tmp/opencode/make-e2e-final3.log
299:PASS: the page documents the one-route banner it was written for ('  http · scheduler · dbproxy · 1 route'), so changing it is its own diff
300:PASS: and the summary counts no routes: a `static` mount is not one
361:PASS: docs/frameworks/static-and-spa.md replayed against a real blastit dev (69 checks)
398:PASS: P1 E1: the answer is 200                       # one sentence, not "200, not 200"
500:PASS: P1 E23: the answer is 400
526:PASS: one worker per view, each its own pid (['api', 'app'])
532:PASS: the php2shell matrix and docs/frameworks/php.md replayed against a real blastit dev (139 checks)
598:  ok   a view granting an empty `postgres { }` refuses an ordinary statement (SQLSTATE 42501)
635:  ok   the view with an empty `postgres { }` refused an ordinary statement with SQLSTATE 42501
721:docs_deploy: 45/45 documented pooler promises hold
```

The static leg gained the page-count check (68 → 69 checks); the php2shell leg
passes 139 checks with the equal-status message fixed; the demo's `deny` gate
and `check_logs`' line now name an empty `postgres { }` block.

Every leg ran, and every leg loaded a `Blastfile`:

| Leg | Evidence it read a `Blastfile` |
|---|---|
| Django demo (compose `app`) | `BLASTIT_MANIFEST=/app/Blastfile` (image env), the scenario's `sql.denied` lines and the route map below |
| Flask + SQLAlchemy (`sqlalchemy`) | `BLASTIT_MANIFEST=/sqlalchemy/Blastfile` |
| static / static_nospa | `BLASTIT_MANIFEST=/site/Blastfile`, both served |
| php2shell | the driver starts `blastit dev` in the site copy, so the default `./Blastfile` is what it reads |
| rootless D6 | `./Blastfile: no such file or directory` — the boot looks for the new file, and nothing else |
| `docs-serve-check` | `target/docs-serve/Blastfile` (above) |

```console
$ grep -nE 'PASS: (`--learn` records|and the two views)|ok   every refusal names' \
    /tmp/opencode/make-e2e-final3.log
528:PASS: `--learn` records nothing: this file's views run no query
530:PASS: and the two views are two processes, not one
637:  ok   every refusal names its own route ({'deny', 'notes', 'sql'})
971:  ok   every refusal names its own route ({'deny', 'notes', 'sql'})

$ grep -n 'names the entrypoint\|names the file and the reason' /tmp/opencode/make-e2e-final3.log
1039:  ok   and stdout's `app.output` names the file and the reason (./Blastfile: no such file or directory); stderr carries 0 bytes
1083:  ok   and the worker names the entrypoint, its file and the fix in its log
```

The matrix, the static page and the demo's empty-grant assertion are all green:
`E23` is the `400` this leaf predicts (`PASS: P1 E23: the answer is 400`), the
static banner's summary is `0 routes` while the page still documents `1 route`,
and `/deny/sql/` still refuses with `42501` on a listener whose allowlist is
empty.

**What the gate found, and what it cost.** The first `make e2e` run failed one
promise in the rootless PHP entry-swap check: it looked for
`"[route.php] entry"` in the worker's refusal, and the worker now says
`entrypoint php <path>` (the message 4.3 re-spelled). The assertion now pins the
directive **and** the entry's path — `"entrypoint php /app/public/index.php"` —
which is one fact more than before. Re-run of that leg alone (`EXIT=0`) and the
full `make e2e` above are both green.

One **one-off** was observed and not reproduced: during a second full run, `E7`
(the first `POST` to the `/api` view) answered `502 bad gateway` (the scheduler's
`view_abort`), and every following request in that same run passed. It is not
reproducible: the same image replayed the whole matrix 3/3 times
(`PASS … (139 checks)`) and 8/8 cold boots answered that exact `POST` `200`
locally; the run recorded above is green. Left as an observation, not a claim.

**This run is the final tree.** It was made after every change in this leaf,
§2(c) and the four review minors included; the two files §2(c) touches
(`python/tests/test_pytest_plugin.py`, `test_project_manifest.py`) are imported by
no e2e leg (`grep -rlnE 'test_pytest_plugin|test_project_manifest' e2e scripts
Makefile` → nothing), and the fixtures the harness serves are the ones listed
above.

### `make docs-serve-check` (acceptance 4) — PASS

```console
$ timeout 1800 make docs-serve-check > /tmp/opencode/docs-serve-check.log 2>&1; echo EXIT=$?
EXIT=0
$ tail -5 /tmp/opencode/docs-serve-check.log
docs-serve-check: pid 1326888 (/proc/self/exe --stage=1 --config-fd=3 --dev-root=/run/user/1000/blastit-dev-1000-1326888-1e25fb)
ok   / (index)                      200  text/html; charset=utf-8  no-cache
ok   /reference/cli/                200  text/html; charset=utf-8  no-cache
ok   missing path (not the index)   404  text/plain; charset=utf-8  no-store
ok   asset /assets/javascripts/bundle.3cb411a8.min.js 200  text/javascript  no-cache
docs-serve-check: 4/4 checks passed (server log: …/target/docs-serve/check.log)
$ cat target/docs-serve/Blastfile
# Generated by `make docs-serve`; blastit serves the built site from here.
static "/" {
    root "site"
}
```

### The retired-name grep — empty (acceptance 2)

```console
$ grep -rnE 'blastit[.]toml' python/ scripts/ e2e/ Makefile \
    --exclude-dir=__pycache__ --exclude='*.so' --exclude-dir=target
(no output; exit 1)
$ grep -rlE 'blastit[.]toml' crates/ --include='*.rs' | sort     # MB.21.4.5's, listed in §4
crates/blastit-env/tests/environment.rs
crates/blastit-manifest/src/model.rs
crates/blastit-manifest/src/parse.rs
crates/blastit-manifest/src/validate.rs
crates/blastit-manifest/tests/critical_manifest_011.rs
crates/blastit-manifest/tests/critical_php_manifest.rs
crates/blastit-manifest/tests/docs_fences.rs
crates/blastit-scheduler/src/config.rs
crates/blastit-sqlpolicy/src/learn.rs
```

### The two `find` counts (acceptance 3) — `15` and `0`

```console
$ find e2e python/tests -name 'Blastfile' | wc -l
15
$ find e2e python/tests -name '*.toml' ! -name 'Cargo.toml' ! -name 'inventory.toml' | wc -l
0
$ find e2e python/tests -name 'Blastfile' | sort
e2e/demo/Blastfile
e2e/php/fixture/Blastfile
e2e/sqlalchemy/Blastfile
e2e/static_site/Blastfile
e2e/static_site/nospa/Blastfile
e2e/venv-smoke/Blastfile
python/tests/live_project/Blastfile
python/tests/packaged_project/Blastfile
python/tests/project/Blastfile
python/tests/runnable_docs/fixtures/denial/Blastfile
python/tests/runnable_docs/fixtures/findings/Blastfile
python/tests/runnable_docs/fixtures/hello/Blastfile
python/tests/runnable_docs/fixtures/learn/Blastfile
python/tests/runnable_docs/fixtures/shop/Blastfile
python/tests/runnable_docs/fixtures/static-site/Blastfile
```

The six `runnable_docs` ones and `python/tests/project/` are `MB.21.4.1`'s and
`MB.21.4.7`'s; the eight above them are this leaf's.

### `blastit fmt --check` in every fixture (acceptance 4) — PASS, nine of nine

The eight this leaf translated **and** `python/tests/project/Blastfile`, which
4.7 wrote: it was not canonical (one blank line before `postgres`), so
`blastit fmt` rewrote it — a one-line diff, no model change:

```console
$ ./target/debug/blastit fmt --check python/tests/project/Blastfile; echo EXIT=$?
python/tests/project/Blastfile: not in canonical layout
EXIT=1
$ ./target/debug/blastit fmt python/tests/project/Blastfile
$ git diff python/tests/project/Blastfile
@@ -5,7 +5,6 @@
 # the tests use has no view at all, and no view grants the statements psycopg
 # runs as a connection is established. Every one of those is a line in the report
 # the plugin writes.
-
 postgres $DATABASE_URL
 
 # Complete: the single SELECT this view sends is granted.

$ for f in e2e/demo/Blastfile e2e/sqlalchemy/Blastfile e2e/static_site/Blastfile \
    e2e/static_site/nospa/Blastfile e2e/venv-smoke/Blastfile e2e/php/fixture/Blastfile \
    python/tests/live_project/Blastfile python/tests/packaged_project/Blastfile \
    python/tests/project/Blastfile; do
    ./target/debug/blastit fmt --check "$f" || echo "  ^ $f"; done; echo FMT_NINE_EXIT=$?
FMT_NINE_EXIT=0
```

`blastit fmt --check` passes on `target/docs-serve/Blastfile` too (it is the
file `make docs-serve-check` wrote above).

### The two behaviours measured by hand, before Docker (the §2 claims)

```console
$ cd /tmp/opencode/staticsite && <tree>/target/debug/blastit dev --port 18996   # static fixture
  http · scheduler · dbproxy · 0 routes
$ raw /../etc/passwd       → HTTP/1.1 400 Bad Request
$ raw /api/../../etc/passwd → HTTP/1.1 400 Bad Request
$ raw /assets/app.4f1c2a9b7d3e.js → 200 … Cache-Control: public, max-age=31536000, immutable
$ raw /dashboard/settings  → 200 text/html  Cache-Control: no-cache
$ BLASTIT_MANIFEST=…/Blastfile <tree>/target/debug/blastit check --json
{"schema_version": 1, "ok": true, …, "routes": {"total": 0, "exercised": 0}, …}
EXIT=0

$ cd /tmp/opencode/phpfx && <tree>/target/debug/blastit dev --port 18990        # PHP fixture
  http · scheduler · dbproxy · 2 routes
$ raw /                → 200 {"entry":"front-controller","script_filename":"…/public/index.php",…}
$ raw /api/__inspect   → 200 … "script_filename":"…/public/index.php"     # the `api` view
$ raw /index.php       → 404                                              # never served
$ raw /uploads/shell.jpg/x.php → 404
$ grep 'GET /api/__inspect' dev.log → "GET /api/__inspect → api"
```

### `grants.py` regenerates the committed demo `Blastfile` byte for byte — PASS

```console
$ uv run --no-project python /tmp/opencode/synthetic_harvest.py /tmp/opencode/harvest.jsonl
wrote /tmp/opencode/harvest.jsonl (19 records)
$ uv run --no-project python e2e/grants.py /tmp/opencode/harvest.jsonl > /tmp/opencode/regenerated.Blastfile
$ diff -u e2e/demo/Blastfile /tmp/opencode/regenerated.Blastfile
(no output; identical)
```

### The rewritten `python/tests/` fixtures run through the plugin — PASS

Without a database those suites skip (their own `pytestmark`), so I brought one
up (`docker run postgres:18`, a password in the URL, nothing read from `.env`)
and installed the built wheel plus `django pytest pytest-django pytest-xdist
psycopg[binary]` into a venv **outside** `/home/remy` (the production live server
runs as root here, and a view uid has to be able to read the interpreter):

```console
$ docker run -d --name blastit-live-pg -e POSTGRES_PASSWORD=… -e POSTGRES_USER=blastit \
    -e POSTGRES_DB=blastit -p 127.0.0.1:55432:5432 postgres:18
$ DATABASE_URL='postgresql://blastit:…@127.0.0.1:55432/blastit' \
    /tmp/opencode/livevenv2/bin/python -m pytest python/tests/test_live_server.py -q
.....                                                                    [100%]
5 passed in 9.41s

$ DATABASE_URL=… python -m pytest python/tests/test_check.py -q
..........                                                               [100%]
10 passed in 17.83s

$ PYTHONPATH=target/python-critical:python uv run --no-project --with pytest==9.2.0 \
    python -m pytest python/tests/test_project_manifest.py -q
.......                                                                  [100%]
7 passed in 0.19s
```

The five live-server sessions are the plugin's own `--blastit-manifest Blastfile`
runs of `live_project/` and `packaged_project/` — the two fixtures this leaf
translated, served by the real binary (production mode, five-role tree) and
asserted through the report. `test_check.py`'s ten are the `blastit check` twins,
including `--learn --dry-run`, the real learn write and the `pytest-django`
transaction cases (with `with_transactions()`).

### The whole suite, on CI's own command — PASS

CI's `pytest-plugin` job runs `pytest python/tests -q
--ignore=python/tests/runnable_docs/fixtures` against `postgres:18` with
`DATABASE_URL` set (`ci.yml:546-591`), so that is the command and the server I
ran:

```console
$ docker run -d --name blastit-live-pg -e POSTGRES_PASSWORD=… -e POSTGRES_USER=blastit \
    -e POSTGRES_DB=blastit -p 127.0.0.1:55432:5432 postgres:18
$ DATABASE_URL='postgresql://blastit:…@127.0.0.1:55432/blastit' \
    /tmp/opencode/livevenv2/bin/python -m pytest python/tests -q \
    --ignore=python/tests/runnable_docs/fixtures
399 passed, 1 skipped, 65 subtests passed in 78.67s (0:01:18)
```

The one skip is an inner fixture app's own test collected by the outer run
(`packaged_project/tests/test_live.py`, "needs `--blastit-manifest`"), not a
suite this leaf touched. `-rs`:

```console
SKIPPED [1] python/tests/packaged_project/tests/test_live.py:16: blastit: a live server
needs --blastit-manifest (or blastit_manifest in the configuration) to serve the project with
```

and the top-level modules alone, for the plugin's own counts:

```console
$ DATABASE_URL=… python -m pytest python/tests/test_*.py -q
324 passed, 19 subtests passed in 71.07s (0:01:11)
```

`python/tests/test_pytest_plugin.py` alone, on both server zones (the file's
`TIMEZONE_SETUP_SQL` note says the second one is server-dependent):

```console
$ DATABASE_URL=…55432… (postgres:18, `Etc/UTC`)  pytest …/test_pytest_plugin.py -q
39 passed in 42.05s

$ DATABASE_URL=…55433… (postgres:15-alpine, `UTC`) pytest …/test_pytest_plugin.py -q
39 passed in 40.77s
```

The first full run of this session had three `-n 2` cases exit `4` because my
first venv lacked `pytest-xdist`; installing it fixed those. The other two are
§2(c): both were real, and both are fixed in `python/tests/` here.


### No dependency moved (acceptance 5) — PASS

```console
$ git diff --stat Cargo.toml Cargo.lock
(no output)
$ uv run --no-project tools/critical_guard.py --base 621626c --node MB.21.4.4
critical-guard: 621626c..working tree, 49 changed file(s)
node MB.21.4.4: paths ['e2e/**', 'python/tests/**', 'scripts/**', 'Makefile']
  no change to a protected critical test
EXIT=0
```

`python/tests/critical/**` is not in this leaf's change set: 4.7 had already
translated both protected fixtures, and no fixture *beside* them is protected.

## 4. The Rust mentions this leaf leaves alone (`MB.21.4.5`'s)

```console
$ grep -rlE 'blastit[.]toml' crates/ --include='*.rs' | sort
crates/blastit-env/tests/environment.rs
crates/blastit-manifest/src/model.rs
crates/blastit-manifest/src/parse.rs
crates/blastit-manifest/src/validate.rs
crates/blastit-manifest/tests/critical_manifest_011.rs
crates/blastit-manifest/tests/critical_php_manifest.rs
crates/blastit-manifest/tests/docs_fences.rs
crates/blastit-scheduler/src/config.rs
crates/blastit-sqlpolicy/src/learn.rs
```

The reader (`parse.rs`), `Manifest::from_toml_str` and the `toml` dependency are
4.5's, and so is this sweep of comments, fixtures and the protected crate
suites (which 4.1/4.2/4.3 own until it runs).

## 5. Follow-ups

- **Open risk for the epic gate: one `502` on the first `/api` request was seen
  once and never reproduced.** In a second full `make e2e` run (not the green one
  recorded in §3), `E7` — the first `POST` to the `api` view — answered
  `502 bad gateway` (the scheduler's `view_abort`), and every later request in
  that same run passed. It is **not** `MB.21.4.8`'s crate-side pairing flake: the
  same image replayed the whole matrix 3/3 times (`PASS … (139 checks)`), 8/8 cold
  boots answered that exact `POST` `200` locally, and the recorded run is green.
  Re-open this line if the epic gate sees a `502` on a view's first request.
- `plan/nodes/MB/MB.22/**` and `docs/research/**` are dirty in this worktree from
  **another session**: they are not this leaf's and must not be staged with it.
- The two `test_pytest_plugin.py` cases this leaf found red at the base
  (an unparsable file's exit code, and the repair's created view missing the
  trait) are fixed here, on the side that was wrong (§2(c)); nothing is left for
  a follow-up node.
- `docs/frameworks/{php,static-and-spa,django}.md` and the other pages still show
  the retired manifest openings and the `1 route` banner line: `MB.21.5.1`.
- The permanent `Blastfile`-only guard the epic names
  (`test_the_real_tree_carries_no_retired_toml_manifest`) does not exist yet:
  `MB.21.5.2`.
- `probes.md` row 23's `Outcome` column remains the verbatim §4.7 record: it is
  provenance, not a promise.

## Files

Added: `e2e/demo/Blastfile`, `e2e/php/fixture/Blastfile`,
`e2e/sqlalchemy/Blastfile`, `e2e/static_site/Blastfile`,
`e2e/static_site/nospa/Blastfile`, `e2e/venv-smoke/Blastfile`,
`python/tests/live_project/Blastfile`, `python/tests/packaged_project/Blastfile`,
this `RESULT.md`.

Changed: `Makefile`, `e2e/{Dockerfile,Dockerfile.prebuilt,compose.yaml,README.md,check_logs.py,grants.py,php2shell.py,static_assets.py,scenario.py}`,
`e2e/demo/notes/views.py`, `e2e/php/README.md`,
`e2e/php-sapi/run-unsupported-minor.sh`,
`e2e/rootless/{Dockerfile,Dockerfile.prebuilt,README.md,run.py,php_entry_swap.py}`,
`e2e/sqlalchemy/app.py`, `e2e/static_site/run-dev.sh`, `e2e/venv-smoke/run.sh`,
`python/tests/{README.md,test_check.py,test_live_server.py,test_project_manifest.py,test_pytest_plugin.py,test_runnable_docs.py}`,
`python/tests/project/Blastfile` (4.7's: `blastit fmt`, one blank line),
`scripts/{standalone-runtime-check.sh,wheel-smoke.sh}`.

Deleted: `e2e/demo/blastit.toml`, `e2e/sqlalchemy/blastit.toml`,
`e2e/static_site/blastit.toml`, `e2e/static_site/nospa/blastit.toml`,
`e2e/venv-smoke/blastit.toml`, `e2e/php/fixture/blastit.toml`,
`python/tests/live_project/blastit.toml`,
`python/tests/packaged_project/blastit.toml`,
`e2e/php/fixture/api/index.php` (its route is the one entrypoint now).

No `Cargo.toml`, no `Cargo.lock`, no `crates/**`, no published page, no plan
frontmatter (this file is the only `plan/` write).

One deliberate write outside the node's globs, the same shape `MB.21.4.7` used:
one line in `docs/knowledge/LESSONS.md` (the `static`-mount/§9.2-step-0 trap that
turned `E23` from `200` to `400`). `docs/knowledge/` is excluded from the site
(`zensical.toml:66`), from `doc-tone` (`scripts/check_doc_tone.py:45`) and from the
docs-assets guard, so no gate reads it.

