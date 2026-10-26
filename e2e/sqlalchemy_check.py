"""The Flask + SQLAlchemy app, driven over HTTP. Stdlib only.

    sqlalchemy_check.py BASE_URL [LOG]

The app is the second half of the end-to-end test: the Django demo proves the
sandbox, and this proves that a *non-Django* framework, with its own URL
parsing and its own connection statements, is served by the same manifest
design (E2b):

* `POST /notes/add/` writes a row and `GET /notes/` reads it back — two routes,
  two grants, and the row survives between them because both go to the same
  table through their own route;
* the read route reports the `DATABASE_URL` the app was handed: it must be the
  one blastit renders — no password, and the socket directory as the `host`
  query parameter that SQLAlchemy (which does not percent-decode a host) and
  psycopg both read;
* with `LOG`, blastit's event stream is checked too: every request was served
  (`outcome: ok`, `200`) and nothing was denied (`sql.denied` would mean the
  manifest's `connection` group or a route's grant is missing a statement the
  app really runs).
"""

import json
import sys
import urllib.error
import urllib.parse
import urllib.request

FAILURES = []


def check(cond, what):
    print(f"  {'ok  ' if cond else 'FAIL'} {what}", flush=True)
    if not cond:
        FAILURES.append(what)


def request(method, url, timeout=30):
    """One request, answered in full, with an error response as a value."""
    req = urllib.request.Request(url, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:  # noqa: S310 - 127.0.0.1
            return response.status, response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")


def check_the_app(base):
    """The two routes, and the URL the app is pointed at."""
    status, body = request("POST", f"{base}/notes/add/")
    check(status == 200, f"the write route answers ({status})")
    document = json.loads(body) if status == 200 else {}
    check(document.get("added") is True, f"and reports the insert ({body[:200]})")

    status, body = request("GET", f"{base}/notes/")
    check(status == 200, f"the read route answers ({status})")
    document = json.loads(body) if status == 200 else {}
    notes = document.get("notes", [])
    check(
        [note.get("body") for note in notes] == ["sqlalchemy"],
        f"the row the write route inserted is the one the read route reads ({notes})",
    )

    # What SQLAlchemy was handed: no password, an empty authority, and the socket
    # directory as the `host` parameter — the shape it hands to psycopg as a
    # keyword argument instead of a host to resolve.
    url = document.get("database_url") or ""
    parts = urllib.parse.urlsplit(url)
    query = dict(urllib.parse.parse_qsl(parts.query))
    check(
        parts.scheme == "postgresql"
        and parts.username == "blastit"
        and parts.password is None
        and parts.path.lstrip("/") != "",
        f"the app was handed blastit's URL, the user and no password ({url!r})",
    )
    check(
        not parts.hostname,
        f"whose authority carries no host to percent-decode ({url!r})",
    )
    check(
        query.get("host") == "/proc/self/cwd",
        f"and whose socket directory is the `host` parameter ({url!r})",
    )
    check(
        (query.get("port") or "").isdigit(),
        f"with the proxy's port next to it ({url!r})",
    )

    status, body = request("POST", f"{base}/notes/add/")
    document = json.loads(body) if status == 200 else {}
    check(document.get("count") == 2, f"a second insert sees both rows ({document})")

    # The route that is not granted its `DELETE`: the statement reaches the proxy
    # and is refused there, so the request fails and the rows are still there.
    status, _ = request("POST", f"{base}/notes/clear/")
    check(status == 500, f"a statement no route grants is refused ({status})")
    status, body = request("GET", f"{base}/notes/")
    document = json.loads(body) if status == 200 else {}
    check(
        len(document.get("notes", [])) == 2,
        f"and nothing was deleted ({document.get('notes')})",
    )


def check_the_log(text):
    """blastit's own stream: the requests, and the one statement that was refused."""
    events = []
    for line in text.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            events.append(json.loads(line))
        except ValueError:
            continue
    requests = [event for event in events if event.get("event") == "request"]
    check(
        len(requests) == 5,
        f"every request is in the log ({len(requests)} of 5)",
    )
    check(
        [event.get("status") for event in requests].count(200) == 4,
        f"four of them answered 200 ({[event.get('status') for event in requests]})",
    )
    check(
        sorted({event.get("route") for event in requests})
        == ["notes-clear", "notes-read", "notes-write"],
        f"and each went to its own route ({sorted({event.get('route') for event in requests})})",
    )
    denied = [event for event in events if event.get("event") == "sql.denied"]
    check(
        [event.get("sql") for event in denied] == ["DELETE FROM sqlalchemy_notes"],
        f"exactly the statement no route grants was denied ({denied})",
    )
    check(
        all(event.get("sqlstate") == "42501" for event in denied),
        "refused by the proxy (SQLSTATE 42501), not by the database's own grants",
    )


def main():
    """`sqlalchemy_check.py BASE_URL` drives the app; `--log LOG` reads blastit's stream.

    Two passes, because the log only holds the requests once they have been made:
    `run.py` drives the app first, then collects the log and checks it.
    """
    if len(sys.argv) == 3 and sys.argv[1] == "--log":
        print("blastit's log")
        with open(sys.argv[2], encoding="utf-8") as log:
            check_the_log(log.read())
    elif len(sys.argv) == 2:
        print("the app")
        check_the_app(sys.argv[1].rstrip("/"))
    else:
        raise SystemExit("usage: sqlalchemy_check.py [--log LOG | BASE_URL]")
    for failure in FAILURES:
        print(f"FAILED: {failure}")
    print(f"\n{'PASS' if not FAILURES else 'FAIL'}: {len(FAILURES)} failure(s)")
    return 1 if FAILURES else 0


if __name__ == "__main__":
    sys.exit(main())
