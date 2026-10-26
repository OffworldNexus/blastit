# pytest plugin

Diffs what your test suite really did against the manifest, before you deploy, so
grants are not discovered one deploy at a time.

!!! warning "Django only"

    "A request" is Django's `BaseHandler.get_response`. For Flask and other WSGI
    apps, `blastit-manifest fold` is the answer.

## Options

Inert unless asked: `uv run pytest --blastit-manifest blastit.toml`. Without the
manifest, no test sees a difference, and the other options are an error rather
than ignored.

| Option | Default | What it does |
|---|---|---|
| `--blastit-manifest PATH` | — | the `blastit.toml` to diff against; without it the plugin does nothing |
| `--blastit-report PATH` | `blastit-report.json` | where the JSON report goes |
| `--blastit-proxy ALIAS=PROXY` | — | maps a Django database alias to a manifest proxy; repeatable |
| `--blastit-strict` | off | fail the session on anything missing, unrouted or in need of attention |
| `--blastit-prefer-tables` | off | print the table-grant snippet, not just the missing statements |
| `--blastit-live-server` | off | serve pytest-django's `live_server` through blastit itself (needs root) |
| `--blastit-live-app MODULE:ATTR` | — | the WSGI callable that live server serves |

The manifest can live in `pytest.ini`: `blastit_manifest = blastit.toml`. It
needs pytest-django and psycopg 3 with server-side binding.

## The report

It records every request Django handled, every statement it sent with its real
`$n` placeholders, and every outbound connection it tried to open. Statements
outside a request — fixtures, teardown — are ignored. pytest-xdist works: each
worker reports, the controller merges.

At the end of the session, a summary and `blastit-report.json`:

```text
blastit: 412 requests, 977 statements, 38/41 routes exercised — report in blastit-report.json
blastit: route product-list: 3 statement(s) missing
blastit: route checkout: needs transaction_control = true
blastit: route queue opens outbound connections (redis:6379) but has no egress = true
blastit: 4 unrouted request(s): GET /robots.txt, GET /favicon.ico
```

Per route, in the report:

* `missing` — statements no grant allows. `sql` is the normalised text, ready to
  paste into `queries`. **A statement your tests never sent can still be missing
  in production: this is a lower bound.**
* `unused` — grants nothing matched: dead code, or a request your suite does not
  make yet.
* `transaction_control` — the route ran `BEGIN`/`COMMIT` or a savepoint without
  the grant. Add `transaction_control = true`; never try to grant a savepoint.
* `table_grants` — the smallest `[route.<proxy>.tables]` grant covering every
  statement the pair ran, with the TOML to paste in.
* `egress` — the connections the route opened, and `needed` when it has no
  `egress = true`.
* `unrouted` — paths blastit answers `404` for before Django sees them.

Across routes:

* `unexercised_routes` — routes nothing hit: their grants are unproven.
* `connection_setup` — the statements psycopg runs as a connection is
  established. Every request opens its own connection, so put them in a group
  attached to every route.
* `unused_global` — grants from a top-level group that no route matched.
* `unrecognised` and `plugin_warnings` — what the plugin could not read or do.
* `totals` — the counts, including `routes_exercised` and `clean`.

`--blastit-strict` fails on any of it. That is the CI job that keeps the manifest
from rotting.

!!! warning "It only fails on what the suite reached"

    A session that routed no request reports clean. Read `routes_exercised`
    before you trust a green run.

## Connection-setup statements are not `unused_global`

A statement that only runs while a connection is being established — psycopg's
`set_config('TimeZone', …)` — is no route's request statement, so it lands under
`connection_setup`, with `granted_in` and `ungranted`. `ungranted: 0` is what
`--blastit-strict` acts on.

## The live server

`blastit_live_server` starts the real blastit binary with your manifest and your
test database, and hands you the URL:

```python
from urllib.request import urlopen


def test_notes_page(blastit_live_server):
    with urlopen(f"{blastit_live_server.url}/notes/") as response:
        assert response.status == 200
```

Run it with `--blastit-live-server`, or in `conftest.py`:

```python
from blastit.pytest_plugin import blastit_live_server as live_server
```

On teardown, the server's log is folded into the same observations, so a grant
the front end needs shows up as a missing statement. It needs root, or
passwordless sudo, and skips with the reason otherwise. A route only the live
server saw reports no unused grants.
