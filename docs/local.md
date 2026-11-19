# Local development

Blastit runs on **Linux**: `blastit dev` starts your app the way production does,
without root, so a misconfiguration fails here instead of in production.

```console
$ blastit dev        # or `uv run blastit dev` in a Python project
$ blastit dev --learn
```

!!! warning "One uid, not a sandbox"

    Every role shares your uid here. The credential barrier and the grants are real,
    but any process running as you can read `.env` and your `/proc` entries, and no
    uid boundary keeps a compromise inside its view. Use a throwaway database, never
    production data; `--host` is refused unless `BLASTIT_DEV_EXPOSE=1` acknowledges it.

??? note "Developing on macOS or Windows?"

    The server is Linux-only: run `blastit dev` in Docker/Compose with the port
    published, or in a Linux VM. The editor is not — `blastit-lsp` ships native
    Linux, macOS and Windows binaries ([Editors](#editors)).

## Start

=== "Python (WSGI)"
    ```console
    $ uv run blastit dev
      blastit dev · single-uid simulation · http://127.0.0.1:8080
    ```

=== "PHP"
    ```console
    $ blastit dev
    ```

=== "Static & SPA"
    ```console
    $ blastit dev             # the release archive: no language runtime, no store
    ```

It binds `127.0.0.1:8080` (`--port N`, `--host ADDR`), prints one components line and a
row per view and per `static` mount, and reloads every view on save (`--no-reload`).

```mermaid
flowchart LR
    save["you save a file"] -- "fanotify" --> reload["every view reloaded<br/>port 8080 stays up"]
    req(["curl a route"]) -- "not granted" --> denied["sql.denied · 42501"]
    req -- "--learn" --> record["learn: 1 route, 1 query"]
```

## PostgreSQL and `DATABASE_URL`

`DATABASE_URL` is the app's normal libpq URL; a throwaway store is `docker run --rm -d
-p 127.0.0.1:5432:5432 -e POSTGRES_USER=shop -e POSTGRES_PASSWORD=secret -e
POSTGRES_DB=shop postgres:18`. Blastit reads the process environment only (`uv run
--env-file .env blastit dev`; `set -a; . ./.env; set +a` for the archive), hands the app
a local URL, and connects upstream itself, so the password never enters the app.

=== "Python (WSGI)"
    ```console
    $ export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
    $ uv run blastit dev
    ```

=== "PHP"
    ```console
    $ export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
    $ blastit dev
    ```

=== "Static & SPA"
    ```console
    $ blastit check         # a mount reads files: no store, no URL
    ```

Your app keeps reading `DATABASE_URL`, and the `Blastfile` stays safe to commit.

## Fill the grants in one pass

=== "Python (WSGI)"
    ```console
    $ uv run blastit check --learn
    learn: 1 route, 1 query added → /home/you/shop/Blastfile
    ```

=== "PHP"
    ```console
    $ blastit dev --learn       # click through the routes, then Ctrl-C
    $ blastit fmt
    ```

=== "Static & SPA"
    ```console
    $ blastit check            # roots audited; nothing to learn
    ```

Learn mode allows what the file does not cover and records it, so one pass sees every
view and statement instead of stopping at the first denial.

## A denial

A statement a view is not granted fails with PostgreSQL's `42501`, and the log names
it normalised, each literal replaced by `…`. Swap every `…` for `$?` and it is what a
grant must contain — add that `query '…'` to the view's `postgres` block; if it does
not belong there, the view does more than you thought.

## Editors { #editors }

Zed ≥ 0.205: **Blastfile** is not in the registry yet, so `git clone
https://github.com/OffworldNexus/blastit` and `zed: install dev extension` on
`editors/zed`. The extension finds [`blastit-lsp`](reference/cli.md#blastit-lsp) by
itself (settings → `PATH` → release binary) and offers Quick Fix **Parameterize SQL
for Blastfile ($?)** at the `line:col` a denial prints.

```json
{
  "languages": {
    "Blastfile": {
      "format_on_save": "on"
    }
  }
}
```

## Where next

* [CI/CD](ci.md) — the same file, one command, in a runner.
* [Deployments](deploy/index.md) — real uids and capabilities.
* [Blastfile reference](reference/blastfile.md) — every directive.
