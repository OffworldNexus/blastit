"""The WSGI callable of the packaged project (`WSGI_APPLICATION` names it).

`django-admin startproject` writes this module inside the package and points
`WSGI_APPLICATION` at it by its dotted path; blastit starts the *project*, not the
test suite's thread, which is what makes the requests the tests make real ones.
"""

import os

from django.core.wsgi import get_wsgi_application

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "myshop.settings")

application = get_wsgi_application()
