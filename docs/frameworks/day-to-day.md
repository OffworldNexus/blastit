# Day-to-day life

Once blastit is installed and the manifest roughly matches the app, this is what
working with it looks like.

## Everyday life

Two things change about your day:

1. **A new URL needs a route.** Not a view — the view is Django's or Flask's
   business — but a decision about which sandbox it belongs to. Usually the
   answer is "the route next to it", so nothing changes at all.
2. **A new query needs a grant.** Either an exact `queries` entry, or a table
   grant that already covers it. Until then the statement is refused, and the
   refusal is a log line.

Everything else is unchanged: you still run `manage.py`, still run
`pytest`, still deploy a container.

## Adding a view

=== "Django"

    A new view under an existing URL prefix needs no manifest change at all: the
    route matches the *prefix*, and Django routes the rest.

    ```python
    # shop/urls.py
    path("products/<int:pk>/reviews/", views.reviews),
    ```

    `path = ['/products/', '/products/[0-9]+/']` does not match
    `/products/42/reviews/`, so widen it if the new page belongs to that route:

    ```toml
    path = ['/products/.*']        # the whole subtree, one sandbox
    ```

    Or give the reviews page a route of its own, which is what you do when it
    needs different grants — a reviews page that writes a table the catalogue
    page may not touch is a different permission set, and therefore a different
    route.

=== "Flask"

    Same thing: the route matches the URL prefix and Flask picks the endpoint.

    ```python
    @app.get("/products/<int:pk>/reviews/")
    def reviews(pk):
        ...
    ```

A new route is a new **process** and a new **uid**
(`BLASTIT_UID_BASE + 1000 + its index in the manifest`), so reordering routes
changes which uid serves which URL. That is harmless: the uids only live as long
as the container. Restart blastit and the new routing table is in force — the
manifest is read at boot, not per request.

A URL that matches no route never reaches your app. It gets blastit's own `404`,
logged as a `request` event with `"route": null`. That is why most projects keep
a catch-all:

```toml
[[route]]
name = "fallback"
path = ['.*']
```

## A denial in development

Run blastit with the readable format and click around:

```console
$ BLASTIT_LOG_FORMAT=pretty sudo -E .venv/bin/blastit
15:41:02.118 WARN  sql.denied  products  db  parse  SELECT "shop_product"."price" FROM "shop_product" …
```

The JSON form says more, and is what tooling reads:

```json
{"ts":"…","level":"warn","event":"sql.denied","component":"dbproxy","route":"products","pid":4321,"proxy":"db","kind":"parse","sqlstate":"42501","in_transaction":false,"sql":"SELECT \"shop_product\".\"price\" FROM \"shop_product\"","reason":"select on shop_product.price not granted"}
```

| Field | What it tells you |
|---|---|
| `route` | which sandbox refused it |
| `proxy` | which service the connection was to |
| `sql` | the statement, normalised — the text a grant has to contain |
| `kind` | `parse`, `query`, `function_call`, `copy`, … — how it arrived |
| `sqlstate` | `42501`, "insufficient privilege" |
| `reason` | the policy's explanation, e.g. `select on shop_product.price not granted`. Never a parameter value. |
| `in_transaction` | whether the connection was inside a transaction |

Read the `reason` before the `sql`. `select on shop_product.price not granted`
and `the statement is not granted` are different problems: the first is a table
grant that is missing a column, the second is a statement nobody listed at all.

Then decide, and there are only two answers:

* **The statement belongs there.** Add it — the normalized `sql` field can be
  pasted into `queries` as it is.
* **It does not belong there.** You have just found a route that does more than
  you thought. Give it its own route, or narrow the route's paths so that the
  code path is not reachable from it.

Do not reach for `unrestricted = true` before trying the second answer.
`unrestricted` is honest about a route whose SQL cannot be enumerated (Django's
admin), and it gives away every statement on that proxy — including the ones
that read password hashes.

Watch out for the shape of the loop: a denied statement **aborts the request**,
so each run only shows you the *first* denial per request. That is why a test
suite is the better tool earlier in the process.

## A denial in production

Identical line, identical fields — the difference is that you cannot iterate on
it, and that it is happening now. Two things to do, in this order:

1. **Read the `reason` and decide whether the request should have reached that
   route at all.** A denial on a route nothing should be calling is a finding,
   not a missing grant. Every refusal is blastit doing its job; the log is how
   you hear about it.
2. **Collect the denials into the manifest.** `blastit-manifest fold` is exactly
   this: give it the log, and it prints (or writes) the grants the manifest is
   missing.

```console
$ blastit-manifest fold app.log --manifest blastit.toml
$ blastit-manifest fold app.log --manifest blastit.toml --write
```

A single deployment's log has every denial in it, so one pass beats one deploy
per statement.

!!! tip "Denials are not errors your users see twice"

    A denied statement surfaces in the app as a database error, which usually
    becomes a 500. `BLASTIT_LOG_LEVEL=warn` keeps the steady-state log to the
    interesting lines without hiding them.

## Fold the log

`blastit-manifest fold` reads blastit's own JSON events, keeps the `sql.denied`
ones, and compares them with the manifest using blastit's own normaliser and
table-grant analysis — so what it writes is what dbproxy matches.

```console
$ blastit-manifest fold app.log --manifest blastit.toml            # print the TOML to paste
$ blastit-manifest fold app.log --manifest blastit.toml --write    # or write it in place
$ blastit-manifest fold app.log --manifest blastit.toml --prefer-tables
$ blastit-manifest fold app.log --manifest blastit.toml --allow-dangerous
$ blastit-manifest fold app.log - --manifest blastit.toml          # read a log from stdin
```

