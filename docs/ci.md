# CI/CD

One command, no root, any WSGI framework:

```console
$ uv run blastit check
```

It runs `pytest -q` with Blastit in front of PostgreSQL: every request the suite
makes and every statement it sends is checked against `./blastit.toml`. A clean
run means every route and every declared proxy was observed, and nothing **your
suite exercised** was forgotten.

## What it fails on

| Finding | Meaning | Fix |
|---|---|---|
| `unrouted_request` | a path no route matches | add a route, or widen a `path` |
| `missing_grant` | a statement no grant allows | add it to `queries`; session setup (`SET`, `SHOW`, `set_config(…)`) to the `connection` group |
| `dead_grant` | a grant the suite never used | cover that code path, or remove the grant |
| `unexercised_route` | a route the suite never hit | add a test |
| `missing_ingress` | a route took a body and declares no `[route.ingress]` | declare the codec; `raw` also needs `allow_raw = true` |
| `ingress_mismatch` | the declared codec is not what the suite sent | fix the `body` value |
| `raw_ingress` | a route declares `body = "raw"` without acknowledging it | add `allow_raw = true`, or use a codec |
| `no_observations` | the suite reached no route or no proxy | check the test database and `DATABASE_URL` |
| `proxy_unused` | a declared proxy saw zero statements | fix the suite's URL, or remove the proxy by hand |

Production enforces the same declaration: a body to a route without
`[route.ingress]` is a `413` there, not a warning. Deleting a route or a grant is
never safe automatically: the path becomes `404`, the statement is denied.

## What you see

```console
$ uv run blastit check
blastit check: 412 requests, 977 statements, 7/8 routes, 2 proxies
  route catalog: 1 statement not granted
    SELECT "shop_product"."price" FROM "shop_product"
  route search: 1 grant never used
  route checkout: never exercised
  route upload: takes a body and declares no ingress validation
  request GET /robots.txt: no route
  proxy analytics: declared, 0 statements observed
blastit check: 6 problems · `--json` for the machine-readable list
```

One finding per line, the indented statement as detail; `--json` below lists the same six.

## For agents: `--json`

```console
$ uv run blastit check --json
```

```json
{
  "schema_version": 1, "ok": false,
  "requests": 412, "statements": 977,
  "routes": {"total": 8, "exercised": 7}, "proxies": {"db": {"statements": 977, "denied": 1}, "analytics": {"statements": 0, "denied": 0}},
  "findings": [
    {"kind": "missing_grant", "severity": "error", "route": "catalog", "proxy": "db", "sql": "SELECT \"shop_product\".\"price\" FROM \"shop_product\"", "fix": {"action": "add_query", "to": "[route.db].queries"}},
    {"kind": "dead_grant", "severity": "error", "route": "search", "proxy": "db", "sql": "SELECT count(*) FROM \"shop_product\"", "fix": {"action": "remove_query", "to": "[route.db].queries", "unsafe": true}},
    {"kind": "unexercised_route", "severity": "error", "route": "checkout", "fix": {"action": "add_test"}},
    {"kind": "missing_ingress", "severity": "error", "route": "upload", "observed": ["multipart/form-data"], "fix": {"action": "declare_ingress", "codecs": ["raw"], "allow_raw": true}},
    {"kind": "unrouted_request", "severity": "error", "method": "GET", "path": "/robots.txt", "fix": {"action": "add_route"}},
    {"kind": "proxy_unused", "severity": "error", "proxy": "analytics", "fix": {"action": "observe"}}
  ]
}
```

`severity` is `error` on every kind in 0.1.1, so any finding fails the run and
`ok` is `false`; a `warning` leaves the exit code alone. `sql` is the normalised
statement, never a literal value, and a grant finding's `fix.to` is its table in
the manifest: `"[route.db].queries"`. `fix.action` is `add_query`, `add_group`,
`remove_query`, `add_route`, `remove_route`, `add_test`, `declare_ingress`,
`fix_ingress` or `observe`; a destructive one carries `"unsafe": true`. `add_group`
fits only `SET`, `SHOW` or `set_config('TimeZone', …)`; another `set_config(…)` is
`observe`. Setup runs on whichever route connects first, so `fix.to` names a group:

```toml
groups = ["connection"]                 # to every route
[group.connection.db]
queries = ["SELECT set_config('TimeZone', $?, false)"]
```

`--json` writes one document to stdout and nothing else: pytest output, logs and
`learn:` lines go to stderr, exit `2` leaves stdout empty, and under `--learn` it
is the proposal ([below](#bootstrap-an-existing-project)).

## Exit codes

`0` clean · `1` findings · `2` could not start (manifest, config, no `pytest`) · `3` suite failed.

## Your suite

`blastit check` runs `pytest -q`; without `pytest` it exits `2` (`uv add --dev
pytest`). With `pytest-django` importable **and** `DJANGO_SETTINGS_MODULE` or a
`./manage.py`, migrations and fixtures use the `test_*` database, never your
development one. Statements outside a routed request — migrations, fixtures,
teardown — are not audited; only a request's are. Arguments after `--` go to pytest:

```console
$ uv run blastit check -- -k products
```

## Bootstrap an existing project

It needs a manifest that boots: `version`, `app`, `[proxy.db]` and one route.
[Getting started](getting-started.md) is the smallest. `--learn` discovers routes
and statements; `app`, `preload` and the proxy's input names are yours to write.

```console
$ uv run blastit check --learn              # record, write what was missing
$ uv run blastit check --learn --dry-run    # print the proposal, write nothing
$ uv run blastit check                      # prove it
```

For an agent, `--learn --json` prints exactly one document on stdout:

```json
{"proposed": {"routes": 2, "queries": 38}, "wrote": true}
```

Add `--dry-run` and the document says `"wrote": false`, with nothing written. A
real `--learn` replaces the manifest atomically and only after the suite passed,
writes exact `queries` (never table grants) and refuses a statement that inlines a
value: parameterise it first. Review `git diff` before committing.

## GitHub Actions

A PostgreSQL service, no root, no extra setup; commit `uv.lock` (the workflow runs `uv sync --frozen`).

```yaml title=".github/workflows/check.yml"
name: check
on: [push, pull_request]
permissions: {contents: read}

jobs:
  blastit:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:18
        env:
          POSTGRES_PASSWORD: postgres
          POSTGRES_DB: shop
        ports: ["5432:5432"]
        options: >-
          --health-cmd pg_isready --health-interval 5s
          --health-timeout 5s --health-retries 20
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v10.3.2
      - run: uv sync --frozen
      - run: uv run blastit check --json
        env:
          DATABASE_URL: postgresql://postgres:postgres@127.0.0.1:5432/shop
```
