"""The WSGI callable blastit serves (`WSGI_APPLICATION` names it).

A module of its own, as `django-admin startproject` writes one: blastit starts
the project's application, not the test suite's thread, which is what makes the
requests real.
"""

import os

from django.core.wsgi import get_wsgi_application

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "settings")

application = get_wsgi_application()
