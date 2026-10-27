"""The Django settings the live-server tests are served with (D3c).

Small on purpose: one URLconf, one PostgreSQL database from `DATABASE_URL`, and
the two things a live server needs that a plain test project does not:

* `WSGI_APPLICATION`, so `blastit_live_server` knows which callable to hand to
  `BLASTIT_APP` (blastit starts the *project*, not the test's thread);
* a `DATABASE_URL` reader that understands the URL blastit renders for the app:
  `postgresql://blastit@/db?host=/proc/self/cwd&port=5432`, whose *socket
  directory* — the one the view runs in — travels as a query parameter (§5.2).
  libpq reads that as a Unix socket; a URL a project parses itself has to look
  in the query when there is no authority, and a live server that connects to
  the wrong socket is a live server that misbehaves in a way that looks like a
  manifest problem.
"""

import os
from urllib.parse import parse_qsl, unquote, urlsplit

SECRET_KEY = "not-a-secret-this-is-a-test"
DEBUG = False
ALLOWED_HOSTS = ["*"]
ROOT_URLCONF = "urls"
# What blastit_live_server serves: the project's own WSGI callable.
WSGI_APPLICATION = "wsgi:application"
USE_TZ = True
DEFAULT_AUTO_FIELD = "django.db.models.AutoField"
# The live server's static handler reads `STATIC_URL`; Django's global default
# is `None`.
STATIC_URL = "/static/"

_url = urlsplit(os.environ.get("DATABASE_URL", ""))
# blastit's socket directory is a query parameter
# (`?host=/proc/self/cwd&port=5432`); a URL this process is handed by the test
# runner names a host and a port the ordinary way. Both are read here.
_query = dict(parse_qsl(_url.query))
DATABASES = {
    "default": {
        "ENGINE": "django.db.backends.postgresql",
        "NAME": unquote(_url.path).lstrip("/"),
        "USER": unquote(_url.username or ""),
        "PASSWORD": unquote(_url.password or ""),
        # The socket directory the view runs in, under blastit (§5.2); the
        # suite's own URL names a host and port, which works out to the same
        # thing here.
        "HOST": unquote(_url.hostname) if _url.hostname else unquote(_query.get("host", "")),
        "PORT": str(_url.port or _query.get("port") or 5432),
        # §6: without this, psycopg inlines the literals and no grant matches.
        "OPTIONS": {"server_side_binding": True},
        # §6: request threads are thrown away after every request.
        "CONN_MAX_AGE": 0,
    }
}
