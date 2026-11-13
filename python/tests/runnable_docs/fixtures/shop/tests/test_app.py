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


def test_products():
    code, payload = request(path="/products/")
    assert code == "200 OK"
    assert payload == b"ok"
