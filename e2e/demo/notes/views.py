"""A tiny notes app: log in, write and delete your own notes, log out.

Two extra endpoints exist to test the sandbox, not the app:

* ``/notes/peek/`` runs, from the notes route, a statement the manifest never
  granted: reading every user's password hash. Under blastit it must fail.
* ``/debug/env/`` reports *facts* about the database configuration the app
  sees (never the values themselves): under blastit it holds no password and
  points at a local socket.
"""

import os
from urllib.parse import unquote, urlsplit

from django.contrib.auth import authenticate, login, logout
from django.contrib.auth.decorators import login_required
from django.contrib.auth.models import User
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
