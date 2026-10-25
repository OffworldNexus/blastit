"""The test suite the plugin records.

One request (or a few) per view, over a real PostgreSQL: what the plugin reports
is exactly this file's traffic, plus the SQL pytest-django itself runs around it
(which the plugin must ignore, because no route is responsible for it).

Nothing here knows about blastit or the manifest, which is the point — the
report is written from an ordinary Django test suite.

The last test drives the same view through `live_server`, i.e. over HTTP, from
the server's own thread: the case a real suite has when it also drives the app
through a browser, and the one a plugin that kept per-request state in a global
would get wrong.
"""

from urllib.request import urlopen

import pytest
from django.db import connection


@pytest.mark.django_db
def test_list_notes(client):
    """A granted SELECT."""
    response = client.get("/notes/")
    assert response.status_code == 200


@pytest.mark.django_db
def test_create_note(client):
    """A granted INSERT."""
    response = client.post("/notes/create/", {"body": "hello"})
    assert response.status_code == 201


@pytest.mark.django_db
def test_delete_note(client):
    """A DELETE the manifest does not grant."""
    response = client.post("/notes/4/delete/")
    assert response.status_code == 200
    assert b"deleted" in response.content


@pytest.mark.django_db
def test_atomic_note(client):
    """An UPDATE inside a nested atomic, i.e. a SAVEPOINT."""
    response = client.post("/notes/atomic/")
    assert response.status_code == 200


@pytest.mark.django_db
def test_id_in_lists_of_any_length(client):
    """The same `IN` list query with one id and with three: one grant (D3a)."""
    assert client.get("/notes/in/?ids=1").status_code == 200
    assert client.get("/notes/in/?ids=1,2,3").status_code == 200


@pytest.mark.django_db
def test_multi_row_values(client):
    """A hand-written two-row `INSERT`, then a five-row one: one grant (D3a)."""
    assert client.get("/notes/multi/?rows=2").status_code == 200
    assert client.get("/notes/multi/?rows=5").status_code == 200


@pytest.mark.django_db
def test_bulk_create_shapes(client):
    """Three rows in one `bulk_create`, then one: Django sends two shapes (D3a)."""
    assert client.get("/notes/bulk/?rows=3").status_code == 200
    assert client.get("/notes/bulk/?rows=1").status_code == 200


@pytest.mark.django_db
def test_type_info_fetch(client):
    """A type lookup through psycopg itself, as postgis and hstore do it."""
    response = client.get("/notes/types/")
    assert response.status_code == 200
    assert response.content.isdigit()


@pytest.mark.django_db
def test_unrouted_path(client):
    """A path blastit has no route for: it would answer 404 before Django."""
    response = client.get("/oops/")
    assert response.status_code == 200


@pytest.mark.django_db
def test_sql_outside_a_request_is_ignored():
    """A statement no request made belongs to no route: the report must not list it."""
    with connection.cursor() as cursor:
        cursor.execute("SELECT 4242")
        assert cursor.fetchone() == (4242,)


# `transaction=True` because the live server serves the request on a connection
# of its own, which cannot see the test's uncommitted transaction.
@pytest.mark.django_db(transaction=True)
def test_notes_over_live_server(live_server):
    """The same granted SELECT, served over HTTP from another thread."""
    with urlopen(f"{live_server.url}/notes/") as response:  # noqa: S310 - 127.0.0.1
        assert response.status == 200
