"""The app the venv smoke test serves: it reports which environment runs it.

Four things have to hold for `pip install blastit` to be usable, and one
response makes them all visible:

* `venv_marker` — a module that exists in the venv's `site-packages` and nowhere
  else — imports, so the app's *dependencies* are importable, and `library` names
  the directory it came from, so that directory is the **venv's**;
* this module itself imports, which it does only because stage 1 put the working
  directory blastit was started in on `sys.path` (§5.3): the project is not
  installed and the environment carries no `PYTHONPATH`;
* and it is *first* on `sys.path`, which is `path_first` below.

`prefix` and `executable` are reported for the record and not asserted: the
CPython shim initialises libpython with the venv's recorded `home`, so
`sys.prefix` is the *base* installation the venv was built from and the venv's
packages arrive on `sys.path` (§2.3, `critical_python_shim`). The interpreter is
the venv's — `check.py` reads the console script's shebang and the boot's own
`environment` field for that.
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
            "library": os.path.dirname(venv_marker.__file__),
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
