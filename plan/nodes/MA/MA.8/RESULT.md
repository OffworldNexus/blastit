# Result MA.8

STATUS: done

The two red `blastit check` tests were **not** caused by MA.6's hot-path backport.
Both failed on one and the same audit finding, a correct `missing_grant` for
Django's own connection-setup statement, which only appears when the PostgreSQL
image spells the session time zone `Etc/UTC` instead of `UTC` — `postgres:18`
(Debian, the CI service) does; the `postgres:15-alpine` server the tests were
authored against did not. The finding is right (in production that statement
would be denied on the route too); the test fixture was incomplete.

Attempt 1 fixed that by attaching a `connection` group **unconditionally**, which
just moved the red to the other image: where the statement never runs, the same
group is a `dead_grant` **error** (`audit_v1.rs:799-866`, `dead_group_grants`),
so `postgres:15-alpine` went red. Attempt 2 decides per server, from the same
`parameter_status("TimeZone")` Django reads, and both images are green with the
statement still observed and counted — nothing hidden, no assertion relaxed.

## Findings

Environment: `postgres:18` on `127.0.0.1:55450` and `postgres:15-alpine` on
`127.0.0.1:55451`, both `POSTGRES_USER=blastit POSTGRES_PASSWORD=ci-not-secret
POSTGRES_DB=blastit`; wheel built from the working tree, Django 6.1.3, psycopg
3.3.9, pytest 9.2.0, `/tmp/opencode/ma8/venv/bin/pytest`. Documents:
`/tmp/opencode/ma8a/{base,pre,post}-{18,15}/*.json` (base = pre-attempt-1
manifest, pre = attempt-1 code, post = this attempt).

**F1 — the original finding (the CI red), `postgres:18` only.**

| field | value |
|---|---|
| `kind` | `missing_grant` |
| `severity` | `error` |
| `route` | `home` |
| `proxy` | `db` |
| `sql` | `SELECT set_config('TimeZone', $?, false)` |
| `fix` | `{"action": "add_query", "to": "[route.db].queries"}` |

```json
{
  "schema_version": 1, "ok": false, "requests": 1, "statements": 3,
  "routes": {"total": 1, "exercised": 1},
  "proxies": {"db": {"statements": 3, "denied": 1}},
  "findings": [{"kind": "missing_grant", "severity": "error", "route": "home",
                "proxy": "db", "sql": "SELECT set_config('TimeZone', $?, false)",
                "fix": {"action": "add_query", "to": "[route.db].queries"}}]
}
```

**F2 — attempt 1's own regression, on `postgres:15-alpine`** (the control that
made this review finding real):

```json
{
  "schema_version": 1, "ok": false, "requests": 1, "statements": 2,
  "proxies": {"db": {"statements": 2, "denied": 0}},
  "findings": [{"kind": "dead_grant", "severity": "error", "route": "home",
                "group": "connection", "proxy": "db",
                "sql": "SELECT set_config('TimeZone', $?, false)",
                "fix": {"action": "remove_query",
                        "to": "[group.connection.db].queries", "unsafe": true}}]
}
```

which is exactly what `dead_group_grants` (`crates/blastit/src/audit_v1.rs:799-866`)
must emit: an attached group grant that no exercised route matched is an error,
not a harmless extra. The statement is *absent* from `matched` because Django
never sent it, and `denied: 0` proves the route had nothing to deny.

## Root cause (evidence, in order)

1. **Django sends the statement; the image decides whether it does.**
   `django/db/backends/postgresql/operations.py:197` (`set_time_zone_sql`) returns
   `SELECT set_config('TimeZone', %s, false)` and
   `postgresql/base.py:361-372` (`_configure_timezone`, called from
   `init_connection_state` inside `DatabaseWrapper.connect()`) executes it when
   `connection.info.parameter_status("TimeZone") != self.timezone_name`. With
   `USE_TZ = True` and `settings_dict["TIME_ZONE"] is None`
   (`django/db/utils.py:171`), `timezone_name` is `UTC`
   (`base/base.py:158-167`). Measured against these containers:
   ```
   $ psycopg.connect(DATABASE_URL).info.parameter_status("TimeZone")
   postgres:18 (18.6 Debian)       -> 'Etc/UTC'   (the CI service)
   postgres:15-alpine (15.19)      -> 'UTC'       (what MA.2.3.5 ran against)
   ```
   So on `postgres:18` every new connection runs one extra statement; on the
   alpine 15 the comparison matched and it never ran. That is why the tests were
   green for their author and red in the CI job — and why a grant for it is only
   correct on one of the two images.
