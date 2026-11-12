import io

from app import application
from blastit import _check_pytest as gate


def request(method="GET", path="/", body=b"", content_type=None):
    environ = {
        "REQUEST_METHOD": method,
        "PATH_INFO": path,
        "CONTENT_LENGTH": str(len(body)),
        "wsgi.input": io.BytesIO(body),
    }
    if content_type:
        environ["CONTENT_TYPE"] = content_type
    status = {}
    payload = b"".join(
        gate.wrap(application)(
            environ,
            lambda code, headers: status.update(code=code),
        )
    )
    return status.get("code"), payload


def test_catalog():
    assert request(path="/catalog")[0] == "200 OK"


def test_search():
    assert request(path="/search")[0] == "200 OK"


def test_upload():
    assert (
        request(
            "POST", "/upload", b"name=blastit", "application/x-www-form-urlencoded"
        )[0]
        == "200 OK"
    )


def test_robots():
    # `check` resolves the path against the routing table after the suite; the
    # gate calls the app either way, so the status here is the app's own.
    assert request(path="/robots.txt")[0] == "200 OK"
