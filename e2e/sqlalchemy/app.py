"""A minimal Flask + SQLAlchemy app, served by blastit (E2b).

It exists for the two things the Django demo cannot show:

* **the rendered `DATABASE_URL` read by SQLAlchemy.** SQLAlchemy parses a URL
  itself and hands the fields to the driver, and it does *not* percent-decode a
  host — a socket directory written the libpq way (`postgresql://user@%2Fproc
  %2Fself%2Fcwd:5432/db`) reaches psycopg as the literal host
  `%2Fproc%2Fself%2Fcwd`, which the driver then puts through DNS. blastit
  renders the socket directory as a **query parameter** instead
  (`postgresql://blastit@/shop?host=/proc/self/cwd&port=5432`), which every
  client reads; this app is the proof, because a wrong host here is a `500`.
* **SQLAlchemy's own connection statements.** Opening a connection runs
  `select pg_catalog.version()`, `select current_schema()`, `show
  standard_conforming_strings`, `show transaction isolation level` and a type
  lookup, and every transaction is `BEGIN`/`COMMIT` (`SAVEPOINT`/`RELEASE` for a
  nested one). None of that is a *request's* statement, which is exactly what
  the manifest's `connection` group and `transaction_control` are for (§6): the
  app below works because `sqlalchemy/blastit.toml` grants them, not because
  blastit lets a connection do anything it likes.

Two routes, two grants: the read route may run its `SELECT` and nothing else,
the write route its `INSERT`. Anything else a route runs is refused by dbproxy
with SQLSTATE `42501`, and the `sql.denied` event that goes with it is what
`sqlalchemy_check.py` looks for in the log.
"""

import os

from flask import Flask, jsonify
from sqlalchemy import create_engine, text
from sqlalchemy.engine import make_url

app = Flask(__name__)

#: What blastit rendered for the app: no password, and the view's own socket
#: directory as the `host` parameter. SQLAlchemy's `postgresql+psycopg://`
#: dialect hands `host` and `port` straight to `psycopg.connect`.
URL = make_url(os.environ["DATABASE_URL"])
engine = create_engine(URL.set(drivername="postgresql+psycopg"), pool_pre_ping=False)


@app.get("/notes/")
def notes():
    """Reads every note, and reports the URL it was pointed at."""
    with engine.begin() as connection:
        rows = connection.execute(
            text("SELECT id, body FROM sqlalchemy_notes ORDER BY id")
        ).all()
    return jsonify(
        {
            "notes": [{"id": row[0], "body": row[1]} for row in rows],
            "database_url": os.environ["DATABASE_URL"],
        }
    )


@app.post("/notes/add/")
def add():
    """Writes one note, and answers with how many there are now."""
    with engine.begin() as connection:
        connection.execute(
            text("INSERT INTO sqlalchemy_notes (body) VALUES (:body)"),
            {"body": "sqlalchemy"},
        )
        total = connection.execute(text("SELECT count(*) FROM sqlalchemy_notes")).scalar_one()
    return jsonify({"added": True, "count": total})


@app.post("/notes/clear/")
def clear():
    """Deletes every note — which no route of this manifest is granted.

    The route *has* a grant on the proxy (its own `SELECT`), so it gets a socket
    and the statement travels as far as the proxy; the proxy refuses it with
    SQLSTATE `42501`, the view dies with the driver's error, and Flask answers
    `500`. That is the difference between a route that cannot reach the database
    and one that reaches it and is limited by its grant (§6).
    """
    with engine.begin() as connection:
        connection.execute(text("DELETE FROM sqlalchemy_notes"))
    return jsonify({"cleared": True})


# The console script serves this app from the directory it is started in, so no
# `PYTHONPATH` is involved (ARCHITECTURE.md §5.3).
application = app
