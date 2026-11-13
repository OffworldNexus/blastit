import pgwire


def application(environ, start_response):
    pgwire.simple(["SELECT 1"])
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"ok\n"]
