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
client-IP header the *client* sent never becomes `REMOTE_ADDR` — and where the
deployment's *edge* refuses to carry that header at all, so that no app could
ever be told it, that is a pass of its own (`EDGE_REFUSED_CLIENT_IP_HEADERS`).
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
    """The IP of a `REMOTE_ADDR`, whatever shape it arrives in.

    blastit reports the address alone (CGI), so a value is already its own
    address; the `ip:port` and `[ipv6]:port` shapes an older build reported — and
    that a driver may still hand in through `--remote-addr` — are unwrapped all
    the same, because a check that reads an address must not care which spelling
    of one it was given.
    """
    text = (remote_addr or "").strip()
    if text.startswith("[") and "]" in text:
        return text[1 : text.index("]")]
    if text.count(":") == 1:
        return text.split(":")[0]
    return text


def is_port(value):
    """Whether a `REMOTE_PORT` really is the port of the connection.

    CGI's `REMOTE_PORT` is a decimal string; anything else — a whole address, an
    empty value, a number out of range — is a deployment saying something it
    cannot mean.
    """
    return isinstance(value, str) and value.isdigit() and 0 < int(value) < 65536


# §10: the socket peer is DO's ingress, and the ingress address was measured as
# `10.244.x.x` on one cluster and `100.127.x` on another. Those are what a
# reported `REMOTE_ADDR` must not be. `100.64.0.0/10` as a whole is *not* here:
# it is also carrier-NAT client space, and a CGNAT client is a legitimate
# origin — blastit reports what `do-connecting-ip` carries, and that is the
# client.
INGRESS_NETWORKS = (
    ipaddress.ip_network("10.244.0.0/16"),
    ipaddress.ip_network("100.127.0.0/16"),
)


def is_client_address(remote_addr):
    """Whether a `REMOTE_ADDR` could be an internet client's own address.

    What it must not be is the ingress's: blastit reports the socket peer when
    nothing in the request may be believed, and the ingress is the only peer a
    hosted deployment has (§10, [`INGRESS_NETWORKS`]). Loopback is not a client
    such a deployment can have either.

    Everything else is a client, including addresses a `is_private`-style test
    would call private: a deployment behind a corporate network or a carrier NAT
    reports the address the edge saw, and that address is a real client's. This
    is a check on *who the deployment says the client is*, not on whether the
    client is reachable from the internet.
    """
    try:
        address = ipaddress.ip_address(address_of(remote_addr))
    except ValueError:
        return False
    return not (address.is_loopback or any(address in net for net in INGRESS_NETWORKS))


# E1: the lie every forged header tells, and the client-IP headers that carry
# it. A client sends all of them; none of them may become `REMOTE_ADDR`.
FORGED_ADDRESS = "6.6.6.6"
FORGED_CLIENT_IP_HEADERS = {
    "X-Forwarded-For": FORGED_ADDRESS,
    "X-Real-Ip": FORGED_ADDRESS,
    "Forwarded": f"for={FORGED_ADDRESS};proto=https",
    "X-Client-IP": FORGED_ADDRESS,
    "True-Client-IP": FORGED_ADDRESS,
    "do-connecting-ip": FORGED_ADDRESS,
}

# The one header a client cannot even send on App Platform, measured there on
# 2026-10-27 (`blastit-forge-probe` in ams3, and every other App Platform app
# tried): the edge is Cloudflare, and it refuses a request that carries a
# client-sent `CF-Connecting-IP` with `403`, a body of `error code: 1000` and
# `Server: cloudflare` — the origin never sees the request. Ten requests with it
# alone and ten with the whole set were refused 10/10, while each of the other
# headers alone, and the whole set without this one, were answered 20/20 with
# 200. So this one is asked for on its own request (below), where the edge's
# refusal *is* the pass: a header that cannot reach the app cannot become
# `REMOTE_ADDR`. Where there is no such edge (Docker/Traefik, a published port)
# the request arrives and the header is checked like every other one.
EDGE_REFUSED_CLIENT_IP_HEADERS = ("CF-Connecting-IP",)


def facts_of(body):
    """The probe route's JSON facts, or `{}` for anything that is not them.

    An answer that is not the probe route's own object — the edge's error page,
    the app's traceback, a body that never arrived — is `{}`: the scenario must
    report what it was told rather than crash on a `json.loads` it did not
    expect, or read someone else's JSON as the probe's answer.
    """
    try:
        facts = json.loads(body)
    except (TypeError, ValueError):
        return {}
    return facts if isinstance(facts, dict) else {}


