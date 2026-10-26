# Day-to-day life

Two things change: a new URL needs a route, and a new query needs a grant.
Everything else is unchanged — you still run `manage.py`, `pytest` and your
container.

## Adding a view

A new view under an existing URL prefix needs no manifest change: the route
matches the prefix.

`path = ['/products/', '/products/[0-9]+/']` does not match
`/products/42/reviews/`, so widen it to `['/products/.*']` — or give the reviews
page its own route when it needs different grants. A new route is a new process
and a new uid, and the manifest is read at boot: restart blastit on each change.

## A denial in development

Run blastit with `BLASTIT_LOG_FORMAT=pretty` and click around:

```console
15:41:02.118 WARN  sql.denied  products  db  parse  SELECT "shop_product"."price" FROM "shop_product" …
```

The JSON form carries more: `route` and `proxy` name the sandbox that refused
it, `sql` is the statement as normalised — what a grant has to contain — and
`reason` is the policy's explanation, never a parameter value.

Read the `reason` before the `sql`:
`select on shop_product.price not granted` is a table grant missing a column,
and `the statement is not granted` is a statement nobody listed at all.

Two answers:

* **The statement belongs there** — paste the normalised `sql` into `queries`.
* **It does not** — you have found a route that does more than you thought.
  Give it its own route, or narrow its paths so the code path is unreachable
  from it.

Do not reach for `unrestricted = true` first: it gives away every statement on
that proxy, including the ones reading password hashes.

A denied statement aborts the request, so a run shows only the first denial of
each one. That is what the test suite is for.

## A denial in production

Same line, same fields — but you cannot iterate on it. A denial on a route
nothing should be calling is a finding, not a missing grant. Collect them into
the manifest in one pass, because a deployment's log holds all of them:

```console
$ blastit-manifest fold app.log --manifest blastit.toml --write
```

A denied statement surfaces as a database error, usually a 500.
`BLASTIT_LOG_LEVEL=warn` keeps the steady-state log to the interesting lines.

## Fold the log

`fold` keeps the `sql.denied` events and compares them with the manifest using
blastit's own normaliser.

| Option | What it does |
|---|---|
| `<LOG>…` | one or more logs, or `-` for stdin |
| `--manifest PATH` | **required**: the manifest to extend |
| `--write` | rewrite it in place instead of printing |
| `--prefer-tables` | the smallest `tables` grant covering the denials |
| `--allow-dangerous` | also grant statements a grant is not an allowlist for |

It never adds a grant to an `unrestricted` route, and touches nothing without
`--write`.

An exact `queries` entry is trusted as it is: blastit does not look at what the
statement calls. So `fold` refuses DDL, `SET`, `COPY`, `CALL`, `TRUNCATE`, the
transaction commands, and statements calling a function outside
[blastit's safe set](../reference/manifest.md#the-built-in-safe-function-set) —
unless `--allow-dangerous` is given. It names what it refused, on stderr.

Two statements no grant can cover: a savepoint's generated name needs
`transaction_control = true` on the route, and a named cursor
(`QuerySet.iterator()`) needs server-side cursors off
(`DATABASES[...]["DISABLE_SERVER_SIDE_CURSORS"] = True`).

## Keeping the manifest honest

**Fail CI when it drifts.** With the plugin:

```console
$ uv run pytest --blastit-manifest blastit.toml --blastit-strict
```

**Without the plugin, fold a staging log before a release:** run the real app
with blastit in front, then:

```console
$ blastit-manifest fold staging.log --manifest blastit.toml --write
$ git diff blastit.toml
```

**Keep one group for connection setup.** psycopg runs statements of its own
whenever a connection is opened, and none goes through a request's cursors, so
they belong in a group attached to every route:

```toml
groups = ["connection"]

[group.connection.db]
queries = ["SELECT set_config('TimeZone', $1, false)", "SELECT 1"]
```

## Upgrading

Upgrading is `uv add --upgrade blastit`. Restart, then read the boot lines:
`privilege`, `preset`, `seccomp`. A manifest a new version no longer accepts
fails at boot, naming what it did not like.

## Next

[Fit it to your infra](../infra/index.md) for presets, memory and logs.
