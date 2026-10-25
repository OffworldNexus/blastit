"""The app the venv smoke test serves: it reports which interpreter runs it.

Three things have to hold for `pip install blastit` to be usable, and one
response makes all three visible:

* `sys.prefix` is the virtualenv, so the interpreter really is the venv's;
* `venv_marker` — a module that exists in the venv's `site-packages` and nowhere
  else — imports, so the app's *dependencies* are importable;
* this module itself imports, which it does only because stage 1 restored the
  `sys.path` the console script had (`PYTHONPATH` here), across the `execve`
  barrier of §4.
"""

import json
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