2. **The audit charges it to the route on purpose.**
   `crates/blastit-dbproxy/src/learn.rs` ("Session setup"): a driver's
   connection-setup statements "are recorded per session … and replayed **once**,
   against the first target the session reports — routed or unrouted, because
   either one is a request whose route needs the same grants when it reuses the
   connection". `blastit check`'s observations have no `connection_setup`
   channel at all (`crates/blastit-py/src/audit.rs` defines it only for the
   pytest *plugin*; `CheckObsV1::from_jsonl` in `crates/blastit/src/check.rs`
   feeds only requests), and with `CONN_MAX_AGE = 0` the connection is opened
   *inside* the request anyway. `crates/blastit/src/audit_v1.rs:604-620` then
   asks the same `Allowlist` dbproxy enforces (`blastit-sqlpolicy`), so the
   finding is exactly what `blastit dev`/`serve` would deny on the first
   request. Hiding it would make `check` green and production red — a DX
   regression, not a fix.
   The statement is grantable: it is one of `KNOWN_STATIC_SETUP_SQL`
   (`crates/blastit-sqlpolicy/src/literals.rs:42-45`), and the documented fix for
   "connection setup runs on whichever route opens the connection" is a group
   attached to every route (`docs/reference/manifest.md:10-49`).
3. **Not the backport.** The same capture was run against a wheel built at
   `7dfa863` (the commit MA.6 was merged onto) and against the current tree,
   both with the pre-fix manifest: **byte-identical reports**, findings included
   (`dbsuite`, `atomic-denied`, `atomic-granted` all `True` for `a == b`; see
   `/tmp/opencode/ma8/prefix/` vs `prefix-head/`). `4493699`'s undecoded head,
   the in-place response-head encoding and the pooled view threads change no
   route, statement, grant or proxy this suite observes: `requests`, `statements`,
   `routes` and `proxies` are identical across the backport.

## Fix (test fixture, and why it must be conditional)

`python/tests/test_check.py`:

- `DJANGO_MANIFEST` — `MANIFEST` plus `groups = ["connection"]` and

  ```toml
  [group.connection.db]
  queries = ["SELECT set_config('TimeZone', $1, false)"]
  ```

  placed **before** `[[route]]` so `[route.db]` stays the last table (the atomic
  granted twin appends `transaction_control = true` to it). The doc comment above
  it now says the grant is conditional on the server's startup zone spelling, and
  why granting it where it never runs is a `dead_grant` error rather than extra
  safety.
- `_django_manifest()`, next to `_upstream_db_and_password`: opens one
  `psycopg.connect(DATABASE_URL)` and returns `MANIFEST` when
  `connection.info.parameter_status("TimeZone") == "UTC"`, else `DJANGO_MANIFEST`
  (a `None` answer also means Django sends the statement). Both tests call it —
  `django_project(..., manifest=_django_manifest())` and
  `atomic_manifest = _django_manifest().replace(...)`.
- Failure messages no longer hide the report: the four `returncode` asserts in the
  `pytest-django` DB tests now read `f"{completed.stderr}\n{completed.stdout}"`
  (db-suite `:439`, denied-grant `:481`, atomic denied `:572`, atomic granted
  `:601`). That missing JSON is why this node had to reproduce the failure by
  hand.

No assertion was relaxed, skipped or xfailed: the db-suite test still requires
`findings == []`, the atomic test still requires `missing_grant SAVEPOINT` in the
denied twin and `findings == []` in the granted one, on **both** images. No
`crates/` change was needed — the engine's behaviour is correct on both servers.

## Acceptance

### 1. Root cause reproduced on both images (pre-fix, full JSON)

`postgres:15-alpine` with attempt-1's code (unconditional group) — red, F2:

```
$ DATABASE_URL=postgresql://blastit:ci-not-secret@127.0.0.1:55451/blastit \
      /tmp/opencode/ma8/venv/bin/pytest python/tests/test_check.py -q \
      -k "django_db_suite or atomic_view"
FAILED python/tests/test_check.py::test_pytest_django_db_suite_has_zero_teardown_warnings_and_drops_test_db
FAILED python/tests/test_check.py::test_pytest_django_atomic_view_still_enforces_transaction_control
2 failed, 8 deselected in 2.97s
```

