"""One request through the package's own gate (`blastit check`, WSGI leg).

`blastit check` runs pytest with the *installed* package's `python/blastit` on
`PYTHONPATH` (`MB.42.11`) and `blastit._check_pytest` recording what the view saw.
Nothing here imports Blastit from a checkout: the `from blastit import _check_pytest`
below is the shipped tree, which is what the container leg is about.
"""

import io
import sys
from pathlib import Path

from blastit import _check_pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from hello import application  # noqa: E402  (the fixture's app, one directory up)

gated = _check_pytest.wrap(application)


def test_the_packaged_gate_observes_one_request() -> None:
    environ = {
        "REQUEST_METHOD": "GET",
        "PATH_INFO": "/",
        "QUERY_STRING": "",
        "CONTENT_TYPE": "",
        "CONTENT_LENGTH": "0",
        "SERVER_NAME": "localhost",
        "SERVER_PORT": "80",
        "SERVER_PROTOCOL": "HTTP/1.1",
        "wsgi.version": (1, 0),
        "wsgi.url_scheme": "http",
        "wsgi.input": io.BytesIO(b""),
        "wsgi.errors": sys.stderr,
        "wsgi.multithread": False,
        "wsgi.multiprocess": False,
        "wsgi.run_once": False,
    }
    body = b"".join(gated(environ, lambda status, headers: None))
    assert b"hello from the packaged shim" in body
