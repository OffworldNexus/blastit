"""The Django project's settings: one PostgreSQL store from `DATABASE_URL`.

`dj_database_url.config(conn_max_age=0)` is the documented reader: the URL
blastit renders puts the socket directory in the `host` query parameter
(`postgresql://app@/shop?host=/proc/self/cwd&port=5432`), which dj-database-url
passes through Django's PostgreSQL backend as a connection option.
`server_side_binding` keeps every value out of the SQL text, so the grants match
the parameterised form the manifest declares.

`STATIC_ROOT`/`STATIC_URL` are the documented static pair: `manage.py
collectstatic --no-input` runs before `blastit dev`, and the mount serves
`staticfiles/` with the year-long cache for `/static/*`.
"""

from pathlib import Path

import dj_database_url

BASE_DIR = Path(__file__).resolve().parent.parent

SECRET_KEY = "e2e-not-secret"
DEBUG = False
ALLOWED_HOSTS = ["*"]

INSTALLED_APPS = ["django.contrib.staticfiles"]
MIDDLEWARE = []
ROOT_URLCONF = "shop.urls"
WSGI_APPLICATION = "shop.wsgi.application"

DATABASES = {"default": dj_database_url.config(conn_max_age=0)}
DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True

USE_TZ = True
TIME_ZONE = "UTC"
DEFAULT_AUTO_FIELD = "django.db.models.BigAutoField"

STATIC_ROOT = BASE_DIR / "staticfiles"
STATIC_URL = "/static/"
STATICFILES_DIRS = [BASE_DIR / "public" / "static"]

LOGGING = {
    "version": 1,
    "disable_existing_loggers": False,
    "handlers": {"stderr": {"class": "logging.StreamHandler"}},
    "root": {"handlers": ["stderr"], "level": "WARNING"},
}
