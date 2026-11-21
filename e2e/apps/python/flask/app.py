"""The Flask + SQLAlchemy shop.

It exists for what the Django leg cannot show: SQLAlchemy parses the rendered
`DATABASE_URL` itself and does not percent-decode a host, so the socket
directory has to arrive as the libpq `host` query parameter. It also runs
connection statements of its own (`pg_catalog.version()`, `current_schema()`,
two `show`s, psycopg's `hstore` OID lookup and a type lookup) and a
`BEGIN`/`COMMIT` per `engine.begin()`, which is what the manifest's shared
`connection` trait grants.

`NullPool` gives each request its own connection, as the framework page
documents: a pooled connection would outlive the request (and its fork) and hide
the connection lifecycle this leg exists to test.
"""

import os
import sys

from flask import Flask, jsonify, request
from sqlalchemy import create_engine, text
from sqlalchemy.exc import DBAPIError
from sqlalchemy.pool import NullPool

app = Flask(__name__)

_url = os.environ["DATABASE_URL"].replace("postgresql://", "postgresql+psycopg://", 1)
engine = create_engine(_url, poolclass=NullPool)


def _product(row):
    return {"id": row[0], "name": row[1], "price": str(row[2])}


def _denied(error: DBAPIError):
    """The proxy's refusal, as the app reports it: never the statement or a secret."""
    sqlstate = getattr(getattr(error, "orig", None), "sqlstate", None)
    return jsonify({"denied": True, "sqlstate": sqlstate}), 403


@app.get("/products")
def products():
    with engine.begin() as connection:
        rows = connection.execute(
            text("SELECT id, name, price FROM products ORDER BY id")
        ).all()
    return jsonify({"products": [_product(row) for row in rows]})


@app.get("/products/<int:product_id>")
def product(product_id):
    with engine.begin() as connection:
        row = connection.execute(
            text("SELECT id, name, price FROM products WHERE id = :id"),
            {"id": product_id},
        ).first()
    return jsonify({"product": _product(row) if row else None})


@app.post("/orders")
def orders():
    customer = int(request.form.get("customer_id", "0"))
    total = request.form.get("total", "0")
    with engine.begin() as connection:
        connection.execute(
            text("INSERT INTO orders (customer_id, total) VALUES (:customer, :total)"),
            {"customer": customer, "total": total},
        )
        rows = connection.execute(
            text("SELECT customer_id, total FROM orders WHERE customer_id = :c"),
            {"c": customer},
        ).all()
    return (
        jsonify(
            {
                "inserted": True,
                "orders": [
                    {"customer_id": row[0], "total": str(row[1])} for row in rows
                ],
            }
        ),
        201,
    )


@app.post("/orders/note")
def order_note():
    """An insert whose ungranted `note` column is the refusal."""
    try:
        with engine.begin() as connection:
            connection.execute(
                text(
                    "INSERT INTO orders (customer_id, total, note)"
                    " VALUES (:customer, :total, :note)"
                ),
                {"customer": 1, "total": "1.00", "note": "note"},
            )
    except DBAPIError as error:
        return _denied(error)
    return jsonify({"inserted": True}), 201


@app.get("/customers/peek")
def peek():
    """A cross-table read no view was granted: refused by the proxy."""
    try:
        with engine.begin() as connection:
            rows = connection.execute(
                text("SELECT email, password_hash FROM customers ORDER BY id")
            ).all()
    except DBAPIError as error:
        return _denied(error)
    return jsonify({"customers": [list(row) for row in rows]})


@app.get("/customers/insert")
def insert_customer():
    """An `INSERT` into a table this view was not granted at all."""
    try:
        with engine.begin() as connection:
            connection.execute(
                text(
                    "INSERT INTO customers (email, password_hash)"
                    " VALUES (:email, :password_hash)"
                ),
                {"email": "mallory@example.test", "password_hash": "not-a-real-hash"},
            )
    except DBAPIError as error:
        return _denied(error)
    return jsonify({"inserted": True}), 201


@app.get("/orders/note-insert")
def insert_order_note_from_peek():
    """The order insert from a view with no insert grant: refused whole."""
    try:
        with engine.begin() as connection:
            connection.execute(
                text(
                    "INSERT INTO orders (customer_id, total, note)"
                    " VALUES (:customer, :total, :note)"
                ),
                {"customer": 1, "total": "1.00", "note": "note"},
            )
    except DBAPIError as error:
        return _denied(error)
    return jsonify({"inserted": True}), 201


@app.post("/orders/purge")
def purge():
    """An ungranted `DELETE`: refused, with the proxy's SQLSTATE in the log."""
    try:
        with engine.begin() as connection:
            connection.execute(text("DELETE FROM orders"))
    except DBAPIError as error:
        return _denied(error)
    return jsonify({"purged": True})


def _has_password(url: str) -> bool:
    """Whether the URL's userinfo carries a password (`user:secret@`)."""
    authority = url.split("://", 1)[-1].split("/", 1)[0]
    userinfo = authority.rsplit("@", 1)[0] if "@" in authority else ""
    return ":" in userinfo


@app.get("/env")
def env():
    url = os.environ.get("DATABASE_URL", "")
    return jsonify(
        {
            "database_url": url,
            "url_has_password": _has_password(url),
            "db_password_in_env": "DB_PASSWORD" in os.environ,
            "python": f"{sys.version_info[0]}.{sys.version_info[1]}",
            "executable": sys.executable,
        }
    )
