"""The workload every server serves, byte for byte.

A raw WSGI callable, no framework: what is measured is the server plus WSGI
dispatch, not a router. The same file is loaded by blastit, granian and
gunicorn; the only thing that differs behind it is ``DATABASE_URL`` — blastit
rewrites it to a local socket served by its dbproxy, the other two get the
direct one. Nothing in here knows which server it is running under.

Routes:

* ``/``      a tiny text response — pure request/response overhead;
* ``/json``  a small JSON document — response encoding and a bigger body;
* ``/db``    one SELECT through a pooled psycopg connection — the database
             path, which under blastit crosses dbproxy (its sandbox) and under
             the others talks to PostgreSQL directly;
* ``/work``  a little pure-Python CPU work — where server overhead should wash
             out once the app costs something.

Connections are pooled for all three servers. blastit's dbproxy opens one
upstream connection per inbound connection, so a pool keeps the comparison
about serving a request rather than about `connect(2)` on every request. The
pool size and every other knob are the same for the three.
"""

from __future__ import annotations

import json
import os

from psycopg_pool import ConnectionPool

# The statement `/db` runs. It is granted to blastit's route, verbatim, in
# `blastit.toml`, so the two must stay in sync.
DB_SQL = "SELECT id, name, payload FROM bench_item ORDER BY id LIMIT 10"

POOL_MAX = int(os.environ.get("BENCH_POOL_MAX", "6"))
WORK_ITERS = int(os.environ.get("BENCH_WORK_ITERS", "4000"))

_pool: ConnectionPool | None = None


def _pool_for() -> ConnectionPool:
    """The connection pool, created lazily so it is opened after any fork.

    blastit imports this module before its fork and serves requests in processes
    forked afterwards; a pool opened at import time would hand a live socket to
    a process that never created it.
    """
    global _pool
    if _pool is None:
        _pool = ConnectionPool(
            conninfo=os.environ["DATABASE_URL"],
            min_size=1,
            max_size=POOL_MAX,
            open=True,
            # autocommit: no BEGIN/COMMIT per request, so blastit's route needs
            # no transaction_control grant and all three servers put the same
            # bytes on the wire.
            # prepare_threshold=None: no auto-PREPARE after N executions, so the
            # statement text is the same on every request.
            kwargs={"autocommit": True, "prepare_threshold": None},
        )
    return _pool


def _db_body() -> bytes:
    """`/db`: one granted SELECT, through the pooled connection."""
    with _pool_for().connection() as conn:
        with conn.cursor() as cur:
            cur.execute(DB_SQL)
            rows = cur.fetchall()
    return json.dumps([{"id": r[0], "name": r[1], "payload": r[2]} for r in rows]).encode()


def _work_body() -> bytes:
    """`/work`: a fixed slice of pure-Python CPU, to see overhead wash out."""
    total = 0
    for i in range(WORK_ITERS):
        total += i * i
    return str(total).encode()


def application(environ, start_response):
    """The WSGI callable every server loads, routing on `PATH_INFO` alone."""
    path = environ.get("PATH_INFO", "/")
    if path == "/json":
        body = json.dumps({"ok": True, "server": "bench", "items": list(range(20))}).encode()
        content_type = "application/json"
    elif path == "/db":
        body = _db_body()
        content_type = "application/json"
    elif path == "/work":
        body = _work_body()
        content_type = "text/plain"
    else:
        body = b"hello from bench\n"
        content_type = "text/plain"

    start_response(
        "200 OK",
        [("Content-Type", content_type), ("Content-Length", str(len(body)))],
    )
    return [body]
