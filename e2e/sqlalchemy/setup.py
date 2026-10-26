"""Creates the table the SQLAlchemy app uses, outside blastit (E2b).

    setup.py postgresql://user:password@host:5432/db

It runs in the `migrate` service, with the real credentials and no sandbox in
the way — the same place `manage.py migrate` runs for the Django demo — because
a table is not something a route's grant can create: `CREATE TABLE` is not a
statement a grant is an allowlist for (§6).
"""

import sys

import psycopg

with psycopg.connect(sys.argv[1]) as connection:
    connection.execute(
        "CREATE TABLE IF NOT EXISTS sqlalchemy_notes ("
        " id serial PRIMARY KEY, body text NOT NULL)"
    )
    connection.execute("TRUNCATE sqlalchemy_notes RESTART IDENTITY")
print("the table the SQLAlchemy app uses is there")
