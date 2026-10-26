"""The app the venv smoke test serves: it reports which interpreter runs it.

Four things have to hold for `pip install blastit` to be usable, and one
response makes all four visible:

* `sys.prefix` is the virtualenv, so the interpreter really is the venv's;
* `venv_marker` — a module that exists in the venv's `site-packages` and nowhere
  else — imports, so the app's *dependencies* are importable;
* this module itself imports, which it does only because stage 1 put the working
  directory blastit was started in on `sys.path` (§5.3): the project is not
  installed and the environment carries no `PYTHONPATH`;
* and it is *first* on `sys.path`, which is `path_first` below.
"""

import json
import os
import sys

import venv_marker


def application(environ, start_response):
    body = json.dumps(
        {
            "prefix": sys.prefix,
            "executable": sys.executable,
            "marker": venv_marker.VALUE,
            "path": environ["PATH_INFO"],
            "route": environ.get("blastit.route"),
            "path_first": sys.path[0] if sys.path else None,
            "cwd": os.getcwd(),
        }
    ).encode()
    start_response(
        "200 OK",
        [
            ("Content-Type", "application/json"),
            ("Content-Length", str(len(body))),
        ],
    )
    return [body]
