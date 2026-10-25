"""The views of the tiny project: each one is one shape of SQL.

* `note_list` sends the one `SELECT` the manifest grants;
* `note_create` sends the one `INSERT` the manifest grants, next to a grant
  nothing uses;
* `note_delete` sends a `DELETE` the manifest does **not** grant, which is what
  the report is for;
* `note_atomic` wraps its `UPDATE` in a nested `atomic`, which is a `SAVEPOINT`
  under pytest-django: the route needs `transaction_control`, not a grant for a
  statement whose name changes every run.
"""

from django.db import connection, transaction
from django.http import HttpResponse

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


def oops(request):
    """`GET /oops/`: a view blastit has no route for, so it would answer 404."""
    with connection.cursor() as cursor:
        cursor.execute("SELECT 1")
        cursor.fetchone()
    return HttpResponse("django served this, blastit would not")
