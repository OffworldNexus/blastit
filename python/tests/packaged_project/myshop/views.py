"""The views of the packaged project: one granted statement, reached over HTTP.

Nothing here runs in the pytest process: a real blastit serves it, so the
manifest is what decides whether the request works.
"""

from django.db import connection
from django.http import HttpResponse

#: The statement the manifest grants to the `packaged` route. It names no table
#: on purpose: the project needs no fixtures, no migrations and no data, so what
#: the test proves is that a project in this layout boots and serves at all.
GRANTED = "SELECT 1"


def packaged(request):
    """Runs the statement the manifest grants, and answers 200."""
    with connection.cursor() as cursor:
        cursor.execute(GRANTED)
        cursor.fetchall()
    return HttpResponse(b"ok", content_type="text/plain")