| Option | What it does |
|---|---|
| `<LOG>…` | one or more logs, or `-` for standard input |
| `--manifest PATH` | **required**: the `blastit.toml` to extend |
| `--write` | rewrite the manifest in place instead of printing the TOML |
| `--prefer-tables` | add the smallest `[route.<proxy>.tables]` grant covering the denials, keeping as exact queries whatever a table grant cannot describe |
| `--allow-dangerous` | grant statements a grant is not an allowlist for (see below) instead of refusing them |

Without `--write` nothing on disk is touched: it prints the grant to add, with
the reason dbproxy gave as a comment. With it, the statements go inside the
route's existing `queries = [ … ]` (or a `[route.<proxy>.tables]` section is
added under the grant), and the rest of the file — comments, order, everything —
stays where it was.

It skips what your manifest already allows, and it never adds a grant to an
`unrestricted` route: there is nothing left to grant there.

### What it refuses, and why

An exact `queries` entry is trusted as it is. blastit compares the text and runs
what it matches — it does not look at what the statement calls. So a `queries`
entry for `SELECT set_config(…)`, `SELECT pg_read_file(…)`,`SELECT
lo_import(…)` or a statement calling `pg_terminate_backend` runs that function
with the app's privileges.

`fold` therefore refuses two kinds of thing unless `--allow-dangerous` is given:

* **a statement whose kind a grant is not an allowlist for** — DDL, `SET` and
  `RESET` (`SET ROLE` above all), `COPY`, `DO`, `CALL`, `LOCK`, `GRANT`,
  `REVOKE`, `TRUNCATE`, `VACUUM`, `LISTEN`/`NOTIFY`, the transaction commands;
* **a statement calling a function outside blastit's safe set** —
  `set_config`, `pg_read_file`, `lo_import`, `pg_terminate_backend`, an
  extension's function.

Give the route a **table grant** instead, which does check functions; or say
`--allow-dangerous` if that really is what you want. And a text holding more
than one statement is refused with or without the flag, because a grant is one
statement — split it first.

Either way it says what it refused and why, on stderr, and writes none of it.

### Two statements no grant can cover

`fold` cannot help with these two, and the report says so rather than inventing a
grant:

* **a savepoint's name** — `SAVEPOINT "s140234_x1"` is generated per request.
  The fix is `transaction_control = true` on the route.
* **a named cursor** — `QuerySet.iterator()`, that is Django's
  `chunked_cursor()`, sends `DECLARE "_django_curs_140234_sync_1" CURSOR FOR …`
  and then `FETCH`/`CLOSE` with the same generated name. Turn server-side
  cursors off for the routes that use them
  (`DATABASES[...]["DISABLE_SERVER_SIDE_CURSORS"] = True`) or accept that they
  will be denied.

## Keeping the manifest honest

Two habits keep a manifest from drifting away from the app.

**Fail CI when it drifts.** With the plugin:

```console
$ uv run pytest --blastit-manifest blastit.toml --blastit-strict
```

`--blastit-strict` fails the session on anything the report finds: a statement
that is missing, a path that is unrouted, a route that needs
`transaction_control`, a route that opens connections without `egress = true`, a
connection-setup statement some route does not grant, a statement the plugin
could not read.

**Keep one group for connection setup.** psycopg runs a few statements of its
own every time a connection is opened, and under blastit that is *every
request*. Django's `set_config('TimeZone', …)` is one; a `connection_created`
receiver that runs a `SET` is another. None of them goes through a request's
cursors, so they belong in a group attached to every route:

```toml
groups = ["connection"]

[group.connection.db]
queries = ["SELECT set_config('TimeZone', $1, false)", "SELECT 1"]
```

The plugin reports them separately, as `connection_setup`, with the routes that
do not grant them.

## Upgrading

blastit is pre-release, so upgrading is the one operation to be deliberate about.

=== "uv"

    ```console
    $ uv add --prerelease allow --upgrade blastit
    ```

    The wheel index keeps every version it has ever published, so a lock file
    keeps resolving.

=== "pip"

    ```console
    $ pip install --pre --upgrade blastit --extra-index-url https://offworldnexus.github.io/blastit/simple/
    ```

Then restart the server and read the first three lines of the log. They say
which build's decisions are in force:

```json
{"event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"root","bounding_set":"dropped"}
{"event":"boot","component":"init","stage":"config","seccomp":true,"privilege":"root","preset":"digitalocean-paas",…}
{"event":"boot","component":"init","stage":"preload","immortalize":"on","immortalized":148213,…}
```

Three things to check after an upgrade:

* **`privilege`** — `root` or `capabilities`. A deployment that *thinks* it
  granted a capability finds out here, at boot, rather than at the first
  request.
* **`preset`** — the preset in force, the settings it supplied and the ones an
  explicit variable overrode. A preset only ever supplies defaults, so a
  variable you set yourself still wins.
* **`seccomp`** — `true` unless `BLASTIT_SECCOMP=0`.

Because blastit has not reached 0.1, the **manifest format may still change**,
and a new version can refuse a `blastit.toml` an old one accepted. Boot
validation is strict and names what it did not like, so an upgrade that breaks a
manifest fails at boot with a message rather than at the first request.

If you keep the wheel in an image, rebuild the image: the extension is not
`abi3` and is built for the interpreter it is installed into, so a wheel copied
across a base-image change will not load.
