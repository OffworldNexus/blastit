import pgwire


def application(environ, start_response):
    try:
        pgwire.extended("SELECT 1")
    except RuntimeError:
        start_response("500 Internal Server Error", [("Content-Length", "0")])
        return [b""]
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"ok\n"]
