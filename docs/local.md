# Local development

`blastit dev` is Linux only; the editor is not — it works on Linux, macOS and
Windows ([Editors](#editors)). `blastit dev` runs your app the way production does,
without root: everything runs as you, so nothing needs `sudo`, and a
misconfiguration fails on your machine instead of in production. One uid means it
is **not a sandbox**: the credential barrier still keeps the database URL out of
the app's `environ` and `argv`, but any process running as you can read `.env` and
your `/proc` entries. Use a throwaway local database; never point `blastit dev` at
production data.

## Start

```console
$ uv run blastit dev
```

It reads `./Blastfile` here (no parent search), binds `127.0.0.1:8080` only, and
prints pretty logs (`BLASTIT_LOG_FORMAT` wins); the banner stays for the run.
Startup prints one `runtime` line naming what it will run:

| `Blastfile` | `blastit dev` prints |
|---|---|
| `entrypoint php "public/index.php"` | `00:11:36.867 INFO  runtime     php 8.5.4  libphp /usr/lib/libphp8.5.so  php.ini /etc/php/8.5/embed/php.ini` |
| `entrypoint python "app:app"` | `00:11:36.867 INFO  runtime     python 3.14.0  environment /home/you/shop/.venv` |
| no `entrypoint` | `00:11:36.867 INFO  runtime     static  serving static assets only` |

`php.ini` is the file PHP actually loads; `none` means built-in defaults.

Edits reload your views, all together. `--no-reload` turns that off.

| You save | Blastit |
|---|---|
| a view, a shared module, an editable package or template, even outside the project | reloads every view together |
| `./Blastfile` or a [`preload`](reference/blastfile.md#sections) module | re-reads what changed, then reloads the whole topology |
| *any of these* | keeps `:8080` up: no restart, no second terminal |

| Flag | What it does |
|---|---|
| `--port N` | listen on `N` instead of `8080` |
| `--host ADDR` | expose the server on another address for a VM or phone; also set `BLASTIT_DEV_EXPOSE=1`, and the boot line then names the address and your uid |
| `--learn` | record instead of denying; see [below](#fill-the-grants-in-one-pass) |
| `--no-reload` | turns reload off (it is on by default); nothing is watched |

`--host` on its own is refused: `BLASTIT_DEV_EXPOSE=1` is the second acknowledgement.
Exposed there is no authentication — anyone who reaches it drives every route as you —
so use only the throwaway database above, never production data or an `unrestricted` route.

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

A view with no `postgres` block gets no listener at all. Here `postgres {}` gives
it a listener that denies every statement (`42501`) — what the next step fixes.

## Fill the grants in one pass

With a test suite, one command records the whole app:

```console
$ uv run blastit check --learn
learn: 2 routes, 38 queries added → /home/you/shop/Blastfile
$ git diff Blastfile
$ uv run blastit check
blastit check: 412 requests, 977 statements, 3/3 routes, clean
```

Without a suite, click through the app instead; ++ctrl+c++ stops it:

```console
$ uv run blastit dev --learn
…
learn: 2 routes, 3 queries added → /home/you/shop/Blastfile
```

Learn mode allows what the file does not cover and records it, so one pass sees
every view and statement instead of stopping at the first denial. On exit it adds
the views it saw with their exact statements, names a view from the path, and maps
numeric and UUID segments to `{id:int}`/`{id:uuid}`; review the diff and narrow
anything guessed too widely. A statement that inlines a value is refused: bind it
and learn again, then run `blastit check` without `--learn`.

## A denial

Outside learn mode, a statement a view is not granted fails the request with
PostgreSQL's permission error (`42501`) — uncaught, a `500`. One line says why:

```console
15:41:02.118 WARN  sql.denied  products  db  parse  SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?
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

## Editors

1. Zed ≥ 0.205. **Blastfile** is not in Zed's registry yet: `git clone
   https://github.com/OffworldNexus/blastit`, then `zed: install dev extension` on
   `editors/zed` — the only step needing Rust (Zed's builder installs the
   `wasm32-wasip2` target itself). Once listed, `zed: extensions` → **Blastfile** is
   the whole step.
2. Once the release ships `blastit-lsp`, the extension finds the server itself, in a
   fixed order ([the order](reference/cli.md#blastit-lsp)); there is nothing to
   configure.
3. Open `./Blastfile`: colours (SQL inside a `query '…'` only: a `query "…"` is
   one string), bracket matching, an outline, and a squiggle, inside the string,
   at the `line:col` `blastit check` prints. Quick Fix **Parameterize SQL for
   Blastfile ($?)** turns a pasted literal or `$1…$n` into `$?`, and an
   `IN ($1, $2)` list into `IN ($?, ...)` ([grants](reference/blastfile.md#grants)
   has the shapes). Hovering `$?` or `...` explains each.
4. Merge this into `.zed/settings.json` to turn format-on-save on: Zed's default is
   `off` and an extension cannot set it. Blastit's repo commits that file; save a
   mis-indented line and it snaps back.

```json
{ "languages": { "Blastfile": { "format_on_save": "on" } } }
```

Another editor registers the extensionless `Blastfile` and spawns
[`blastit-lsp`](reference/cli.md#blastit-lsp) with nothing on the command line, or
[`blastit lsp`](reference/cli.md#blastit-lsp-linux) on Linux, speaking LSP on
stdin/stdout. Running the server needs no `npm`, Tree-sitter CLI, `cargo` or
`wasi-sdk`. CI checks the file with `blastit check`, and production never runs
either. macOS and Windows get this editor only: serving an app needs Linux, as root
or with the capability set ([deployments](deploy/index.md)), and those platforms
have no `blastit` binary at all — `dev`, `check`, `fmt` and `lsp` are Linux only —
so run the audit in CI or a Linux container.

## Before you deploy

Production refuses to start on one uid. To exercise the real uid and capability
path locally, use the [rootless recipe](deploy/anything-else.md#root-or-rootless).
Otherwise, [CI](ci.md) is the next gate.
