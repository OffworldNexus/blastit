"""The Django settings the plugin's tests run against.

Small on purpose: one app, one PostgreSQL database from `DATABASE_URL`, and
`server_side_binding = True` because that is what blastit needs to match a grant
at all (ARCHITECTURE.md §6, "What the app must do") — the plugin reports the SQL
as psycopg 3 sends it with that option on. Nothing else is installed, so the SQL
each request sends is the SQL the views write and nothing more.

`BLASTIT_TEST_ATOMIC=1` turns `ATOMIC_REQUESTS` on for the alias, for the test
that checks the plugin notices a transaction even when pytest-django's own
transaction swallows the `BEGIN`.
"""

import os
from pathlib import Path
from urllib.parse import unquote, urlsplit

BASE_DIR = Path(__file__).resolve().parent

SECRET_KEY = "not-a-secret-this-is-a-test"
DEBUG = False
ALLOWED_HOSTS = ["*"]
DEFAULT_AUTO_FIELD = "django.db.models.AutoField"

INSTALLED_APPS = ["notes"]

MIDDLEWARE: list[str] = []
ROOT_URLCONF = "urls"
USE_TZ = True
# The live server's static handler reads `STATIC_URL`; Django's global default is
# `None`, which its `_StaticFilesHandler` cannot turn into a path.
STATIC_URL = "/static/"

_url = urlsplit(os.environ.get("DATABASE_URL", ""))
DATABASES = {
    "default": {
        "ENGINE": "django.db.backends.postgresql",
        "NAME": unquote(_url.path).lstrip("/"),
        "USER": unquote(_url.username or ""),
        "PASSWORD": unquote(_url.password or ""),
        "HOST": _url.hostname or "",
        "PORT": str(_url.port or 5432),
        # §6: without this, psycopg inlines the literals and no allowlist could
        # ever match a statement.
        "OPTIONS": {"server_side_binding": True},
        # §6: request threads are thrown away after every request, so a
        # persistent connection would be orphaned.
        "CONN_MAX_AGE": 0,
        "ATOMIC_REQUESTS": bool(os.environ.get("BLASTIT_TEST_ATOMIC")),
    }
}
