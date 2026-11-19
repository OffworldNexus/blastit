# Python (WSGI)

A Python app is one WSGI callable, `entrypoint python` names it, and `uv add blastit` adds the CLI
while the app keeps its own driver.

```mermaid
flowchart LR
    client(["client"]) --> ingress["ingress"]
    ingress --> sched["scheduler"]
    sched --> view["view · python<br/>shop.wsgi:application"]
    view -- "socket" --> dbproxy["dbproxy<br/>real credential"]
    dbproxy --> db[("PostgreSQL")]
```

## The Blastfile

```text title="Blastfile"
app "shop"

entrypoint python "shop.wsgi:application" {
    preload "shop.urls"
}

postgres $DATABASE_URL

view products GET "/products" {
    postgres { select products(id, name, price) }
}
```

`entrypoint python` names `module:attribute`, and `preload` imports each module once before the
workers fork, so every view starts with it in memory — name the URLconf or the app module, never
`settings`.

`uv add blastit`, then `uv run blastit dev` from the project root. Edits reload your views, all
together, and `--no-reload` turns that off; `uv run blastit fmt` puts it in canonical layout.

## One view, one worker

Each view is its own sandbox with its own grants and its own worker process, so a hot or risky path
gets its own `view` above the catch-all. `prefork` forks that worker at boot instead of on its
first request.

## The DATABASE_URL your app receives { #bind-parameters-server-side }

The app reads a local Unix-socket URL in `DATABASE_URL` and never the credential: Blastit strips
every store input at the `execve` and renders the local value in its place. Parameters travel apart
from the statement text, so one template is one grant; an inlined literal has no stable text to
grant, and `--learn` refuses to write one.

A statement `check` reports as not granted is closed by pasting the `sql` in that `add_query`
finding into the view's `postgres` block. The reference has every
[grant directive](../../reference/blastfile.md#grants).

## Learn the grants

`uv run blastit dev --learn` writes the views and exact `query` lines the run exercised, including
new statements on existing views. For a suite, `uv run blastit check --learn --dry-run` prints the
proposal and `uv run blastit check --learn` writes newly discovered views with their statements.

`--learn` never writes `body`, `use` or `transactions`, so declare those yourself. Read `git diff`,
split a view whose paths need different grants, then run `uv run blastit fmt`.

## Check before you commit

`uv run blastit check` runs `pytest -q` and fails on a statement the suite reached that the file
does not grant, on a grant the suite never used, on a view it never reached and on a request no
view matched. `--json` prints one finding per problem, each with a `fix.action` ([what each finding
means](../../ci.md#what-it-fails-on)).

## Where next

[Django](django.md) · [Flask, SQLAlchemy and raw WSGI](flask.md).
