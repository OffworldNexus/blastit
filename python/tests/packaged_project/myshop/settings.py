"""The Django settings of a project laid out the way `startproject` writes one.

`django-admin startproject myshop .` puts the settings *inside the package* it
creates (`myshop/settings.py`) and names it by its dotted path
(`DJANGO_SETTINGS_MODULE = "myshop.settings"`), which makes the directory that
holds `myshop/` the only one an import of it can start from. That is the layout
this fixture exists for: `live_project/` is flat, because that is what the plugin
was written against first, and both have to work.

Everything else is `live_project/settings.py`: one URLconf, one PostgreSQL
database from `DATABASE_URL`, and the two things a live server needs that a plain
test project does not — `WSGI_APPLICATION` in the *dotted* spelling Django
documents, and a `DATABASE_URL` reader that understands the URL blastit renders
for the app (§5.2).
"""

import os
from urllib.parse import unquote, urlsplit

SECRET_KEY = "not-a-secret-this-is-a-test"
DEBUG = False
ALLOWED_HOSTS = ["*"]
ROOT_URLCONF = "myshop.urls"
# What Django writes, and what `blastit_live_server` has to convert to the
# `module:attribute` spelling `BLASTIT_APP` takes: a dotted path, not a colon.
WSGI_APPLICATION = "myshop.wsgi.application"
USE_TZ = True
DEFAULT_AUTO_FIELD = "django.db.models.AutoField"
# The live server's static handler reads `STATIC_URL`; Django's global default
# is `None`.
STATIC_URL = "/static/"

_url = urlsplit(os.environ.get("DATABASE_URL", ""))
DATABASES = {
    "default": {
        "ENGINE": "django.db.backends.postgresql",
        "NAME": unquote(_url.path).lstrip("/"),
        "USER": unquote(_url.username or ""),
        "PASSWORD": unquote(_url.password or ""),
        # The socket directory the view runs in, under blastit (§5.2).
        "HOST": unquote(_url.hostname or ""),
        "PORT": str(_url.port or 5432),
        # §6: without this, psycopg inlines the literals and no grant matches.
        "OPTIONS": {"server_side_binding": True},
        # §6: request threads are thrown away after every request.
        "CONN_MAX_AGE": 0,
    }
}
