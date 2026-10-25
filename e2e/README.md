# End-to-end test

A small Django app (`demo/`: log in, add and delete your own notes, log out)
served by blastit against a real PostgreSQL. It proves the concept rather than
the parts:

* the user flow works unchanged under blastit;
* the notes route can't run a statement the manifest doesn't grant (reading
  every user's password hash, `/notes/peek/`): it is refused and logged as
  `sql.denied`;
* the app's `DATABASE_URL` points at a local socket and holds no password,
  and the password never appears in the logs.

```console
$ uv run e2e/run.py                                   # under blastit, in Docker
$ uv run e2e/scenario.py http://127.0.0.1:8000 --expect baseline   # same app under gunicorn
```

`scenario.py` drives the app over HTTP; `check_logs.py` checks blastit's
stdout afterwards. Both are stdlib-only.

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
