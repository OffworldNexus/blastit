# Adapting your application

Blastit is not a framework integration: it starts your app and calls it. Adapting one is
**a manifest block, a few settings, and one rule about secrets** — no plugin and no
monkeypatching.

Every framework page fills the same nine slots, in this order, with only what is
specific to that framework. Read this page once, then open yours.

## Where the pages differ

| Page | 1. Executor | 2. Static | 3. Database |
|---|---|---|---|
| [Django](django.md) | WSGI `app` | `collectstatic` into `[route.static]` | `DATABASE_URL` on a socket |
| [Flask, SQLAlchemy, raw WSGI](flask.md) | WSGI `app` | a route of its own | `DATABASE_URL` on a socket |
| [PHP](php.md) | `[route.php] entry` | `public/` is the static root | `DATABASE_URL` on a socket |
| [Static and SPAs](static-and-spa.md) | none | `[route.static] root` | none |

The other six slots read the same on every page. A Node HTTP upstream and Tomcat are
planned for 0.2.x and will fill the same nine slots.

Every page writes the same blocks in the same order:

```toml title="blastit.toml"
version = 1
app = "myshop.wsgi:application"   # 1. executor: or [route.php], or [route.static]

[proxy.db]                        # 3. database
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "checkout"
path = ['/checkout/']
[route.db]                        # what this route may run: queries or tables
queries = []
[route.ingress]                   # 4. ingress: json, form, none, raw
body = "json"
```

## 1. Executor

How blastit starts your code: a WSGI callable in `app`, a PHP front controller in
`[route.php] entry`, or a built front end in `[route.static] root`. Without `app`, every
route must be one of the last two and a plain route stops the boot; with `app`, a route
that declares neither is a WSGI view.

## 2. Static assets

Where the built files live, and which are fingerprinted: an `immutable` glob gets the
year-long cache, everything else is revalidated. `*` never crosses `/`, so list one
pattern per depth; get it wrong and a deploy serves last week's JavaScript for a year, or
revalidates every hash-named file forever.

## 3. Database { #bind-parameters-server-side }

The variable blastit owns and the grant that fixes what the route may run: exact
`queries`, or `tables` with the columns it may read and write. The app gets a local URL on
a Unix socket and never the credential. Parameters travel apart from the statement text,
so one template is one grant — an inlined literal has no stable text to grant. Without
`[proxy.db]` there is no database; without `[route.db]` no listener, and `queries = []` a
listener that denies everything (`42501`).

## 4. Ingress

`json` and `form` are decoded and re-encoded before the app sees them, and `none` refuses
any body. `raw` forwards the bytes unchanged and the boot warns either way;
`allow_raw = true` is what stops `blastit check` reporting `raw_ingress`. A body to a
route with no `[route.ingress]` is a `413`, and `check` fails the route that took one.

## 5. Environment and secrets

Blastit reads the real `DATABASE_URL` from its own environment, strips every proxy input
and `BLASTIT_*` at the `execve`, and renders the local URL in their place. Read
`DATABASE_URL` and nothing else about the database — the host, user and password variables
it might expect do not exist — and `BLASTIT_ENV_STRIP` hides anything else you would
rather not pass on.

## 6. Health

`/_blastit/health` (`BLASTIT_HEALTH_PATH`), answered by the scheduler for `GET` and `HEAD`
before routing, from no worker. Point the platform's check at it — a route for it never
runs — and keep database queries out of any health route: the platform check then learns
blastit is up, not the pool's health.

## 7. Migrations

Schema changes run outside blastit, with the real credentials, as their own role on the
database's direct URL. Grants cover what a request needs; a route that could also run
`ALTER TABLE` is a route that can rewrite its own permission model.

## 8. Learn and check

`blastit dev --learn` records what you click through and writes the routes and `queries`
the manifest lacks; `blastit check` is the gate, and `check --learn` adds a route only for
a path your suite requested that no route matches. Read a finding by kind — a missing
grant (paste the statement `check` printed), a dead grant (cover the path or remove it), an
unrouted request (widen a `path`), a missing ingress (declare the codec) — and `--json`
prints the same list for an agent ([CI/CD](../ci.md#what-it-fails-on)). Use
`uv run blastit` in a project and `uvx blastit` without one.

## 9. Deployment notes

One route is one uid: a route whose static responder forwards — to `app` or its own
`[route.php]` — gets a process, while `spa = true` answers alone and needs none.
`BLASTIT_MAX_WORKERS` (16) caps requests across every view and each worker holds one
upstream connection, so fit it inside the database's connection limit. A route that opens
an outbound connection needs `egress = true`, which is every address and port;
[Deployments](../deploy/index.md) has the rest.

## Where next

[Django](django.md) · [Flask, SQLAlchemy and raw WSGI](flask.md) ·
[PHP (Symfony, Laravel)](php.md) · [static assets and SPAs](static-and-spa.md) ·
[local development](../local.md) · [CI/CD](../ci.md).
