# The pytest plugin's tests

`python/tests/` covers the plugin of D3b
(`python/blastit/pytest_plugin.py`, the extension functions in
`crates/blastit-py/src/audit.rs`):

* `test_plugin_helpers.py` — the pure parts, no Django, no database: the wire
  form of a query, `--blastit-proxy` parsing, the observations document.
* `test_pytest_plugin.py` — real `pytest` sessions in a copy of `project/`, a
  tiny Django app with an unfinished `blastit.toml` next to it. That is the only
  honest way to test a pytest plugin: the options, the hooks, the terminal
  summary, the exit code and the xdist merge are all things only a session has.
* `project/` — the Django project those sessions run: five views, one model,
  and a manifest that is missing a grant, needs `transaction_control`, has a
  stale grant, has a route nothing exercises and has no route at all for one
  path a test asks for.

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

The plugin's own runs of `project/` are subprocesses of this suite, so `-n` on
the outer run also works. On a machine without `pip`, `uv venv` and
`uv pip install` take the place of the three middle lines.
