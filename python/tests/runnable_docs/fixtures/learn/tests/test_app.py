import io

from app import application
from blastit import _check_pytest as gate


def request(method="GET", path="/", body=b""):
    environ = {
        "REQUEST_METHOD": method,
        "PATH_INFO": path,
        "CONTENT_LENGTH": str(len(body)),
        "wsgi.input": io.BytesIO(body),
    }
    status = {}
    payload = b"".join(
        gate.wrap(application)(
            environ,
            lambda code, headers: status.update(code=code),
        )
    )
    return status.get("code"), payload


def test_home():
    assert request(path="/")[0] == "200 OK"


def test_extra():
    assert request(path="/extra")[0] == "200 OK"
