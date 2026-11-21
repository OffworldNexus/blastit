"""The routes the Django leg drives, plus the ones that must be refused.

Every statement travels through Django's raw cursor, not the ORM, because the
point of the leg is the *exact* SQL a view runs: a grant matches the text, so a
query builder that quietly adds a column would be a different test.

The refusals answer `403` with the proxy's SQLSTATE. A denied statement makes
Django mark the connection broken; the next request opens a fresh one, which is
why the shared `connection` trait also grants the session time-zone statement.
"""

import os
import sys

from django.db import DatabaseError, connection
from django.http import JsonResponse
from django.urls import path


def _product(row):
    return {"id": row[0], "name": row[1], "price": str(row[2])}


def _denied(error: DatabaseError) -> JsonResponse:
    """The proxy's refusal, as the app reports it: never the statement or a secret."""
    cause = getattr(error, "__cause__", None)
    sqlstate = getattr(cause, "sqlstate", None) or getattr(error, "sqlstate", None)
    return JsonResponse({"denied": True, "sqlstate": sqlstate}, status=403)


def products(request, product_id=None):
    """The public read: either every product or one, parameterised."""
    with connection.cursor() as cursor:
        if product_id is None:
            cursor.execute("SELECT id, name, price FROM products ORDER BY id")
            rows = cursor.fetchall()
            return JsonResponse({"products": [_product(row) for row in rows]})
        cursor.execute(
            "SELECT id, name, price FROM products WHERE id = %s", [product_id]
        )
        row = cursor.fetchone()
    return JsonResponse({"product": _product(row) if row else None})


def orders(request):
    """The write: one parameterised insert and the parameterised read-back."""
    customer = int(request.POST.get("customer_id", "0"))
    total = request.POST.get("total", "0")
    with connection.cursor() as cursor:
        cursor.execute(
            "INSERT INTO orders (customer_id, total) VALUES (%s, %s)",
            [customer, total],
        )
        cursor.execute(
            "SELECT customer_id, total FROM orders WHERE customer_id = %s",
            [customer],
        )
        rows = cursor.fetchall()
    return JsonResponse(
        {
            "inserted": True,
            "orders": [{"customer_id": row[0], "total": str(row[1])} for row in rows],
        },
        status=201,
    )


def order_note(request):
    """An insert whose ungranted `note` column is the refusal."""
    customer = int(request.POST.get("customer_id", "0"))
    total = request.POST.get("total", "0")
    try:
        with connection.cursor() as cursor:
            cursor.execute(
                "INSERT INTO orders (customer_id, total, note) VALUES (%s, %s, %s)",
                [customer, total, "note"],
            )
    except DatabaseError as error:
        return _denied(error)
    return JsonResponse({"inserted": True}, status=201)


def peek(request):
    """A cross-table read no view was granted: refused by the proxy."""
    try:
        with connection.cursor() as cursor:
            cursor.execute("SELECT email, password_hash FROM customers ORDER BY id")
            rows = cursor.fetchall()
    except DatabaseError as error:
        return _denied(error)
    return JsonResponse({"customers": rows})


def insert_customer(request):
    """An `INSERT` into a table this view was not granted at all."""
    try:
        with connection.cursor() as cursor:
            cursor.execute(
                "INSERT INTO customers (email, password_hash) VALUES (%s, %s)",
                ["mallory@example.test", "not-a-real-hash"],
            )
    except DatabaseError as error:
        return _denied(error)
    return JsonResponse({"inserted": True}, status=201)


def insert_order_note_from_peek(request):
    """The order insert from a view with no insert grant: refused whole."""
    try:
        with connection.cursor() as cursor:
            cursor.execute(
                "INSERT INTO orders (customer_id, total, note) VALUES (%s, %s, %s)",
                [1, "1.00", "note"],
            )
    except DatabaseError as error:
        return _denied(error)
    return JsonResponse({"inserted": True}, status=201)


def purge(request):
    """An ungranted `DELETE`: refused, with the proxy's SQLSTATE in the log."""
    try:
        with connection.cursor() as cursor:
            cursor.execute("DELETE FROM orders")
    except DatabaseError as error:
        return _denied(error)
    return JsonResponse({"purged": True})


def _has_password(url: str) -> bool:
    """Whether the URL's userinfo carries a password (`user:secret@`)."""
    authority = url.split("://", 1)[-1].split("/", 1)[0]
    userinfo = authority.rsplit("@", 1)[0] if "@" in authority else ""
    return ":" in userinfo


def env(request):
    """The environment this view was handed: no credential, and the CPython."""
    url = os.environ.get("DATABASE_URL", "")
    return JsonResponse(
        {
            "database_url": url,
            "url_has_password": _has_password(url),
            "db_password_in_env": "DB_PASSWORD" in os.environ,
            "python": f"{sys.version_info[0]}.{sys.version_info[1]}",
            "executable": sys.executable,
        }
    )


urlpatterns = [
    path("products", products),
    path("products/<int:product_id>", products),
    path("orders", orders),
    path("orders/note", order_note),
    path("orders/purge", purge),
    path("customers/peek", peek),
    path("customers/insert", insert_customer),
    path("orders/note-insert", insert_order_note_from_peek),
    path("env", env),
]
