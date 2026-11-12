import pgwire


def application(environ, start_response):
    path = environ.get("PATH_INFO", "")
    if path == "/catalog":
        pgwire.simple(["SELECT 1", "SELECT 2"])
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"ok\n"]