`postgres:18` with attempt-1's code (same as post-fix there) — green; and with
the **pre-attempt-1** manifest (`MANIFEST`) — red, F1. Both captured by
`/tmp/opencode/ma8a/capture2.py` (`MA8_MANIFEST=plain|django|auto`) into
`/tmp/opencode/ma8a/{base,pre,post}-{18,15}/`:

```
[pg18] # parameter_status(TimeZone)='Etc/UTC'
[pg18] ===== dbsuite: rc=1 ok=False requests=1 statements=3 proxies={"db": {"statements": 3, "denied": 1}}
[pg18]   {"fix": {"action": "add_query", "to": "[route.db].queries"}, "kind": "missing_grant", "proxy": "db", "route": "home", "severity": "error", "sql": "SELECT set_config('TimeZone', $?, false)"}
[pg15] # parameter_status(TimeZone)='UTC'
[pg15] ===== dbsuite: rc=0 ok=True requests=1 statements=2 proxies={"db": {"statements": 2, "denied": 0}}
[pg15]   findings == []
```

i.e. the base manifest is red on 18 and green on 15; attempt-1's unconditional
grant is green on 18 and red on 15 (`dead_grant`, above). No single static
manifest satisfies both — hence the per-server helper.

### 2. Post-fix: both images green, same statements, zero findings

`MA8_MANIFEST=auto` (the code under test) on each image:

```
[pg18] # parameter_status(TimeZone)='Etc/UTC'   base_has_group=True
[pg18] ===== dbsuite: rc=0 ok=True requests=1 statements=3 proxies={"db": {"statements": 3, "denied": 0}}
[pg18]   findings == []
[pg18] ===== atomic-denied: rc=1 ok=False requests=1 statements=5 proxies={"db": {"statements": 5, "denied": 2}}
[pg18]   {"kind": "missing_grant", "sql": "RELEASE"} | {"kind": "missing_grant", "sql": "SAVEPOINT"}
[pg18] ===== atomic-granted: rc=0 ok=True requests=1 statements=5 proxies={"db": {"statements": 5, "denied": 0}}
[pg18]   findings == []
[pg18] # test_db 'test_blastit' exists=False

[pg15] # parameter_status(TimeZone)='UTC'        base_has_group=False
[pg15] ===== dbsuite: rc=0 ok=True requests=1 statements=2 proxies={"db": {"statements": 2, "denied": 0}}
[pg15]   findings == []
[pg15] ===== atomic-denied: rc=1 ok=False requests=1 statements=4 proxies={"db": {"statements": 4, "denied": 2}}
[pg15]   {"kind": "missing_grant", "sql": "RELEASE"} | {"kind": "missing_grant", "sql": "SAVEPOINT"}
[pg15] ===== atomic-granted: rc=0 ok=True requests=1 statements=4 proxies={"db": {"statements": 4, "denied": 0}}
[pg15]   findings == []
[pg15] # test_db 'test_blastit' exists=False
```

Summary table:

| image | `parameter_status('TimeZone')` | manifest used | db-suite `statements`/`denied`/`findings` | atomic-granted `statements`/`denied`/`findings` |
|---|---|---|---|---|
| `postgres:18` | `Etc/UTC` | `DJANGO_MANIFEST` (`connection` group) | 3 / 0 / `[]` | 5 / 0 / `[]` |
| `postgres:15-alpine` | `UTC` | `MANIFEST` (no group; statement never sent) | 2 / 0 / `[]` | 4 / 0 / `[]` |

One statement fewer on 15 in both cases, and no `connection` grant in play there;
the atomic twins still report the intended `SAVEPOINT`/`RELEASE` `missing_grant`
on both images, so the transaction-control contract is still enforced.

### 3. The whole file, both images

```
$ DATABASE_URL=postgresql://blastit:ci-not-secret@127.0.0.1:55450/blastit \
      /tmp/opencode/ma8/venv/bin/pytest python/tests/test_check.py -q
..........                                                               [100%]
10 passed in 8.42s

$ DATABASE_URL=postgresql://blastit:ci-not-secret@127.0.0.1:55451/blastit \
      /tmp/opencode/ma8/venv/bin/pytest python/tests/test_check.py -q
..........                                                               [100%]
10 passed in 8.55s
```

The CI job's own line (`pytest python/tests -q --ignore=python/tests/runnable_docs/fixtures`)
was run in attempt 1 (`3 failed, 365 passed, 1 skipped`); the three are
`test_live_server.py` and are **host-only** (blastit refuses to boot a view uid
inside `/home/remy`, `mode 0700 is not traversable by uid 21000`, the fence in
`docs/knowledge/LESSONS.md`); they passed in the failing CI run. `test_check.py`
is untouched by them.