def refused_at_the_edge(status, server, body):
    """Whether a response is the *edge* refusing the request outright.

    Measured on App Platform: Cloudflare answers a client-sent
    `CF-Connecting-IP` with `403`, `error code: 1000`, and `Server: cloudflare`,
    and the origin never sees the request. A response from the deployment itself
    — the app's own 403, blastit's 500 — is not that: calling one of those an
    edge refusal would turn "the app is misbehaving" into a pass. A hang-up
    (`status` is `None`) is reported, and is not an edge refusal either: it says
    nothing about which header caused it.
    """
    if status == 200 or status is None:
        return False
    return "cloudflare" in (server or "").lower() or "error code:" in (body or "")


def describe(status):
    """An HTTP status, or the absence of one, for a line a human reads."""
    return f"HTTP {status}" if status is not None else "no response"


def request_probe(client, headers):
    """One `GET /debug/request/`, and everything the answer is made of.

    Returns `(status, server, facts, body)`: `status` is `None` when no HTTP
    response arrived at all (an edge can hang up instead of answering), `server`
    is the answer's `Server` header, and `facts` is the probe route's JSON when
    the answer was one — `{}` otherwise.
    """
    try:
        status, response_headers, body = client.request("GET", "/debug/request/", headers=headers)
    except Exception as e:  # noqa: BLE001 — urllib raises URLError when it is hung up on
        return None, "", {}, f"{type(e).__name__}: {e}"
    return status, str(response_headers.get("Server", "")), facts_of(body), body


