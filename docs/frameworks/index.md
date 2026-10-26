# Fit it to your framework

This page is what is true of Django and Flask alike.

## No framework integration

blastit loads one WSGI callable and calls it: `BLASTIT_APP`, written
`module:attribute` — `myproject.wsgi:application` for Django, `app:app` for
Flask. Your framework keeps its own routing, because which process serves a
request comes from `blastit.toml`, not from your URL configuration. Nothing to
install, nothing to import, no `manage.py` command.

## What blastit needs from an app

### Send parameters separately from the SQL text

Statements are matched by their text, so it must be identical on every request:
a client that inlines literals sends a different string every time.

* **psycopg 3** used directly — fine: parameters travel as `$1`.
* **Django with psycopg 3** —
  `DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True`.
* **SQLAlchemy** — `postgresql+psycopg://`.
* **psycopg2** — not usable: it cannot bind server-side.

### Don't keep connections across requests

Each request opens its own connection and then throws its thread away, so a
connection that outlives the request is orphaned.

* Django: `CONN_MAX_AGE = 0`
* SQLAlchemy: `poolclass=NullPool`
* Flask-SQLAlchemy: `SQLALCHEMY_ENGINE_OPTIONS = {"poolclass": NullPool}`

Pool in front of the real database: PgBouncer, or DigitalOcean's pools.

## Routes and URL paths

A route's `path` is a list of Rust
[regexes](https://docs.rs/regex/latest/regex/) matched against the **whole** of
`PATH_INFO`. Routes are tried in manifest order; the first match wins, so a
catch-all goes last.

* `methods = ["GET", "HEAD"]` is optional, and `HEAD` is not implied by `GET`.
* A query string is not part of `PATH_INFO`.
* `BLASTIT_SCRIPT_NAME` strips a mount prefix and sets `SCRIPT_NAME`.
* `path = ['.*']` last lets your app render its own 404s.

## Next

[Day-to-day life](day-to-day.md), then
[fit it to your infra](../infra/index.md).
