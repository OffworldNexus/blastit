import pgwire


def application(environ, start_response):
    if environ.get("PATH_INFO") in ("", "/", "/products/"):
        pgwire.simple(["SELECT 1"])
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"ok"]


def products(environ, start_response):
    return application(environ, start_response)


# The page in docs/local.md names `app = "app:app"`; the same callable answers.
app = application
