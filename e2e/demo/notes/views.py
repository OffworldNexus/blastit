"""A tiny notes app: log in, write and delete your own notes, log out.

Three extra endpoints exist to test the sandbox, not the app:

* ``/notes/peek/`` runs, from the notes route, a statement the manifest never
  granted: reading every user's password hash. Under blastit it must fail.
* ``/debug/sql/?q=…`` and ``/notes/sql/?q=…`` run ``q`` as raw SQL, verbatim.
  The same view is mounted in two routes, so the same text can be sent from
  two processes: blastit must let each process run only its own grants.
  Outside blastit this is a plain SQL injection hole; never serve it so.
* ``/debug/env/`` reports *facts* about the database configuration the app
  sees (never the values themselves): under blastit it holds no password and
  points at a local socket.
* ``/debug/egress/`` and ``/egress/probe/`` report what the process's system
  calls allow; only the second route may reach the network.
* ``/debug/caps/`` reports what the process's *capabilities* are: D6's proof
  that a rootless init still hands a view nothing to escalate with.
"""

import ctypes
import os
import socket
from urllib.parse import unquote, urlsplit

from django.contrib.auth import authenticate, login, logout
from django.contrib.auth.decorators import login_required
from django.contrib.auth.models import User
from django.db import DatabaseError, connection
from django.http import HttpResponse, HttpResponseNotAllowed, JsonResponse
from django.middleware.csrf import get_token
from django.shortcuts import get_object_or_404, redirect
from django.utils.html import escape
from django.views.decorators.http import require_POST

from .models import Note


def page(request, title, body):
    return HttpResponse(
        f"<!doctype html><title>{escape(title)}</title><h1>{escape(title)}</h1>{body}",
        content_type="text/html; charset=utf-8",
    )


def csrf_field(request):
    return f'<input type="hidden" name="csrfmiddlewaretoken" value="{get_token(request)}">'


def home(request):
    return page(request, "blastit demo", '<a href="/notes/">notes</a> · <a href="/login/">log in</a>')


def login_view(request):
    error = ""
    if request.method == "POST":
        user = authenticate(request, username=request.POST.get("username"), password=request.POST.get("password"))
        if user is not None:
            login(request, user)
            return redirect("/notes/")
        error = '<p class="error">Invalid username or password.</p>'
    elif request.method != "GET":
        return HttpResponseNotAllowed(["GET", "POST"])
    form = (
        f'<form method="post">{csrf_field(request)}'
        '<input name="username"><input name="password" type="password">'
        "<button>Log in</button></form>"
    )
    return page(request, "Log in", error + form)


@require_POST
def logout_view(request):
    logout(request)
    return redirect("/login/")


@login_required
def notes(request):
    if request.method == "POST":
        text = request.POST.get("text", "").strip()
        if text:
            Note.objects.create(owner=request.user, text=text)
        return redirect("/notes/")
    items = "".join(
        f'<li data-id="{n.id}">{escape(n.text)}'
        f'<form method="post" action="/notes/{n.id}/delete/">{csrf_field(request)}<button>delete</button></form></li>'
        for n in Note.objects.filter(owner=request.user).order_by("id")
    )
    form = f'<form method="post">{csrf_field(request)}<input name="text"><button>Add</button></form>'
    logout_form = f'<form method="post" action="/logout/">{csrf_field(request)}<button>Log out</button></form>'
    return page(request, f"Notes of {request.user.username}", f"<ul>{items}</ul>{form}{logout_form}")


@login_required
@require_POST
def delete_note(request, note_id):
    get_object_or_404(Note, id=note_id, owner=request.user).delete()
    return redirect("/notes/")


@login_required
def peek(request):
    hashes = list(User.objects.values_list("username", "password"))
    return JsonResponse({"users_read": len(hashes)})


def env(request):
    url = os.environ.get("DATABASE_URL", "")
    parts = urlsplit(url)
    host = unquote(parts.hostname or "") if parts.hostname else ""
    return JsonResponse(
        {
            "database_url_set": bool(url),
            "has_password": bool(parts.password),
            "host_is_local_socket": host.startswith("/"),
            "ca_cert_visible": "DATABASE_CA_CERT" in os.environ,
            "blastit_vars_visible": sorted(k for k in os.environ if k.startswith("BLASTIT_")),
            "route": request.META.get("blastit.route"),
        }
    )


def raw_sql(request):
    """Run ``q`` verbatim and report what happened, including the proxy's refusal.

    Mounted in two routes (`/debug/sql/` and `/notes/sql/`), so the same text
    can be sent from two processes: the proof is that each process can only run
    what its own route was granted, and a refused statement is refused by
    blastit's proxy (SQLSTATE 42501), not by the database's permissions.
    """
    route = request.META.get("blastit.route")
    try:
        with connection.cursor() as cursor:
            cursor.execute(request.GET.get("q", ""))
            rows = [list(map(str, r)) for r in cursor.fetchall()] if cursor.description else []
        return JsonResponse({"ok": True, "route": route, "rows": rows})
    except DatabaseError as e:
        sqlstate = getattr(e, "sqlstate", None) or getattr(e.__cause__, "sqlstate", None)
        return JsonResponse({"ok": False, "route": route, "sqlstate": sqlstate})


def caps_probe(request):
    """Report this process's own privilege state, without changing anything.

    Mounted on the ``debug`` route, which has no database grant. D6: a view
    process runs with **no capability at all** — not even the ones the rootless
    init that forked it holds — and with ``no_new_privs`` set, which is what
    keeps an ``execve`` of a binary with file capabilities from handing them
    back. The values are the raw ``/proc/self/status`` fields, so the check is
    the kernel's, not the app's.
    """
    fields = {}
    try:
        with open("/proc/self/status", encoding="ascii", errors="replace") as status:
            for line in status:
                key, separator, value = line.partition(":")
                if not separator:
                    continue
                if key.startswith("Cap"):
                    fields[key] = value.strip()
                elif key == "NoNewPrivs":
                    fields["NoNewPrivs"] = value.strip()
    except OSError as e:  # pragma: no cover - the kernel always has /proc
        return JsonResponse({"route": request.META.get("blastit.route"), "error": str(e)}, status=500)
    return JsonResponse(
        {
            "route": request.META.get("blastit.route"),
            "uid": os.getuid(),
            "inheritable": fields.get("CapInh"),
            "permitted": fields.get("CapPrm"),
            "effective": fields.get("CapEff"),
            "bounding": fields.get("CapBnd"),
            "ambient": fields.get("CapAmb"),
            "no_new_privs": fields.get("NoNewPrivs"),
        }
    )


def egress_probe(request):
    """Report what this process's system calls allow, without doing anything else.

    Mounted in two routes: ``/debug/egress/`` (no egress) and
    ``/egress/probe/`` (``egress = true``). The socket is created and closed
    again, never connected, and ``ptrace`` traces this very process. Under
    blastit's filters (ARCHITECTURE.md §7.3) both are refused unless the route
    was granted egress; outside blastit both are allowed.
    """
    result = {
        "route": request.META.get("blastit.route"),
        "inet": "allowed",
        "inet_errno": None,
        "ptrace": "allowed",
    }
    try:
        socket.socket(socket.AF_INET, socket.SOCK_STREAM).close()
    except OSError as e:
        result["inet"], result["inet_errno"] = "refused", e.errno

    libc = ctypes.CDLL(None, use_errno=True)
    libc.ptrace.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p]
    libc.ptrace.restype = ctypes.c_long
    if libc.ptrace(0, 0, None, None) != 0:  # PTRACE_TRACEME
        result["ptrace"] = "refused"
    return JsonResponse(result)
