"""Creates the table the SQLAlchemy app uses, outside blastit (E2b).

    DATABASE_URL=postgresql://user:password@host:5432/db python setup.py

It runs in the `migrate` service, with the real credentials and no sandbox in
the way — the same place `manage.py migrate` runs for the Django demo — because
a table is not something a route's grant can create: `CREATE TABLE` is not a
statement a grant is an allowlist for (§6).

The URL comes from the environment, which is where the rest of the image keeps
its credentials: `DATABASE_URL` is the variable the Django demo reads and the
one the manifests name as their `input`. Passing it as an argument as well would
be a second spelling of it, and a run's copy of the connection string would be
in `docker ps` and in the image's history.
"""

import os

import psycopg

with psycopg.connect(os.environ["DATABASE_URL"]) as connection:
    connection.execute(
        "CREATE TABLE IF NOT EXISTS sqlalchemy_notes ("
        " id serial PRIMARY KEY, body text NOT NULL)"
    )
    connection.execute("TRUNCATE sqlalchemy_notes RESTART IDENTITY")
print("the table the SQLAlchemy app uses is there")