def self_test():
    """The decisions this file makes about what it is told, asserted.

    `scenario.py --self-test`, and run by both e2e drivers before the scenario
    itself: the scenario needs a running deployment, these do not, and
    `is_client_address` is the one place where naming the wrong range would make
    the "is this address the ingress?" check pass for something that is not a
    client — or fail for a client that is one.
    """
    print("helpers")
    # A socket peer carries `ip:port`; a header's entry is a bare address.
    check(address_of("172.18.0.1:51234") == "172.18.0.1", "a peer's port is dropped")
    check(address_of("203.0.113.7") == "203.0.113.7", "a bare address is itself")
    check(address_of("[2001:db8::1]:443") == "2001:db8::1", "a bracketed IPv6 is unwrapped")
    check(address_of("") == "" and address_of(None) == "", "nothing is nothing")

    # The ingress a hosted deployment reports is the socket peer (§10), in
    # either cluster's address range; loopback is nobody's client either.
    for ingress in ("10.244.3.9", "100.127.1.1", "::1", "127.0.0.1:5000", "[::1]:80"):
        check(not is_client_address(ingress), f"{ingress} is not a client")
    # Everything else is one — including a CGNAT address, which is why
    # `100.64.0.0/10` is not excluded as a whole (§10: the range is also
    # carrier-NAT client space), and a private one, which is what a deployment
    # behind a corporate network reports.
    for client in ("203.0.113.7", "100.64.3.9", "100.126.0.1", "2001:db8::1", "172.18.0.1"):
        check(is_client_address(client), f"{client} is a client")
    check(not is_client_address(""), "an empty address is not a client")
    check(not is_client_address("garbage:1"), "neither is text")

    # CGI: the port is a key of its own, and a port is what it has to be.
    for port in ("51000", "1", "65535"):
        check(is_port(port), f"{port} is a port")
    for not_a_port in (None, "", "0", "65536", "127.0.0.1:51000", 51000, "-1"):
        check(not is_port(not_a_port), f"{not_a_port!r} is not a port")

    # E1's forged headers: every one a client sends is asked for exactly once,
    # between the request the app answers and the header the edge refuses — so
    # that dropping one from either list fails here rather than silently
    # weakening the check.
    mandate = {
        "X-Forwarded-For",
        "X-Real-Ip",
        "Forwarded",
        "X-Client-IP",
        "True-Client-IP",
        "CF-Connecting-IP",
        "do-connecting-ip",
    }
    check(
        set(FORGED_CLIENT_IP_HEADERS) | set(EDGE_REFUSED_CLIENT_IP_HEADERS) == mandate,
        "every client-IP header a client sends is asked for",
    )
    check(
        not set(FORGED_CLIENT_IP_HEADERS) & set(EDGE_REFUSED_CLIENT_IP_HEADERS),
        "and each of them exactly once",
    )
    check(
        all(FORGED_ADDRESS in value for value in FORGED_CLIENT_IP_HEADERS.values()),
        f"every one of them carries the lie ({FORGED_ADDRESS})",
    )

    # A probe answer is the route's object; anything else is no answer at all,
    # and a refusal is the edge's 403 — never a 200, an app's 500, or a hang-up.
    check(facts_of('{"remote_addr": "6.6.6.6"}') == {"remote_addr": FORGED_ADDRESS},
          "an answer is its JSON object")
    check(facts_of("<html>403 Forbidden</html>") == {}, "an error page is no answer")
    check(facts_of("") == {} and facts_of(None) == {}, "and neither is an empty one")
    check(refused_at_the_edge(403, "cloudflare", "error code: 1000\n"),
          "Cloudflare's 403 is the edge refusing")
    check(refused_at_the_edge(403, "", "error code: 1000\n"), "the error body alone says so")
    check(refused_at_the_edge(403, "cloudflare", ""), "the Server header alone says so")
    check(not refused_at_the_edge(200, "cloudflare", "{}"), "a 200 is never a refusal")
    check(not refused_at_the_edge(500, "gunicorn", "Traceback (most recent call last)"),
          "an app's 500 is not the edge")
    check(not refused_at_the_edge(403, "nginx", "Forbidden"), "nor is another server's 403")
    check(not refused_at_the_edge(None, "", "URLError: connection reset"),
          "nor a hang-up, which names no header")

    check(describe(403) == "HTTP 403" and describe(None) == "no response",
          "a status says what it is, including absent")

    class _HungUp:
        def request(self, *args, **kwargs):
            raise OSError("connection reset by peer")

    status, server, facts, body = request_probe(_HungUp(), {})
    check(
        status is None and facts == {} and "connection reset" in body,
        "a hang-up is reported, not raised",
    )
    check(not refused_at_the_edge(status, server, body), "and it is not a pass")

    print(f"\n{'PASS' if not FAILURES else 'FAIL'}: {len(FAILURES)} failure(s)")
    for failure in FAILURES:
        print(f"  - {failure}")
    return 1 if FAILURES else 0


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
    ap.add_argument("base", nargs="?", help="the URL to drive; omitted with --self-test")
    ap.add_argument("--expect", choices=["blastit", "baseline"],
                    help="what is serving the app; required unless --self-test")
    ap.add_argument("--password", default="demo-password")
    ap.add_argument("--wait", type=int, default=120)
    ap.add_argument("--self-test", action="store_true",
                    help="check the helpers of this file and exit (E1)")
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
    if args.self_test:
        return self_test()
    if not args.base or not args.expect:
        ap.error("a base URL and --expect are required (or use --self-test)")
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
    if under_blastit:
        # CGI and PEP 3333: `REMOTE_ADDR` is the address, and the connection's
        # port is `REMOTE_PORT`. A socket peer's `ip:port` in the first one, or a
        # `REMOTE_PORT` that is not a port, is a deployment the app cannot read
        # the way every other server lets it.
        check(
            honest_addr == address_of(honest_addr),
            f"REMOTE_ADDR is an address and nothing else ({honest_addr})",
        )
        check(
            is_port(facts.get("remote_port")),
            f"the connection's port is REMOTE_PORT ({facts.get('remote_port')!r})",
        )
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
    # them is a lie it cannot make the deployment believe. One of them does not
    # even get that far on App Platform — the edge refuses to carry it
    # (`EDGE_REFUSED_CLIENT_IP_HEADERS`) — so it is asked for on its own, below,
    # and the rest go in one request, which is the one that has to arrive.
    headers = dict(FORGED_CLIENT_IP_HEADERS)
    if args.forged_proto_ignored:
        headers["X-Forwarded-Proto"] = "https"
    status, _, facts, body = request_probe(forged, headers)
    if status != 200:
        print(f"  info the forged request: {describe(status)}: {body[:300]!r}", flush=True)
    check(status == 200, "the deployment answers one request carrying every forged header")
    forged_addr = facts.get("remote_addr")
    check(
        bool(forged_addr) and forged_addr != FORGED_ADDRESS,
        f"a client-IP header the client sent is never REMOTE_ADDR ({forged_addr})",
    )
    # The address, not the exact string: the two requests arrived as two
    # connections, and whichever port each came from is `REMOTE_PORT`, never
    # part of `REMOTE_ADDR`.
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
    for name in EDGE_REFUSED_CLIENT_IP_HEADERS:
        # Asked for alone. Where the edge refuses to carry it, the request never
        # reaches the app: a header no app is ever told cannot become
        # `REMOTE_ADDR` either, and what a client gets is the edge's answer.
        # Where there is no such edge, the request arrives and the header is
        # held to exactly the same two checks as the ones above.
        status, server, facts, body = request_probe(forged, {name: FORGED_ADDRESS})
        if status == 200:
            alone_addr = facts.get("remote_addr")
            check(
                bool(alone_addr) and alone_addr != FORGED_ADDRESS,
                f"a client-IP header the client sent alone is never REMOTE_ADDR ({alone_addr})",
            )
            check(
                address_of(alone_addr) == address_of(honest_addr),
                f"and the client the deployment sees is unchanged ({alone_addr})",
            )
        else:
            print(f"  info {name} alone: {describe(status)}: {body[:300]!r}", flush=True)
            check(
                refused_at_the_edge(status, server, body),
                f"the edge refuses to carry a client-sent {name} at all "
                f"({describe(status)}, the request never reaches the app)",
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
