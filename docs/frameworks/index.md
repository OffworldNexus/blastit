# Adapting your application

Blastit is not a framework integration: it starts your app and calls it. Adapting one is
**a view block, a few directives, and one rule about secrets** — no plugin and no
monkeypatching.

Every framework page fills the same nine slots, in this order, with only what is
specific to that framework. Read this page once, then open yours.

## Where the pages differ

| Page | 1. Executor | 2. Static | 3. Database |
|---|---|---|---|
| [Django](django.md) | `entrypoint python` | `collectstatic` into a `static` mount | `postgres $DATABASE_URL` |
| [Flask, SQLAlchemy, raw WSGI](flask.md) | `entrypoint python` | a view of its own | `postgres $DATABASE_URL` |
| [PHP](php.md) | `entrypoint php` | `public/` is the static root | `postgres $DATABASE_URL` |
| [Static and SPAs](static-and-spa.md) | none | a `static` mount's `root` | none |

The other six slots read the same on every page. A Node HTTP upstream and Tomcat are
planned for 0.2.x and will fill the same nine slots.

Every page writes the same sections in the same order:

```text title="Blastfile"
app "myshop"

entrypoint python "myshop.wsgi:application"   # 1. executor, or entrypoint php, or neither

postgres $DATABASE_URL                        # 3. database

view checkout POST "/checkout" {              # 4. ingress and what this view may run
    body json
    postgres {}
}
```

## 1. Executor

How blastit starts your code: a WSGI callable in `entrypoint python`, a PHP front
controller in `entrypoint php`, or a built front end under a `static` mount. A file
declares one executor, so it is a Python app, a PHP app, or static files.

## 2. Static assets

Where the built files live, and which are fingerprinted: an `immutable` glob gets the
year-long cache, everything else is revalidated. `*` never crosses `/`, so list one
pattern per depth; get it wrong and a deploy serves last week's JavaScript for a year, or
revalidates every hash-named file forever.

## 3. Database { #bind-parameters-server-side }

The variable blastit owns and the grant that fixes what the view may run: exact `query`
lines, or a table grant with the columns it may read and write. The app gets a local URL
on a Unix socket and never the credential. Parameters travel apart from the statement
text, so one template is one grant — an inlined literal has no stable text to grant.
Without a `postgres` store there is no database; without a `postgres` block in a view
there is no listener.

## 4. Ingress

`json` and `form` are decoded and re-encoded before the app sees them, and `none` refuses
any body. `raw` forwards the bytes unchanged, and it is the one codec nothing validates at
run time: a production boot **refuses** it without `unvalidated` — naming the view and
the directive, before it binds a listener — while `blastit dev` warns and serves, and
`blastit check` fails with `raw_ingress`. A body to a view with no `body` is a `413`, and
`check` fails the view that took one.

## 5. Environment and secrets

Blastit reads the real `DATABASE_URL` from its own environment, strips every store input
and `BLASTIT_*` at the `execve`, and renders the local URL in their place. Read
`DATABASE_URL` and nothing else about the database — the host, user and password variables
it might expect do not exist — and `BLASTIT_ENV_STRIP` hides anything else you would
rather not pass on.

## 6. Health

`/_blastit/health` (`BLASTIT_HEALTH_PATH`), answered by the scheduler for `GET` and `HEAD`
before routing, from no worker. Point the platform's check at it — a view for it never
runs — and keep database queries out of any health view: the platform check then learns
blastit is up, not the pool's health.

## 7. Migrations

Schema changes run outside blastit, with the real credentials, as their own role on the
database's direct URL. Grants cover what a request needs; a view that could also run
`ALTER TABLE` is a view that can rewrite its own permission model.

## 8. Learn and check

`blastit dev --learn` records what you click through and writes the views and exact
`query` lines the file lacks; `blastit check` is the gate, and `check --learn` adds a view
only for a path your suite requested that no view matches. Read a finding by kind — a
missing grant (paste the statement `check` printed), a dead grant (cover the path or
remove it), an unrouted request (widen a pattern), a missing ingress (declare the codec)
— and `--json` prints the same list for an agent ([CI/CD](../ci.md#what-it-fails-on)). Use
`uv run blastit` in a project and `uvx blastit` without one.

## 9. Deployment notes

One view is one uid: a view whose static responder forwards — to the app or to `entrypoint
php` — gets a process, while an all-static mount answers alone and needs none.
`BLASTIT_MAX_WORKERS` (16) caps requests across every view and each worker holds one
upstream connection, so fit it inside the database's connection limit. A view that opens
an outbound connection needs `egress any`, which is every address and port;
[Deployments](../deploy/index.md) has the rest.

## Where next

[Django](django.md) · [Flask, SQLAlchemy and raw WSGI](flask.md) ·
[PHP (Symfony, Laravel)](php.md) · [static assets and SPAs](static-and-spa.md) ·
[local development](../local.md) · [CI/CD](../ci.md).
