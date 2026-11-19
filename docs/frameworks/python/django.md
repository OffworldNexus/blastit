# Django

A Django project is a WSGI callable; the [connector](index.md) starts it, and this file plus the
settings below are the whole adaptation, with `SECRET_KEY` still coming from the environment.

```mermaid
flowchart LR
    client(["client"]) --> ingress["ingress"]
    ingress --> sched["scheduler"]
    sched --> admin["view admin<br/>body form · transactions"]
    sched --> checkout["view checkout<br/>body json"]
    admin -- "socket" --> dbproxy["dbproxy"]
    checkout -- "socket" --> dbproxy
    dbproxy --> db[("PostgreSQL")]
```

## Install

`uv add blastit 'psycopg[binary]' dj-database-url` is the whole set: the CLI and server, the driver,
and the one-line `DATABASES` reader below.

## The Blastfile { #manifest }

```text title="Blastfile"
app "myshop"

entrypoint python "myshop.wsgi:application" {
    preload "myshop.urls"
}

postgres $DATABASE_URL

view admin GET, POST "/admin/*" {
    body form
    postgres {
        transactions
        select auth_user(id, username, password, last_name, first_name, email, is_staff, is_active, is_superuser, last_login, date_joined)
        select auth_group(id, name), django_content_type(id, app_label, model)
        select auth_permission(id, name, codename, content_type_id)
        select django_session(session_key, session_data, expire_date)
        select django_admin_log(id, action_time, object_id, object_repr, action_flag)
        insert django_admin_log(action_time, object_id, object_repr, action_flag, user_id)
        update auth_user(last_login), django_session(session_data)
        delete django_session
    }
}

view checkout POST "/checkout" {
    body json
    postgres {
        transactions
        insert orders(id, total)
    }
}

view catalog GET "/products/*" {
    postgres {
        select products(id, name, price), categories(id, name)
    }
}

view fallback GET "/*" {
}

static "/static" {
    root "staticfiles"
    immutable "/static/*"
}
```

`preload "myshop.urls"` imports the URLconf before the workers fork; views are first-match, so
`view admin` sits above the catch-all, and the last view takes every path nothing else claimed. A
body sent to the catch-all is a `413`, so a body-taking view is declared above it.

### The admin's grants

The admin's SQL is table reads and writes, not `query` lines: `view admin` takes the four verbs
above for its tables, each naming a column, and `atomic()` plus its savepoints need `transactions`
because the savepoint name is generated per request. Its permission lookup joins
`auth_permission.content_type_id` to `django_content_type`, and its changelist reads every
`auth_user` column above; a `pg_catalog` statement stays a `query`.

## Static assets { #static-assets }

Set `STATIC_ROOT = BASE_DIR / "staticfiles"` and `STATIC_URL = "/static/"`, then run
`uv run python manage.py collectstatic --no-input` once: the mount's `root` must exist before
`blastit dev` starts. One `immutable "/static/*"` reaches every depth, `select2/i18n` included: a
trailing `/*` crosses `/`, exactly as in a `view` pattern. `collectstatic` leaves the unhashed
copies beside the hashed ones, so that line pins both; to cache hashed names only, use
`immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'`.

## Learn the grants

`uv run blastit dev --learn` records what the admin and your routes really send; click through the
pages, stop with ++ctrl+c++, read `git diff Blastfile`, then run `uv run blastit fmt`. The admin's
grants are the long lines above, so split a view whose paths need different grants.

## Check in CI

`uv add --dev pytest pytest-django`, with a `manage.py` or `DJANGO_SETTINGS_MODULE`, is the
handshake `check` uses for `test_<database>`; `check` exits `3` when the suite collects nothing, so
write one test per route.

```console
$ uv run blastit check
```

## Deploy

`uv run blastit --preset digitalocean-paas` — or `compose-traefik` — runs the same file under real
uids. `DATABASE_URL` names the pool, migrations name the database
([the rule](../../deploy/anything-else.md#migrations)).

## Database settings

```python title="myshop/settings.py"
import dj_database_url

DATABASES = {"default": dj_database_url.config(conn_max_age=0)}
DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True
```

`conn_max_age=0` closes the connection with the request, and `server_side_binding = True` in
`OPTIONS` keeps psycopg from inlining values.

## Behind TLS

Do not set `SECURE_PROXY_SSL_HEADER`: Blastit sets `wsgi.url_scheme` from a trusted peer and the raw
`X-Forwarded-Proto` still reaches the app, so trusting it again is the bypass. Name the origin in
`CSRF_TRUSTED_ORIGINS` and the host in `ALLOWED_HOSTS`.

## Migrations

`manage.py migrate` runs outside the view workers, before the new code boots
([the rule](../../deploy/anything-else.md#migrations)).

## Where next

[Flask, SQLAlchemy and raw WSGI](flask.md) · [Static sites and SPAs](../static-and-spa.md).
