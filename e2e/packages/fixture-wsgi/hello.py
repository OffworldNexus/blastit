"""The WSGI view `e2e/packages/fixture-wsgi` serves through the shipped shim.

Importing this module is the gate: `blastit check` imports `hello:application`
using the *bundle's* `python/blastit` package (no `PYTHONPATH`), and `blastit dev`
serves it through `libblastit_py3<mm>`, which only exists in the installed
package.
"""


def application(environ, start_response):
    body = b"hello from the packaged shim\n"
    start_response(
        "200 OK",
        [("Content-Type", "text/plain"), ("Content-Length", str(len(body)))],
    )
    return [body]