### 4. `make check` and the critical guard

```
$ make check
…
38 fences: 26 executed, 12 accounted, 0 failed · 17.2s
uv run --no-project tools/plan.py validate
plan OK
EXIT:0
```

(`guard: OK (3 staged/untracked file(s) checked)`, every `cargo test` line
`test result: ok`, `grep -cE "^(FAIL|ERROR)|test result: FAILED"` over the log →
`0`. `MA` and `MA.8` are both `active`, so `plan-validate` now passes.)

```
$ uvx --offline ruff check python tools && uvx --offline ruff format --check python tools
All checks passed!
61 files already formatted
EXIT:0

$ make critical-guard BASE=f97fd08
critical-guard: f97fd08..working tree, 6 changed file(s)
  no change to a protected critical test
EXIT:0
```

### 5. `smoke (root)`

**Answered — "not green".** Job `131823878765` (run `43407063422`, PR #62) began 10:10:10Z,
`in_progress`, `conclusion: null`. Steps 1–9 finished through
`cargo build -p blastit --tests --locked` (10:10:54–10:12:02). Step 10 — MA.7's `sudo env`
`cargo test -p blastit --locked -- --test-threads=1` — began 10:12:02Z and ran 35+ min
without finishing (`Stop containers`, posts pending): past the sudo-rs lookup, then hung, so
it never goes green — it sits until GitHub's 360-minute default job timeout.

Cause unnamed: an in-flight job has no live log (`gh api …/jobs/131823878765/logs` → 404
`BlobNotFound`). For scale, the pre-MA.7 step on `develop` `2c05139` (run `40469910586`)
took 5 s (`success`) under classic sudo `-E` — not like-for-like, since sudo-rs drops `-E`.

`.github/workflows/ci.yml` is outside these `paths`, so this is the conductor's, not this
node's: `timeout-minutes:` on the job or step, and `--nocapture` / `2>&1 | tee`, so a hang
fails loudly with a partial log instead of pinning the PR. The `pytest plugin` fix — this
node's deliverable — is independent of `smoke (root)`.

## Files

- `python/tests/test_check.py` — `DJANGO_MANIFEST` (+ doc comment),
  `_django_manifest()`, its two call sites, and the four `returncode` messages
  that now include `stdout`.
- Nothing under `crates/`, `docs/` was modified, and no plan status was
  hand-edited: only `plan.py` status/ledger writes (`plan/JOURNAL.md`,
  `plan/BUDGET.md`, `plan/nodes/MA/{NODE,LOG}.md`, `plan/STATE.md`) came from the
  plan tool. No critical test touched (`make critical-guard` above).

## Follow-ups

- **DX (worth its own node):** when the connection-setup statement is charged to
  a route, `check` proposes `fix.action = add_query` to that one route's
  `queries`. The statement runs on whichever route opens the connection first,
  so a multi-route app would be told to grant it on route #1 only — and the other
  routes would still deny it in production. The documented fix is one
  `connection` group attached to every route; a `fix.action = "add_group"`
  (`[group.connection.db]`) would be the actionable form. Out of scope for a
  release hotfix (it changes the `--json` contract), so filed here, not changed.
- **DX, same shape as F2:** a `dead_grant` whose `fix` is
  `remove_query(..., unsafe: true)` has two very different causes — an obsolete
  grant, or a grant for a statement this server's driver/image happens not to
  send. Worth saying so in the finding's message.
- **`docs/knowledge/LESSONS.md`** is outside this node's `paths`, so this line is
  for the lead to append: *"Django 6.1's PostgreSQL backend sends `SELECT
  set_config('TimeZone', $1, false)` at connect whenever the server's startup
  `TimeZone` differs from Django's `UTC` — `postgres:18` (Debian) reports
  `Etc/UTC`, `postgres:15-alpine` reports `UTC`, so a suite green on one image
  gains a `missing_grant` on a route on the other; grant it with a
  `groups = ["connection"]` attached to every route, not one route's `queries`,
  and only on servers whose `parameter_status('TimeZone') != 'UTC'` (an
  unconditional grant is a `dead_grant` error on the other)."*
- `test_live_server.py` cannot run on this host (`/home/remy` is 0700, view uid
  21000 cannot traverse it). Pre-existing, environmental, unrelated.
