# Local development

`blastit dev` runs your app the way production does, without root: everything runs
as you, so nothing needs `sudo`, and a misconfiguration fails on your machine
instead of in production.

## Start

```console
$ uv run blastit dev
  blastit dev · single-uid simulation · http://127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
```

!!! warning "One uid, not a sandbox"

    Every role shares your uid here. The credential barrier and the grants are
    real, so a misconfiguration is caught locally — but any process running as
    you can read `.env` and your `/proc` entries, and there is no uid boundary to
    keep a compromise inside its view. Use a throwaway local database; never
    point `blastit dev` at production data. `--host` adds no authentication
    either: anyone who reaches it drives every view as you, so it is refused
    unless `BLASTIT_DEV_EXPOSE=1` acknowledges that.

It reads `./Blastfile` here (no parent search), binds `127.0.0.1:8080` only, and
prints pretty logs (`BLASTIT_LOG_FORMAT` wins); the banner stays for the run.
Startup prints one `runtime` line naming what it will run:

| `Blastfile` | `blastit dev` prints |
|---|---|
| `entrypoint php "public/index.php"` | `00:11:36.867 INFO  runtime                php 8.5.4  libphp /usr/lib/libphp8.5.so  php.ini /etc/php/8.5/embed/php.ini` |
| `entrypoint python "app:app"` | `00:11:36.867 INFO  runtime                python 3.14.0  environment /home/you/shop/.venv` |
| no `entrypoint` | `00:11:36.867 INFO  runtime                static  serving static assets only` |

`php.ini` is the file PHP actually loads; `none` means built-in defaults. Every
line is one event, aligned in the same columns: `HH:MM:SS.mmm LEVEL event`, then
the summary.

Edits reload your views, all together. `--no-reload` turns that off.

| You save | Blastit |
|---|---|
| a view, a shared module, an editable package or template, even outside the project | reloads every view together |
| `./Blastfile` or a [`preload`](reference/blastfile.md#sections) module | re-reads what changed, then reloads the whole topology |
| *any of these* | keeps `:8080` up: no restart, no second terminal |

```text
15:41:02.119 INFO  reload                 2 directories
```

| Flag | What it does |
|---|---|
| `--port N` | listen on `N` instead of `8080` |
| `--host ADDR` | expose the server on another address for a VM or phone; the boot line then names the address and your uid |
| `--learn` | record instead of denying; see [below](#fill-the-grants-in-one-pass) |
| `--no-reload` | turns reload off (it is on by default); nothing is watched |

```mermaid
flowchart LR
    save["you save a file"] --> watch["fanotify watch<br/>per loaded directory"]
    watch --> reload["reload event<br/>N directories"]
    reload --> views["every view reloaded<br/>port 8080 stays up"]
    curl(["curl a route"]) --> deny["not granted?"]
    deny -- "--learn" --> record["learn: N routes,<br/>M queries added"]
    deny -- "normal run" --> denied["sql.denied<br/>42501"]
    record --> diff["git diff Blastfile<br/>then blastit check"]
```

## Add PostgreSQL

Any PostgreSQL works. No server? One line starts one on loopback only, if you
have Docker (skip it if you already have PostgreSQL):

```console
$ docker run --rm -d -p 127.0.0.1:5432:5432 \
    -e POSTGRES_USER=shop -e POSTGRES_PASSWORD=secret -e POSTGRES_DB=shop postgres:18
```

```text title="Blastfile"
app "shop"

entrypoint python "app:app"

postgres $DATABASE_URL

view products regex '/products/.*' {
    postgres {}
}
```

```console
$ export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
$ uv run blastit dev
```

Blastit reads the process environment only. `uv run --env-file .env blastit dev`
loads a file; keep it to the throwaway database above.

Your app keeps reading `DATABASE_URL`: Blastit reads the real one, hands the app a
local URL, and connects upstream itself. The password never enters the app's
environment, and a `Blastfile` holds no credentials, so it is safe to commit.

A view with no `postgres` block gets no listener in normal mode. Here `postgres {}`
gives it a listener that denies every statement (`42501`) — what `--learn` fills in.

## Fill the grants in one pass

With a test suite, `check --learn` adds newly discovered views and their queries:

```console
$ uv run blastit check --learn
learn: 1 route, 1 query added → /home/you/shop/Blastfile
$ git diff Blastfile
$ uv run blastit check
blastit check: 412 requests, 977 statements, 3/3 views, clean
```

Without a suite, click through the app instead; ++ctrl+c++ stops it:

```console
$ uv run blastit dev --learn
…
learn: 1 route, 1 query added → /home/you/shop/Blastfile
```

Learn mode allows what the file does not cover and records it, so one pass sees
every view and statement instead of stopping at the first denial. On exit it adds
the views it saw with their exact statements and maps numeric and UUID segments to
`{id:int}`/`{id:uuid}`; review the diff and narrow anything guessed too widely. A
statement that inlines a value is refused: bind it, learn again, then run
`blastit check` without `--learn`.

## A denial

Outside learn mode, a statement a view is not granted fails the request with
PostgreSQL's permission error (`42501`) — uncaught, a `500`. One line says why:

```console
15:41:02.118 WARN  sql.denied             products  db  parse  SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?
```

`sql` is the statement as normalised, with each quoted literal replaced by `…`:
replace every `…` with `$?` (the value is gone) and it is exactly what a grant has
to contain, safe to paste into a ticket; bind it in the code that sends it. The
JSON form adds `reason`, separating a missing table or column from an unlisted
statement. Two answers:

* **It belongs there.** Replace the empty `postgres {}` in that view with the
  normalised statement, each `…` bound as `$?`:

  ```text
  view products regex '/products/.*' {
      postgres {
          query 'SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?'
      }
  }
  ```

  An exact `query` is taken as written — parsed, not analysed: paste only a
  statement you have read. `pg_read_file`, `COPY … PROGRAM`, `dblink` and
  `lo_import` are file or network capabilities, not ordinary queries.

* **It does not.** You have found a view that does more than you thought. Give
  the code path its own view, or narrow this view's pattern until it is unreachable.

Do not reach for `unrestricted` first: it gives away every statement on that
store, including the ones reading password hashes.

## Editors { #editors }

Editor support is not Linux-only, unlike the server. Zed ≥ 0.205: **Blastfile**
is not in Zed's registry yet, so `git clone
https://github.com/OffworldNexus/blastit` and `zed: install dev extension` on
`editors/zed` — the only step needing Rust. The extension finds
[`blastit-lsp`](reference/cli.md#blastit-lsp) by itself (settings → `PATH` →
release binary), colours SQL inside a `query '…'`, and offers Quick Fix
**Parameterize SQL for Blastfile ($?)** at the `line:col` `blastit check` prints.

### Format on save

Merge this into `.zed/settings.json` — it is the copy committed in this repo:

```json
{
  "languages": {
    "Blastfile": {
      "format_on_save": "on"
    }
  }
}
```

From any other editor, spawn `blastit-lsp` with nothing on the command line.

## Before you deploy

Production refuses to start on one uid. To exercise the real uid and capability
path locally, use the [rootless recipe](deploy/anything-else.md#root-or-rootless).
Otherwise, [CI](ci.md) is the next gate.
