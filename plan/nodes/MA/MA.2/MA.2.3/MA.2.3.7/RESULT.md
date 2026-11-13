# RESULT MA.2.3.7 — re-pin `test_pytest_plugin.py` against Django 6.1 and R11

STATUS: done

Files changed (both inside `paths`):

- `python/tests/test_pytest_plugin.py` — connection-setup expectations now driven by
  what the fixture actually makes PostgreSQL send, the R11 egress warning pinned to
  its real text, and the "report → manifest" loop states the one statement no grant
  may hold (`ADR-0003`/`R24`) instead of asserting a convergence that cannot happen.
- `python/tests/project/blastit.toml` — unchanged: the warning is documented R11
  behaviour, so the *test* was pinned to it rather than the fixture silenced
  (node `# Notes`: "the fixture is the part that drifted" — pinning was allowed and
  keeps the warning demonstrated in the fixture that documents it).

## What the seven red tests were, and what they now assert

1. `test_connection_setup_names_the_routes_that_do_not_grant_it`: the fixture always
   sends `SET application_name = 'blastit-test'` (its `connection_created` receiver,
   `python/tests/project/notes/apps.py`); Django 6.1 adds
   `SELECT set_config('TimeZone', $?, false)` **only** when the server's `TimeZone`
   is not the `UTC` it asks for (`USE_TZ=True`, no per-alias `TIME_ZONE`). Both are
   pinned by exact text, every unexpected third statement fails the `known` set
   check, and the totals are `len(setup_statements())` rather than a hard `2`.
2. `manifest.warnings` is pinned to
   `['route "note-cache": egress = true opens any outbound address and port']`.
3. Terminal summary / `--blastit-strict` counts / both xdist tests derive the
   connection-setup number from the report instead of assuming 2.
4. `test_more_than_one_proxy_needs_a_mapping` pins `application_name` (guaranteed by
   the fixture) instead of the optional `set_config`.

### Discovered while making it green (latent, hidden behind assert 1)

The fixture app sends `SET application_name = 'blastit-test'` with the value
inlined, and `Manifest::validate` refuses any `queries` entry with an inline quoted
literal outside `KNOWN_STATIC_SETUP_SQL` (`ADR-0003`, `R24`). So that one statement
can never be granted, and `test_the_report_is_enough_to_fill_the_manifest_in` could
not end `clean` — it previously died at its first assertion, before reaching the
second session. The test now says so exactly instead of pretending:
`repaired_manifest()` leaves a non-grantable statement out (`may_be_granted`), the
second run must report exactly that one (`ungranted_setup == [APP_SETUP_SQL]`), and
the third run pins **every** other total at zero plus the strict line
`0 … missing, 0 unrouted, 1 connection-setup … not granted`, exit `1`. No assertion
was deleted without a stronger replacement; no `skip`/`xfail` added.

## Acceptance evidence

Env: throwaway venv on CPython 3.14.10 (`blastit._blastit` staged from
`target/python-critical`, dbproxy from `target/debug`), `django==6.1.3`,
`pytest==9.2.0`, `pytest-django==4.14.0`, `pytest-xdist==3.8.0`, `psycopg==3.3.9`
(uv offline cache — the versions the `pytest-plugin` job resolves). PostgreSQL
`postgres:15-alpine` in Docker on 127.0.0.1:55437. `cargo`/`make` were **not** run
(the target lock belongs to the concurrent Rust node), so the wheel is staged by
hand from the existing build.

```console
$ DATABASE_URL=postgresql://blastit:ci-not-secret@127.0.0.1:55437/blastit \
    /tmp/opencode/m237v2/bin/python -m pytest python/tests/test_pytest_plugin.py -q
.......................................                                  [100%]
39 passed in 33.24s
```

Second zone, to prove the conditional connection-setup pin holds in **both** worlds
(same server class, `TZ=Europe/Paris`, so Django 6.1 does send `set_config`):

```console
$ docker exec m237-pgtz psql -U blastit -d blastit -tAc "show TimeZone"
Europe/Paris
$ DATABASE_URL=postgresql://blastit:ci-not-secret@127.0.0.1:55438/blastit \
    /tmp/opencode/m237v2/bin/python -m pytest python/tests/test_pytest_plugin.py -q
.......................................                                  [100%]
39 passed in 33.28s
```

The two branches, from the fixture project run directly (one session each):

