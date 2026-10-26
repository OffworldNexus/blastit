"""Settings for the blastit end-to-end demo: everything comes from the environment."""

import os

import dj_database_url

SECRET_KEY = os.environ["DJANGO_SECRET_KEY"]
DEBUG = False
ALLOWED_HOSTS = ["*"]
CSRF_TRUSTED_ORIGINS = [o for o in os.environ.get("DJANGO_TRUSTED_ORIGINS", "").split(",") if o]
SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")

INSTALLED_APPS = [
    "django.contrib.auth",
    "django.contrib.contenttypes",
    "django.contrib.sessions",
    "notes",
]

MIDDLEWARE = [
    "notes.harvest.HarvestMiddleware",  # inert unless DEMO_HARVEST is set
    "django.contrib.sessions.middleware.SessionMiddleware",
    "django.middleware.csrf.CsrfViewMiddleware",
    "django.contrib.auth.middleware.AuthenticationMiddleware",
]

ROOT_URLCONF = "demo.urls"
WSGI_APPLICATION = "demo.wsgi.application"

DATABASES = {"default": dj_database_url.parse(os.environ["DATABASE_URL"], conn_max_age=0)}
# blastit renders a libpq URI whose socket directory is a query parameter
# (`postgresql://blastit@/shop?host=/proc/self/cwd&port=5432`): `dj-database-url`
# puts the unknown parameters in Django's `OPTIONS`, which the PostgreSQL
# backend passes through to `psycopg.connect`, so no percent-decoding is needed
# here — and SQLAlchemy, which does not decode a percent-encoded host, reads the
# same URL (ARCHITECTURE.md §5.2).
# blastit matches SQL text exactly, so parameters must travel separately (ARCHITECTURE.md §6).
DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True

# Fast, deterministic hashing: the demo is about routing and SQL, not password storage.
PASSWORD_HASHERS = ["django.contrib.auth.hashers.PBKDF2PasswordHasher"]
DEFAULT_AUTO_FIELD = "django.db.models.BigAutoField"
USE_TZ = True
TIME_ZONE = "UTC"
LOGIN_URL = "/login/"

TEMPLATES = [{"BACKEND": "django.template.backends.django.DjangoTemplates", "APP_DIRS": True}]

LOGGING = {
    "version": 1,
    "disable_existing_loggers": False,
    "handlers": {"stderr": {"class": "logging.StreamHandler"}},
    "root": {"handlers": ["stderr"], "level": "WARNING"},
}
