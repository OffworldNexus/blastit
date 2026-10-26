# The pytest plugin's tests

`python/tests/` covers the plugin of D3b and the manifest tooling of D3c
(`python/blastit/pytest_plugin.py`, `python/blastit/logs.py`,
`python/blastit/fold.py`, and the extension functions in
`crates/blastit-py/src/audit.rs`):

* `test_plugin_helpers.py` — the pure parts, no Django, no database: the wire
  form of a query, `--blastit-proxy` parsing, the observations document, the
  Django-level capture (with a fake connection and a fake session), and what a
  live server's log folds into.
* `test_logs.py` — blastit's event stream, and the normalised text of a
  `sql.denied` event turned back into something PostgreSQL's parser accepts.
* `test_fold.py` — `blastit-manifest fold`: which denials become grants, and the
  line-level edit `--write` makes (checked by parsing the result).
* `test_wire_capture.py` — what psycopg sends by itself, over a real connection
  and no Django at all: `Connection.execute`, `Cursor.execute`/`executemany`/
  `stream`, `ServerCursor`'s `DECLARE`, `TypeInfo.fetch`, and the statement
  deduplication between the two capture layers.
* `test_pytest_plugin.py` — real `pytest` sessions in a copy of `project/`, a
  tiny Django app with an unfinished `blastit.toml` next to it. That is the only
  honest way to test a pytest plugin: the options, the hooks, the terminal
  summary, the exit code and the xdist merge are all things only a session has.
* `project/` — the Django project those sessions run: a dozen views, one model, a
  `connection_created` receiver that runs a `SET`, and a manifest that is missing
  grants, needs `transaction_control`, has stale grants, has a route nothing
  exercises, has no route at all for one path a test asks for, grants none of the
  statements psycopg runs as a connection is established, and lets only one of
  the two routes that open a socket say `egress = true`.
* `test_live_server.py` + `live_project/` — the live server of D3c: a second
  tiny Django project whose tests are served by the *real* `blastit` binary, so
  what is checked is front-end traffic against the manifest. Those tests skip
  without root (or passwordless sudo), which is what starting blastit needs.

They need two things `cargo test` does not give them: the compiled extension
(`blastit._blastit`) plus the plugin entry point, installed in a virtualenv, and
a real PostgreSQL whose user may create databases.

```console
$ export PYO3_PYTHON=python3.14          # the interpreter the wheel is built for
$ scripts/build-wheel.sh                 # dist/blastit-*.whl
$ python3.14 -m venv /tmp/blastit-tests
$ /tmp/blastit-tests/bin/pip install dist/blastit-*.whl \
    pytest pytest-django pytest-xdist django 'psycopg[binary]'
$ DATABASE_URL=postgresql://blastit:ci-not-secret@127.0.0.1:5432/blastit \
    /tmp/blastit-tests/bin/pytest python/tests
```

`DATABASE_URL` (or `BLASTIT_TEST_PG_URL`, the spelling dbproxy's tests use) is
what the tests hand to the tiny project; without it they are skipped.

The plugin's own runs of `project/` and `live_project/` are subprocesses of this
suite, so `-n` on the outer run also works. On a machine without `pip`,
`uv venv` and `uv pip install` take the place of the three middle lines.
