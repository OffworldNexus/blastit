"""End-to-end scenario against the demo app. Stdlib only.

    scenario.py BASE_URL --expect blastit     # served by blastit: the sandbox must hold
    scenario.py BASE_URL --expect baseline    # served by gunicorn: same app, no sandbox

The user flow (log in, add and delete notes, log out) must work the same way
in both modes. The sandbox checks differ: under blastit the notes route can't
read password hashes and the app sees no database password.

Under blastit the client's own address is checked too (E1): `--remote-addr` says
what the deployment's ingress sees for this client — an IP to assert exactly, or
`public` where the driver cannot know it (a hosted deployment) and the address
only has to be a client's rather than the ingress's. Whatever the answer is, a
client-IP header the *client* sent never becomes `REMOTE_ADDR`.
"""

import argparse
import errno
import http.cookiejar
import ipaddress
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


def address_of(remote_addr):
    """The IP of a `REMOTE_ADDR`: a socket peer carries `ip:port`."""
    text = (remote_addr or "").strip()
    if text.startswith("[") and "]" in text:
        return text[1 : text.index("]")]
    if text.count(":") == 1:
        return text.split(":")[0]
    return text


def is_client_address(remote_addr):
    """Whether a `REMOTE_ADDR` could be an internet client's own address.

    What it must not be is the ingress's: blastit reports the socket peer when
    nothing in the request may be believed, and an ingress address is a private
    or carrier-NAT one (§10: `10.244.x.x`, `100.127.x`). Loopback is not a
    client a hosted deployment can have either.
    """
    try:
        address = ipaddress.ip_address(address_of(remote_addr))
    except ValueError:
        return False
    return not (
        address.is_loopback
        or address.is_private
        or address.is_link_local
        or address in ipaddress.ip_network("100.64.0.0/10")
    )


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


