# End-to-end test

A small Django app (`demo/`: log in, add and delete your own notes, log out) and
a small Flask + SQLAlchemy app (`sqlalchemy/`) served by blastit against a real
PostgreSQL. The Django app proves the concept rather than the parts:

* the user flow works unchanged under blastit;
* the notes route can't run a statement the manifest doesn't grant (reading
  every user's password hash, `/notes/peek/`): it is refused and logged as
  `sql.denied`;
* the app's `DATABASE_URL` points at a local socket and holds no password,
  and the password never appears in the logs.

The Flask + SQLAlchemy app proves the same design for a framework that is not
Django (E2b): it parses `DATABASE_URL` itself, and it does not percent-decode a
host — so it is the check that blastit renders the socket directory as the
`host` **query parameter** every client reads, not as a percent-encoded
authority. It also runs connection statements of its own (`select
pg_catalog.version()`, `select current_schema()`, two `show`s, a type lookup)
and a `BEGIN`/`COMMIT` per request, which is what the manifest's `connection`
group and `transaction_control` are for (§6) — one route is refused a `DELETE`
nobody granted, to show the limits are real.

```console
$ uv run e2e/run.py                                   # both apps, in Docker
$ uv run e2e/scenario.py http://127.0.0.1:8000 --expect baseline   # same app under gunicorn
```

`scenario.py` drives the Django app over HTTP and `check_logs.py` checks
blastit's stdout afterwards; `sqlalchemy_check.py` does both for the second app
(its port is `E2E_SQLALCHEMY_PORT`, `18081` by default). All three are
stdlib-only.

Two Dockerfiles, the same apps either way:

* `Dockerfile` builds the wheel in the image (`scripts/build-wheel.sh`) and
  installs it with pip, so the demo runs exactly the install `pip install
  blastit` gives a project — the console script, on the image's interpreter.
* `Dockerfile.prebuilt` installs a wheel built beforehand
  (`dist/blastit-*-cp313-*.whl`, for the image's CPython 3.13); it is what CI
  uses, one `maturin build` on the runner instead of a Rust toolchain in the
  image.

`venv-smoke.sh` is the other half of the packaging test: the wheel installed
into a project's *virtualenv* (uv, on the `modelw/base` image the pilot uses),
started as root through `sudo .venv/bin/blastit`, checked for a served request
and for `boot`/`ready` in the event stream. Docker only:

```console
$ scripts/build-wheel.sh && e2e/venv-smoke.sh
```

## The manifest

`demo/blastit.toml` is generated, then committed. Run the app outside blastit
with `DEMO_HARVEST=/tmp/harvest.jsonl` (the middleware in `notes/harvest.py`
records each statement as psycopg sends it), run the baseline scenario
against it, then:

```console
$ uv run e2e/grants.py /tmp/harvest.jsonl > e2e/demo/blastit.toml
```

The routes are listed in `grants.py`. `/notes/peek/`'s own statement is left
out on purpose.

`/notes/sql/?q=…` and `/debug/sql/?q=…` run `q` as raw SQL and report what
happened. The same view is mounted in two routes, so the same text can be sent
from two different processes: each one may run only the statements its own
route was granted. `grants.py` gives the `sql` route exactly one statement
(`SELECT count(*) FROM notes_note`) and nothing else, so the `notes` route is
refused that same text and vice versa. The checks in `scenario.py` send the
same text from both processes and from a third route that has no grants.

`/debug/egress/` and `/egress/probe/` run the same probe — create (and close) an
`AF_INET` socket, and `ptrace(PTRACE_TRACEME)` this process — and report what
happened. The first is on the `debug` route, the second on `egress` with
`egress = true` and no grants, so under blastit the §7.3 filters refuse the
socket on one route and not the other. A route is marked egress with the fifth
column of `grants.py`'s `ROUTES`.
