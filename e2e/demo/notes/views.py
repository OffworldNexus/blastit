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
* ``/debug/request/`` reports who blastit says the client is
  (``REMOTE_ADDR``), which scheme it derived, and which forwarded headers
  arrived: E1's proof that a preset's trust rules are the ones in force.
* ``/debug/caps/`` reports what the process's *capabilities* are: D6's proof
  that a rootless init still hands a view nothing to escalate with. It asks the
  kernel through ``prctl(2)``, and reports ``/proc/self/status`` beside it,
  because a platform is free to print less than a kernel does: gVisor, the App
  Platform runtime, has no ``CapAmb`` and no ``NoNewPrivs`` line (§9.1).
"""

import ctypes
import os
import socket
from urllib.parse import parse_qsl, unquote, urlsplit

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


def local_host(url):
    """The host — a socket directory, here — a rendered `DATABASE_URL` names.

    blastit renders a libpq URI whose socket directory travels as a **query
    parameter**, `postgresql://blastit@/shop?host=/proc/self/cwd&port=5432`: the
    authority is empty, which is how libpq and SQLAlchemy both spell a Unix
    socket, and — unlike a percent-encoded host, which SQLAlchemy does not decode
    — it is read by every client. A project that parses the URL itself has to
    look in both places, which is what this does.
    """
    parts = urlsplit(url)
    query = dict(parse_qsl(parts.query))
    if parts.hostname:
        return unquote(parts.hostname)
    return unquote(query.get("host", ""))


def env(request):
    url = os.environ.get("DATABASE_URL", "")
    parts = urlsplit(url)
    return JsonResponse(
        {
            "database_url_set": bool(url),
            "has_password": bool(parts.password),
            "host_is_local_socket": local_host(url).startswith("/"),
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


# `/proc/self/status` is not the only place these answers live, and on gVisor
# (the App Platform runtime, §9.1) it is not a place at all: the file has no
# `CapAmb` and no `NoNewPrivs` line. So the probe asks the *kernel* directly,
# through `prctl(2)`, and reports the file's fields beside that as information.
#
# The numbers are the kernel's (`<linux/prctl.h>`):
#
# * `PR_GET_NO_NEW_PRIVS` returns the flag itself, 1 or 0;
# * `PR_CAP_AMBIENT` (47) with `PR_CAP_AMBIENT_IS_SET` (1) answers for one
#   capability number at a time, and a kernel without ambient capabilities at
#   all — before 4.3, gVisor before release-20260727.0, which leaves the
#   sub-command to its `EINVAL` default — refuses it: there are none to report,
#   which is the same fact an empty set would state, and the errno is what tells
#   the two apart;
# * `PR_CAPBSET_READ` (23) answers whether one capability is still in the
#   bounding set, which is what a later `execve` could still be granted.
#
# `PR_CAP_AMBIENT_IS_SET` is **1**, not 3: 3 is `PR_CAP_AMBIENT_LOWER`, which
# would clear an ambient capability instead of reporting it and would answer 0
# for every capability, so a probe that used it could never see one.
PR_GET_NO_NEW_PRIVS = 39
PR_CAPBSET_READ = 23
PR_CAP_AMBIENT = 47
PR_CAP_AMBIENT_IS_SET = 1
# `CAP_CHECKPOINT_RESTORE` (40), the last capability Linux 6.x names. The loop
# is bounded by this constant rather than by the first `EINVAL`: a number the
# kernel does not know and a sub-command it does not know both answer `EINVAL`,
# and the two mean different things here.
CAP_LAST = 40

_LIBC = None


def _prctl(subcommand, *args):
    """One ``prctl(2)`` call, through ``libc``, as ``(result, errno)``.

    There is no Python wrapper for ``prctl``, so the call goes through
    ``ctypes``. On success the result is what the kernel returned and the errno
    is ``None``; on failure the result is ``None`` and the errno is the one the
    call set. ``args`` are the integer arguments after the sub-command, of which
    the kernel takes at most four.
    """
    global _LIBC
    if _LIBC is None:
        libc = ctypes.CDLL(None, use_errno=True)
        libc.prctl.argtypes = [ctypes.c_int, *(ctypes.c_ulong,) * 4]
        libc.prctl.restype = ctypes.c_int
        _LIBC = libc
    ctypes.set_errno(0)
    result = _LIBC.prctl(subcommand, *[*args, 0, 0, 0, 0][:4])
    if result == -1:
        return None, ctypes.get_errno()
    return result, None


def _kernel_set(subcommand, *prefix):
    """The capabilities the kernel says are set, by number, as ``(caps, errno)``.

    Both questions the probe asks are one ``prctl`` per capability number:
    ``PR_CAPBSET_READ`` for the bounding set, and ``PR_CAP_AMBIENT`` with
    ``PR_CAP_AMBIENT_IS_SET`` for the ambient one (``prefix`` is that
    sub-command). A kernel that does not know the sub-command answers the first
    call with ``EINVAL``, and then there is no capability to report: the errno
    is what tells "none" apart from an implementation that failed.
    """
    caps = []
    for cap in range(CAP_LAST + 1):
        answer, error = _prctl(subcommand, *prefix, cap, 0, 0)
        if error is not None:
            return caps, error
        if answer:
            caps.append(cap)
    return caps, None


def request_probe(request):
    """Report who the server says the client is, and how it read the transport.

    E1: a configuration preset is mostly a decision about *whom to believe* —
    which client-IP header, from which peers — so the only way to check one is to
    ask the app what blastit resolved. ``REMOTE_ADDR`` is that decision — the
    socket peer's address, or an address a trusted header named — and it is an
    address and nothing else: the port of the connection is the separate
    ``REMOTE_PORT`` key, as CGI and PEP 3333 have it.
    ``wsgi.url_scheme`` is the same decision applied to
    ``X-Forwarded-Proto``: ``https`` only when a peer blastit trusts said so,
    which is why a client's own ``X-Forwarded-Proto`` cannot make a plaintext
    deployment report HTTPS.

    The headers are reported as booleans, never echoed back: this is a probe of
    what the deployment decided, not a header dump.
    """
    return JsonResponse(
        {
            "route": request.META.get("blastit.route"),
            "remote_addr": request.META.get("REMOTE_ADDR"),
            "remote_port": request.META.get("REMOTE_PORT"),
            "scheme": request.META.get("wsgi.url_scheme"),
            "host": request.META.get("HTTP_HOST"),
            "saw_forwarded_for": "HTTP_X_FORWARDED_FOR" in request.META,
            "saw_real_ip": "HTTP_X_REAL_IP" in request.META,
            "saw_forwarded_proto": "HTTP_X_FORWARDED_PROTO" in request.META,
            "saw_do_connecting_ip": "HTTP_DO_CONNECTING_IP" in request.META,
        }
    )


def caps_probe(request):
    """Report this process's own privilege state, without changing anything.

    Mounted on the ``debug`` route, which has no database grant. D6: a view
    process runs with **no capability at all** — not even the ones the rootless
    init that forked it holds — and with ``no_new_privs`` set, which is what
    keeps an ``execve`` of a binary with file capabilities from handing them
    back.

    The answers are the kernel's, asked through ``prctl(2)``: the runtime is
    allowed to print less in ``/proc/self/status`` than the kernel knows, and
    gVisor does (no ``CapAmb``, no ``NoNewPrivs``). The fields of that file are
    reported beside them for the platforms that do print it — ``null`` where
    they are absent — and ``proc_error`` says why they are missing when the file
    itself cannot be read.
    """
    fields = {}
    proc_error = None
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
    except OSError as e:  # pragma: no cover - a kernel always has /proc, a sandbox may not
        proc_error = str(e)
    no_new_privs, no_new_privs_error = _prctl(PR_GET_NO_NEW_PRIVS)
    ambient, ambient_error = _kernel_set(PR_CAP_AMBIENT, PR_CAP_AMBIENT_IS_SET)
    bounding, bounding_error = _kernel_set(PR_CAPBSET_READ)
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
            "proc_error": proc_error,
            "prctl": {
                "no_new_privs": no_new_privs,
                "no_new_privs_errno": no_new_privs_error,
                "ambient": ambient,
                "ambient_errno": ambient_error,
                "bounding": bounding,
                "bounding_errno": bounding_error,
            },
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