```console
# UTC server  -> totals.connection_setup = 1
"SET application_name = 'blastit-test'" ungranted_in= 8
manifest.warnings= ['route "note-cache": egress = true opens any outbound address and port']
# Europe/Paris -> totals.connection_setup = 2
"SELECT set_config('TimeZone', $?, false)" ungranted_in= 8
"SET application_name = 'blastit-test'"    ungranted_in= 8
```

`test_check.py` in the same env:

```console
$ DATABASE_URL=…@127.0.0.1:55437/blastit … pytest python/tests/test_check.py -q
..........                                                               [100%]
10 passed in 13.00s
```

Combined run the brief asked for:

```console
$ DATABASE_URL=…@127.0.0.1:55437/blastit … pytest python/tests/test_live_server.py python/tests/test_check.py -q
3 failed, 12 passed in 52.12s
FAILED python/tests/test_live_server.py::test_a_packaged_settings_module_is_served
FAILED python/tests/test_live_server.py::test_a_live_server_is_served_by_blastit
FAILED python/tests/test_live_server.py::test_the_live_server_option_serves_a_live_server_suite
```

`test_live_server.py` is red on **this host only**, for a reason outside this node
and outside the files it touches (it imports `blastit.pytest_plugin`, never
`test_pytest_plugin.py`, and serves `python/tests/live_project`, never
`python/tests/project`). The server boots and answers `/_blastit/health` 200, then
every route times out queueing — `queue_timeout` after 10 s, status 503. The app
itself is not the problem: the view uid imported the project and wrote
`__pycache__` into the venv (those files are not removable by the test user), and
no `app.output` error is logged — what never happens is a worker picking the
request up. Identical with production `sudo -n` (`"privilege":"root"`) and with
forced `blastit dev` (`"privilege":"unprivileged-dev"`), so it is not the
privilege branch.

```console
$ tail -4 /tmp/pytest-of-remy/pytest-181/test_the_live_server_override_0/blastit-live.log
{"event":"boot","component":"init","stage":"preload","privilege":"unprivileged-dev",…}
{"event":"ready","component":"init","stage":"serving","routes":2,"workers":16,…}
{"event":"request","method":"GET","path":"/_blastit/health","status":200,…}
{"event":"request","method":"GET","path":"/live/denied/","outcome":"queue_timeout","queued_ms":10001.8,"status":503,…}
```

Flagged for the lead / MA.2.3.6 rather than papered over here.

Lint:

```console
$ uvx --offline ruff check python/tests/test_pytest_plugin.py
All checks passed!
$ uvx --offline ruff format --check python/tests/test_pytest_plugin.py
1 file already formatted
```

## Follow-ups (out of scope for MA.2.3.7)

1. `python/tests/project/notes/apps.py` should stop sending a literal-bearing SQL
   statement at connect (e.g. set `application_name` through the connection's own
   options instead of `SET application_name = 'blastit-test'`), so
   `test_the_report_is_enough_to_fill_the_manifest_in` can go back to ending
   `clean`/exit 0 for real. Today `ADR-0003`/`R24` make any app-defined literal
   SET un-grantable; the test now names that instead of hiding it.
2. DX gap worth a docs/plugin task: `connection_setup` (and `missing`) list a
   statement a grant may never hold without saying so — the developer only learns
   it from the manifest validator's `LiteralQuery` error. `--learn`/`fold` already
   refuse such statements; the pytest report could say "bind the value" too.
3. `test_live_server.py` on this host: scheduler answers 503 `queue_timeout` for
   every route (see above) — needs CI/lead confirmation before anyone blames the
   test.

### Knowledge candidates (not written: `docs/knowledge/LESSONS.md` is outside `paths`)

- Django 6.1 `DatabaseWrapper.timezone_name` is `UTC` when `USE_TZ=True` and the
  alias has no `TIME_ZONE`, and `_configure_timezone` only sends
  `SELECT set_config('TimeZone', …)` when the server's `TimeZone` differs: the
  connection-setup statement set depends on the **server's** zone, so any test that
  counts it must be driven by the report.
- `blastit-manifest` refuses a grant whose `queries` entry contains an inline quoted
  literal (only `SET TIME ZONE 'UTC'` and `SELECT set_config('TimeZone', $?, false)`
  are exempt), so an app that spells a value in its connect-time SQL can never have
  that statement granted.
