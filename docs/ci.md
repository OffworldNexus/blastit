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
| `missing_grant` | a statement no grant allows | add it to `queries` |
| `dead_grant` | a grant the suite never used | cover that code path, or remove the grant |
| `unexercised_route` | a route the suite never hit | add a test |
| `missing_ingress` | a route took a body and declares no `[route.ingress]` | declare the codec; `raw` also needs `allow_raw = true` |
| `ingress_mismatch` | the declared codec is not what the suite sent | fix the `body` value |
| `raw_ingress` | a route declares `body = "raw"` without acknowledging it | add `allow_raw = true`, or use a codec |
| `no_observations` | the suite reached no route or no proxy | check the test database and `DATABASE_URL` |
| `proxy_unused` | a declared proxy saw zero statements | fix the suite's URL, or remove the proxy by hand |

Production enforces the same declaration: a body to a route without
`[route.ingress]` is a `413` there, not a warning.

Deleting a route turns its paths into Blastit's `404`; deleting a grant denies the
statement in production. Neither is a safe automatic fix.

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

The summary counts findings, one line each — the indented statement is detail —
and `--json` carries the same six entries.

## For agents: `--json`

```console
$ uv run blastit check --json
```

```json
{
  "schema_version": 1,
  "ok": false,
  "requests": 412,
  "statements": 977,
  "routes": {"total": 8, "exercised": 7},
  "proxies": {
    "db": {"statements": 977, "denied": 1},
    "analytics": {"statements": 0, "denied": 0}
  },
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

`kind` is one of the nine above. `severity` is `error` on every kind in 0.1.1,
so any finding fails the run and sets `ok` to `false`; a `warning` is printed and
leaves the exit code and `ok` alone. `sql` is the normalised statement, exact and
complete, and never contains a literal value. A grant finding names its `route`
and `proxy`, and `fix.to` is that grant's table in the manifest:
`"[route.db].queries"` inside the route and proxy the finding names, so
`add_query` appends `sql` there and `remove_query` deletes it there. `proxies`
counts what each declared proxy saw: `statements` adds up to the total, `denied`
is how many no grant allowed, and a declared proxy with `"statements": 0` is
always a `proxy_unused` finding.

`fix.action` is `add_query`, `remove_query`, `add_route`, `remove_route`,
`add_test`, `declare_ingress`, `fix_ingress` or `observe`. A destructive `fix`
carries `"unsafe": true`: `remove_query` and `remove_route` delete a grant or a
route in production too. `declare_ingress` names the codec that fits what the
suite sent; when none does — a multipart upload in 0.1.1 — the same object
carries `"allow_raw": true`, or the route immediately reports `raw_ingress`.

`--json` prints exactly one document on stdout and nothing else: pytest output,
logs, progress and `learn:` lines go to stderr. On exit `2`, stdout is empty and
the error is on stderr. Under `--learn` the document is the proposal, not the
findings: [bootstrap](#bootstrap-an-existing-project).

## Exit codes

| Code | Meaning |
|---|---|
| `0` | clean |
| `1` | findings |
| `2` | Blastit could not start: a manifest or configuration error |
| `3` | the suite failed, including no tests collected |

## Your suite

`blastit check` runs `pytest -q`; install it with `uv add --dev pytest`. Django
suites use `pytest-django`, which [the Django page](frameworks/django.md)
installs.

Statements outside a routed request — migrations, fixtures, teardown — are not
audited; only what a request sends is. The test database is prepared first with
the real credentials, then the suite runs through the proxy.

Arguments after `--` go to pytest:

```console
$ uv run blastit check -- -k products
```

## Bootstrap an existing project

It needs a manifest that boots: `version`, `app`, `[proxy.db]` and one route.
[Getting started](getting-started.md) is the smallest. `--learn` discovers routes
and statements; `app`, `preload`, the proxy and its input variable names are
yours to write.

```console
$ uv run blastit check --learn              # record, write what was missing
$ uv run blastit check --learn --dry-run    # print the proposal, write nothing
$ uv run blastit check                      # prove it
```

For an agent, `--learn --json` prints exactly one document on stdout:

```json
{"proposed": {"routes": 2, "queries": 38}, "wrote": true}
```

Add `--dry-run` and the same document says `"wrote": false`, with nothing written.
A real `--learn` replaces the manifest atomically, and only after the suite
passed: a failed suite leaves the file untouched. It writes exact `queries`,
never table grants, and refuses a statement that inlines a value: parameterise it
first. Review `git diff` before committing.

## GitHub Actions

A PostgreSQL service, no root, no extra setup. Commit `uv.lock`: the workflow
runs `uv sync --frozen`.

```yaml title=".github/workflows/check.yml"
name: check
on: [push, pull_request]

permissions:
  contents: read

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