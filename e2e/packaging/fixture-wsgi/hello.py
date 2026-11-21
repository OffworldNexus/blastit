"""The WSGI view `e2e/packaging/fixture-wsgi` serves through the shipped shim.

Importing this module is the gate: `blastit check` imports `hello:application`
using the *installed* package's `python/blastit` (no `PYTHONPATH`), and
`blastit dev` serves it through `libblastit_py3<mm>`, which only exists in an
installed package or wheel.
"""


def application(environ, start_response):
    body = b"hello from the packaged shim\n"
    start_response(
        "200 OK",
        [("Content-Type", "text/plain"), ("Content-Length", str(len(body)))],
    )
    return [body]
