"""The views of the tiny project: each one is one shape of SQL.

* `note_list` sends the one `SELECT` the manifest grants;
* `note_create` sends the one `INSERT` the manifest grants, next to a grant
  nothing uses;
* `note_delete` sends a `DELETE` the manifest does **not** grant, which is what
  the report is for;
* `note_atomic` wraps its `UPDATE` in a nested `atomic`, which is a `SAVEPOINT`
  under pytest-django: the route needs `transaction_control`, not a grant for a
  statement whose name changes every run;
* `note_in` filters on `__in` with whatever list the request carries, so the
  statement is `IN ($1)`, `IN ($1, $2, $3)`… — one shape to a grant, since D3a;
* `note_multi` writes a multi-row `INSERT … VALUES (…), (…)` by hand, the way a
  bulk upsert does, so the run of rows D3a collapses is on the wire;
* `note_bulk` inserts whatever number of rows the request asks for with
  `bulk_create`: Django sends a single `VALUES` row for one object and an
  `UNNEST` array for several, so the row count changes the *statement*, not just
  its parameter count — two shapes, and both have to be granted;
* `note_types` looks a type's OID up the way the postgis and hstore backends do
  at connect, through psycopg itself — a statement no Django cursor of the
  request sees;
* `note_stream` walks the table with `QuerySet.iterator()`, which Django gives a
  named (server-side) cursor: what goes on the wire is a `DECLARE … CURSOR FOR …`
  and a `FETCH`, not the `SELECT` the code reads;
* `note_atomic_live` is the same `UPDATE`-in-`atomic()` as `note_atomic`, but
  run from a `transaction=True` test over the live server, so psycopg sends the
  `BEGIN`/`COMMIT` itself;
* `note_egress` opens a TCP connection, the way a cache or a queue client does.
"""

import socket

from django.db import connection, transaction
from django.http import HttpResponse
from psycopg.types import TypeInfo

from notes.models import Note

OWNER = 1


def note_list(request):
    """`GET /notes/`: the granted `SELECT`."""
    ids = Note.objects.filter(owner_id=OWNER).values_list("id", flat=True)
    return HttpResponse("\n".join(str(pk) for pk in ids))


def note_create(request):
    """`POST /notes/create/`: the granted `INSERT`."""
    note = Note.objects.create(owner_id=OWNER, body=request.POST.get("body", "x"))
    return HttpResponse(str(note.pk), status=201)


def note_delete(request, pk):
    """`POST /notes/<pk>/delete/`: a `DELETE` no grant covers."""
    deleted, _ = Note.objects.filter(pk=pk).delete()
    return HttpResponse(f"deleted {deleted}")


def note_atomic(request):
    """`POST /notes/atomic/`: an `UPDATE` inside a nested transaction."""
    with transaction.atomic():
        Note.objects.filter(owner_id=OWNER).update(body="touched")
    return HttpResponse("ok")


def note_atomic_live(request):
    """`POST /notes/atomic-live/`: an `UPDATE` in a transaction of its own.

    Used from a `transaction=True` test over the live server, where no test
    transaction is around the request: psycopg sends the `BEGIN` and the
    `COMMIT` itself, so this route is the one that proves the plugin sees them.
    """
    with transaction.atomic():
        Note.objects.filter(owner_id=OWNER).update(body="live")
    return HttpResponse("ok")


def note_in(request):
    """`GET /notes/in/?ids=1,2,3`: an `IN` list whose length is the request's."""
    ids = [int(value) for value in request.GET.get("ids", "").split(",") if value]
    found = Note.objects.filter(id__in=ids).values_list("id", flat=True)
    return HttpResponse("\n".join(str(pk) for pk in found))


def note_multi(request):
    """`GET /notes/multi/?rows=2`: a hand-written multi-row `INSERT`.

    What a bulk upsert looks like when the ORM's shape does not fit: a run of
    `VALUES` rows whose length is the request's, which D3a collapses to one
    grant.
    """
    rows = int(request.GET.get("rows", "2"))
    placeholders = ", ".join(["(%s, %s)"] * rows)
    params = [value for row in range(rows) for value in (OWNER, f"row {row}")]
    with connection.cursor() as cursor:
        cursor.execute(
            'INSERT INTO "notes_note" ("owner_id", "body") VALUES '
            f'{placeholders} RETURNING "notes_note"."id"',
            params,
        )
        ids = [row[0] for row in cursor.fetchall()]
    return HttpResponse(",".join(str(pk) for pk in ids))


def note_bulk(request):
    """`GET /notes/bulk/?rows=1`: a `bulk_create` whose row count is the request's."""
    rows = int(request.GET.get("rows", "1"))
    Note.objects.bulk_create(
        [Note(owner_id=OWNER, body=f"row {index}") for index in range(rows)]
    )
    return HttpResponse(f"created {rows}")


def note_types(request):
    """`GET /notes/types/`: a type lookup through psycopg, as postgis does it."""
    info = TypeInfo.fetch(connection.connection, "text")
    return HttpResponse(str(info.oid) if info is not None else "unknown")


def note_stream(request):
    """`GET /notes/stream/`: `QuerySet.iterator()`, i.e. a named cursor.

    Django hands the queryset a `chunked_cursor()`, so what PostgreSQL receives
    is `DECLARE … CURSOR FOR …` and a `FETCH`, not the `SELECT` in this file.
    """
    notes = 0
    for _ in Note.objects.iterator():
        notes += 1
    return HttpResponse(f"{notes} notes")


def note_egress(request):
    """`GET /notes/cache/`: opens a TCP connection, as a cache client does.

    Nothing listens on the discard port, so the attempt fails — which is still
    an attempt, and `egress = true` is exactly about being allowed to make one.
    """
    try:
        with socket.create_connection(("127.0.0.1", 9), timeout=0.05):
            pass
    except OSError:
        pass
    return HttpResponse("tried")


def oops(request):
    """`GET /oops/`: a view blastit has no route for, so it would answer 404."""
    with connection.cursor() as cursor:
        cursor.execute("SELECT 1")
        cursor.fetchone()
    return HttpResponse("django served this, blastit would not")