class Client:
    def __init__(self, base):
        self.base = base.rstrip("/")
        self.jar = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar), NoRedirect)

    def request(self, method, path, data=None, headers=None):
        body = urllib.parse.urlencode(data).encode() if data is not None else None
        req = urllib.request.Request(self.base + path, data=body, method=method)
        req.add_header("Referer", self.base + path)
        for name, value in (headers or {}).items():
            req.add_header(name, value)
        if body is not None:
            req.add_header("Content-Type", "application/x-www-form-urlencoded")
        try:
            with self.opener.open(req, timeout=30) as r:
                return r.status, r.headers, r.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            return e.code, e.headers, e.read().decode("utf-8", "replace")

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
    ap.add_argument(
        "--remote-addr",
        help="the client address blastit must report: an IP (asserted exactly, as the "
        "ingress sees this client), or `public` (any client address rather than the "
        "ingress's, for a deployment whose client the driver cannot know)",
    )
    ap.add_argument(
        "--expect-scheme",
        choices=["http", "https"],
        help="the scheme the deployment must report, where the driver knows whether the "
        "ingress terminated TLS",
    )
    ap.add_argument(
        "--forged-proto-ignored",
        action="store_true",
        help="the deployment cannot be lied to about the scheme: the ingress overwrites "
        "X-Forwarded-Proto itself (measured on Traefik v3's default configuration) or "
        "the peer is not trusted to say it. Not for a deployment whose edge forwards a "
        "client's X-Forwarded-Proto untouched — there the client *is* who the scheme "
        "says",
    )
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

    print("client address")
    # E1: what the deployment believes about the client. `REMOTE_ADDR` is
    # blastit's resolution — the socket peer, or a trusted client-IP header's
    # entry — and `wsgi.url_scheme` is `X-Forwarded-Proto` from a peer blastit
    # trusts, so a client that sends either header itself changes neither.
    status, _, body = alice.request("GET", "/debug/request/")
    facts = json.loads(body) if status == 200 else {}
    check(status == 200, "the debug route answers the request probe")
    honest_addr = facts.get("remote_addr")
    honest_scheme = facts.get("scheme")
    check(bool(honest_addr), f"the app is told who the client is ({honest_addr})")
    check(honest_scheme in ("http", "https"), f"and which scheme it spoke ({honest_scheme})")
    if args.remote_addr and args.remote_addr != "public":
        check(
            honest_addr == args.remote_addr,
            f"REMOTE_ADDR is the client the ingress saw "
            f"({honest_addr}, expected {args.remote_addr})",
        )
    elif under_blastit and args.remote_addr == "public":
        check(
            is_client_address(honest_addr),
            f"REMOTE_ADDR is a client's address, not the ingress's ({honest_addr})",
        )

    forged = Client(args.base)
    # The client sends every client-IP header a proxy might set, and every one of
    # them is a lie it cannot make blastit believe.
    headers = {
        "X-Forwarded-For": "6.6.6.6",
        "X-Real-Ip": "6.6.6.6",
        "Forwarded": "for=6.6.6.6;proto=https",
        "X-Client-IP": "6.6.6.6",
        "True-Client-IP": "6.6.6.6",
        "CF-Connecting-IP": "6.6.6.6",
        "do-connecting-ip": "6.6.6.6",
    }
    if args.forged_proto_ignored:
        headers["X-Forwarded-Proto"] = "https"
    status, _, body = forged.request("GET", "/debug/request/", headers=headers)
    facts = json.loads(body) if status == 200 else {}
    forged_addr = facts.get("remote_addr")
    check(
        forged_addr != "6.6.6.6",
        f"a client-IP header the client sent is never REMOTE_ADDR ({forged_addr})",
    )
    # The address, not the exact string: a peer is reported as `ip:port`, and
    # this request is a connection of its own.
    check(
        address_of(forged_addr) == address_of(honest_addr),
        f"and the client the deployment sees is unchanged ({forged_addr})",
    )
    if args.forged_proto_ignored:
        check(
            facts.get("scheme") == honest_scheme,
            f"as is the scheme: X-Forwarded-Proto from an untrusted peer is not read "
            f"({facts.get('scheme')})",
        )
    if args.expect_scheme:
        check(
            honest_scheme == args.expect_scheme,
            f"wsgi.url_scheme is the ingress's own, not the client's "
            f"({honest_scheme}, expected {args.expect_scheme})",
        )

    print("egress and system calls")
    # A fresh client: the egress route has no grants at all, so the probe must
    # not load a session or open a database connection either.
    probe = Client(args.base)
    status, _, body = probe.request("GET", "/debug/egress/")
    facts = json.loads(body) if status == 200 else {}
    check(status == 200, "the debug route answers the egress probe")
    if under_blastit:
        check(facts.get("route") == "debug", "the probe ran in the debug route's process")
        check(facts.get("inet") == "refused", "a route without egress cannot create an inet socket")
        check(facts.get("inet_errno") == 1, "the refusal is EPERM")
        check(facts.get("ptrace") == "refused", "ptrace is refused in a view")
    else:
        check(facts.get("inet") == "allowed", "baseline: no filter, an inet socket is created")

    print("capabilities")
    # D6: whatever mode init runs in — root on App Platform, a few capabilities
    # on a plain container — the *view* process that answers this has none, and
    # has `no_new_privs` set, so an execve of a file with file capabilities
    # cannot give it one back.
    #
    # The verdict has to be the kernel's, and `/proc/self/status` is not always
    # the kernel's: under gVisor (App Platform) the file has no `CapAmb` and no
    # `NoNewPrivs` line at all, so the probe asks through `prctl(2)` and the
    # `/proc` fields are reported — and checked — only where the platform prints
    # them.
    status, _, body = probe.request("GET", "/debug/caps/")
    facts = json.loads(body) if status == 200 else {}
    check(status == 200, "the debug route answers the capability probe")
    if under_blastit:
        check(facts.get("route") == "debug", "the probe ran in the debug route's process")
        for field in ("inheritable", "permitted", "effective", "bounding"):
            check(
                facts.get(field) == "0000000000000000",
                f"a view holds no {field} capability ({facts.get(field)})",
            )
        kernel = facts.get("prctl") or {}
        check(
            kernel.get("no_new_privs") == 1,
            f"the kernel says no_new_privs is set (prctl gave {kernel.get('no_new_privs')})",
        )
        # `PR_CAP_AMBIENT_IS_SET` is asked once per capability number. `EINVAL`
        # is the answer of a kernel that has no such sub-command at all (before
        # 4.3, gVisor before it implemented ambient capabilities): it cannot
        # have one set, which is what this check is after. Any other errno is
        # not an answer, and the check says so.
        ambient = kernel.get("ambient")
        check(
            ambient == [] and kernel.get("ambient_errno") in (None, errno.EINVAL),
            f"the kernel says no ambient capability is set (prctl gave {ambient}, "
            f"errno {kernel.get('ambient_errno')})",
        )
        print(f"  info prctl: {json.dumps(kernel, sort_keys=True)}")
        print(
            "  info /proc/self/status:"
            f" CapInh={facts.get('inheritable')} CapPrm={facts.get('permitted')}"
            f" CapEff={facts.get('effective')} CapBnd={facts.get('bounding')}"
            f" CapAmb={facts.get('ambient')} NoNewPrivs={facts.get('no_new_privs')}"
            f" ({facts.get('proc_error')})"
        )
        # The two fields gVisor omits, when the platform *does* print them: they
        # have to agree with the kernel's own answer. `None` — the line is
        # absent — is not a failure, it is the reason the probe asks `prctl`.
        check(
            facts.get("ambient") in (None, "0000000000000000"),
            f"/proc agrees: no ambient capability ({facts.get('ambient')})",
        )
        check(
            facts.get("no_new_privs") in (None, "1"),
            f"/proc agrees: no_new_privs is set ({facts.get('no_new_privs')})",
        )
    else:
        # Outside blastit there is nothing that clears anything: the check is
        # that the probe itself ran.
        check(isinstance(facts.get("uid"), int), "baseline: the probe reads its own status")

    status, _, body = probe.request("GET", "/egress/probe/")
    facts = json.loads(body) if status == 200 else {}
    check(status == 200, "the egress route answers the egress probe")
    check(facts.get("inet") == "allowed", "a route with egress creates an inet socket")
    if under_blastit:
        check(facts.get("route") == "egress", "the probe ran in the egress route's process")
        check(facts.get("ptrace") == "refused", "ptrace is refused here too")
    # In baseline mode the second probe's `ptrace` is whatever the process it
    # landed on reported: a process may only `PTRACE_TRACEME` once, so a worker
    # that already ran the first probe refuses it. Only blastit's answer is
    # deterministic, and that is the one worth asserting.

    print("raw SQL, from two processes")
    # One note to count, so "unchanged" cannot pass on an empty table.
    alice.post("/notes/", {"text": "keep-me"})
    granted = "SELECT count(*) FROM notes_note"
    other = "SELECT count(*) FROM auth_user"
    destructive = "DROP TABLE notes_note"
    multi = "SELECT 1; " + destructive

    def raw(client, path, sql):
        status, _, body = client.request("GET", f"{path}?q=" + urllib.parse.quote(sql))
        try:
            return status, json.loads(body)
        except ValueError:
            return status, {}

    status, body = raw(alice, "/debug/sql/", granted)
    before = int(body.get("rows", [["0"]])[0][0]) if body.get("ok") else None
    check(status == 200 and body.get("ok") is True, "raw SQL runs where it was granted")
    check(before is not None and before >= 1, "the row to protect is really there")
    if under_blastit:
        check(body.get("route") == "sql", "and it ran in the sql route's own process")

    # The very same text, sent from the notes process: not granted there.
    status, body = raw(alice, "/notes/sql/", granted)
    if under_blastit:
        check(status == 200 and body.get("ok") is False and body.get("sqlstate") == "42501",
              "the same statement is refused on the notes route (SQLSTATE 42501)")
    else:
        check(status == 200 and body.get("ok") is True, "baseline: no allowlist, so it runs anywhere")

    status, body = raw(alice, "/debug/sql/", other)
    if under_blastit:
        check(status == 200 and body.get("ok") is False and body.get("sqlstate") == "42501",
              "the sql route can't read the users table")
    else:
        check(status == 200 and body.get("ok") is True, "baseline: the users table is readable from any route")

    if under_blastit:
        status, body = raw(alice, "/notes/sql/", destructive)
        check(status == 200 and body.get("ok") is False, "a destructive statement is refused")
        status, body = raw(alice, "/notes/sql/", multi)
        check(status == 200 and body.get("ok") is False, "two statements in one text are refused")
        status, body = raw(alice, "/debug/sql/", granted)
        after = int(body.get("rows", [["0"]])[0][0]) if body.get("ok") else None
        check(after == before, "nothing reached the table, the row count is unchanged")
        # And the routes still work: their own grants were never affected.
        status, _, body = alice.request("GET", "/notes/")
        check(status == 200, "the notes route still works")
        fresh = Client(args.base)
        fresh.request("GET", "/login/")
        status, _, _ = fresh.post("/login/", {"username": "alice", "password": args.password})
        check(status == 302, "the users table is intact: a new client still logs in")

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
