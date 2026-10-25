"""End-to-end scenario against the demo app. Stdlib only.

    scenario.py BASE_URL --expect blastit     # served by blastit: the sandbox must hold
    scenario.py BASE_URL --expect baseline    # served by gunicorn: same app, no sandbox

The user flow (log in, add and delete notes, log out) must work the same way
in both modes. The sandbox checks differ: under blastit the notes route can't
read password hashes and the app sees no database password.
"""

import argparse
import http.cookiejar
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

FAILURES = []


def check(cond, what):
    print(f"  {'ok  ' if cond else 'FAIL'} {what}", flush=True)
    if not cond:
        FAILURES.append(what)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


class Client:
    def __init__(self, base):
        self.base = base.rstrip("/")
        self.jar = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar), NoRedirect)

    def request(self, method, path, data=None):
        body = urllib.parse.urlencode(data).encode() if data is not None else None
        req = urllib.request.Request(self.base + path, data=body, method=method)
        req.add_header("Referer", self.base + path)
        if body is not None:
            req.add_header("Content-Type", "application/x-www-form-urlencoded")
        try:
            with self.opener.open(req, timeout=30) as r:
                return r.status, dict(r.headers), r.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            return e.code, dict(e.headers), e.read().decode("utf-8", "replace")

    def csrf(self):
        for c in self.jar:
            if c.name == "csrftoken":
                return c.value
        return ""

    def post(self, path, data=None):
        return self.request("POST", path, {"csrfmiddlewaretoken": self.csrf(), **(data or {})})


def location(headers):
    """The path of a redirect: the platform's edge may make Location absolute."""
    return urllib.parse.urlsplit(headers.get("Location", "")).path


def wait_ready(base, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(base.rstrip("/") + "/", timeout=5) as r:
                if r.status == 200:
                    return
        except Exception:  # noqa: BLE001
            pass
        time.sleep(1)
    sys.exit(f"{base} did not answer within {timeout}s")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("base")
    ap.add_argument("--expect", choices=["blastit", "baseline"], required=True)
    ap.add_argument("--password", default="demo-password")
    ap.add_argument("--wait", type=int, default=120)
    args = ap.parse_args()
    under_blastit = args.expect == "blastit"
    wait_ready(args.base, args.wait)

    alice = Client(args.base)
    print("anonymous")
    status, _, _ = alice.request("GET", "/")
    check(status == 200, "home page is served")
    status, headers, _ = alice.request("GET", "/notes/")
    check(status == 302 and location(headers) == "/login/", "notes redirect to the login page")

    print("log in")
    status, _, body = alice.request("GET", "/login/")
    check(status == 200 and "csrfmiddlewaretoken" in body, "login form has a CSRF token")
    status, _, body = alice.post("/login/", {"username": "alice", "password": "wrong"})
    check(status == 200 and "Invalid username or password" in body, "a wrong password is refused")
    status, headers, _ = alice.post("/login/", {"username": "alice", "password": args.password})
    check(status == 302 and location(headers) == "/notes/", "the right password logs in")

    print("notes")
    marker = f"note-{time.time_ns()}"
    status, _, _ = alice.post("/notes/", {"text": marker})
    check(status == 302, "adding a note redirects")
    status, _, body = alice.request("GET", "/notes/")
    check(status == 200 and marker in body, "the new note is listed")
    ids = re.findall(r'data-id="(\d+)">' + re.escape(marker), body)
    check(len(ids) == 1, "the note has an id")

    bob = Client(args.base)
    bob.request("GET", "/login/")
    bob.post("/login/", {"username": "bob", "password": args.password})
    status, _, body = bob.request("GET", "/notes/")
    check(status == 200 and marker not in body, "bob doesn't see alice's note")
    if ids:
        status, _, _ = bob.post(f"/notes/{ids[0]}/delete/")
        check(status == 404, "bob can't delete alice's note")
        status, _, _ = alice.post(f"/notes/{ids[0]}/delete/")
        check(status == 302, "alice deletes her note")
        _, _, body = alice.request("GET", "/notes/")
        check(marker not in body, "the note is gone")

    print("sandbox")
    status, _, body = alice.request("GET", "/notes/peek/")
    if under_blastit:
        check(status == 500, "the notes route can't read password hashes (statement not granted)")
    else:
        check(status == 200 and '"users_read"' in body, "baseline: the notes route reads password hashes")
    status, _, body = alice.request("GET", "/debug/env/")
    facts = json.loads(body) if status == 200 else {}
    check(facts.get("database_url_set") is True, "the app has a DATABASE_URL")
    if under_blastit:
        check(facts.get("has_password") is False, "the app's DATABASE_URL holds no password")
        check(facts.get("host_is_local_socket") is True, "the app's DATABASE_URL points at a local socket")
        check(facts.get("ca_cert_visible") is False, "DATABASE_CA_CERT is hidden from the app")
        check(facts.get("blastit_vars_visible") == [], "no BLASTIT_* variable reaches the app")
        check(facts.get("route") == "debug", "blastit.route names the route")
        status, _, _ = alice.request("GET", "/_blastit/health")
        check(status == 200, "the health path is answered")
        status, _, _ = alice.request("GET", "/notes/../../etc/passwd")
        check(status in (400, 404), "path traversal-looking paths don't reach a privileged route")
    else:
        check(facts.get("has_password") is True, "baseline: the app holds the database password")

    print("log out")
    status, _, _ = alice.post("/logout/")
    check(status == 302, "logging out redirects")
    status, headers, _ = alice.request("GET", "/notes/")
    check(status == 302 and location(headers) == "/login/", "notes need a login again")

    print(f"\n{'PASS' if not FAILURES else 'FAIL'}: {len(FAILURES)} failure(s)")
    for f in FAILURES:
        print(f"  - {f}")
    return 1 if FAILURES else 0


if __name__ == "__main__":
    sys.exit(main())
