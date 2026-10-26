"""The two views of the live project: one granted statement and one that is not.

Both are reached over HTTP by the tests, through a real blastit: nothing here
runs in the pytest process, so the manifest is what decides whether they work —
which is exactly the traffic a Playwright suite produces.
"""

from django.db import connection
from django.http import HttpResponse

#: The statement the manifest grants to the `live-ok` route, and nothing else.
#: It names no table on purpose: the live project needs no fixtures, no
#: migrations and no data, so what the test proves is the routing and the grant,
#: not a table's contents.
GRANTED = "SELECT 1"

#: The statement the `live-denied` route sends. A grant is an allowlist, so any
#: statement the manifest does not list is refused and logged as `sql.denied`.
DENIED = "SELECT 'denied'"


def granted(request):
    """Runs the statement the manifest grants, and answers 200."""
    with connection.cursor() as cursor:
        cursor.execute(GRANTED)
        cursor.fetchall()
    return HttpResponse(b"ok", content_type="text/plain")


def denied(request):
    """Runs a statement no grant lists, which blastit refuses (a 500 here)."""
    with connection.cursor() as cursor:
        cursor.execute(DENIED)
        cursor.fetchall()
    return HttpResponse(b"ok", content_type="text/plain")
