"""Replay the `php2shell` matrix and the PHP page against a real `blastit dev`.

    php2shell.py --self-test
    php2shell.py --fixture /fixture --site /site --page /opt/e2e/php.md \
        --matrix /opt/e2e/probes.md

`docs/frameworks/php/index.md` is the page of record and `e2e/php/probes.md` is the
refusal matrix of record (a verbatim copy of the accepted §4.7 table, kept
because the design corpus it lived in was deleted with `ADR-0010`). This driver
is what turns both into evidence on the runtime a host actually installs.

**The premise, stated plainly.** The matrix is neutral because the surface is
**absent by construction**: there is no php-cgi binary, no FastCGI parameter
stream, no argv, and no INI channel a request can reach — the worker builds
`$_SERVER` and the `ZEND_INI_SYSTEM` blob in C. This suite does **not** claim
"even an unpatched runtime": the image is Debian's current `libphp8.4-embed`
(8.4.27), and no distribution ships an unpatched `libphp` that an 8.4 shim
could load, so that claim is neither made nor tested. `P4b` is the runtime half
that *is* tested — the same interpreter, the same canary file, `-d` instead of a
request — and rows 9 and 10 say in their note that on Linux NTS they cannot fail
at all (no Windows best-fit, no argv).

What it does:

* it starts `blastit dev` itself — the page's own command, on the page's own
  `127.0.0.1:8080` — and waits for it with a deadline, never a fixed sleep;
* `P1`–`P12` send the 24 client-reachable matrix rows one HTTP request at a
  time (raw sockets: the probe bytes are the point, so no client normalises
  them), and assert the row's expected status plus a **discriminating
  `/__inspect` subset** for every `200` row that answers that document;
* the canaries (`canaries/prepend.php`, `elsewhere/evil.php`,
  `public/uploads/shell.jpg`) must be absent from every response, and `P4b` is
  the **positive control**: the same canary file, the same runtime, `php -d`
  instead of `blastit` — its canary must be **present**, which is what makes
  "absent" mean "the channel is absent" and not "the canary is broken";
* the page has three `$ ` commands: `blastit dev --learn` is executed verbatim
  (it serves, exits `0` on `SIGINT`, and one `EmbeddedView` worker starts per
  `view`, each with its own pid); `uv tool install blastit` and
  `blastit --preset compose-traefik` are explained — a published-wheel install
  and a production preset owned by `e2e/rootless` — and an unknown `$ ` command
  is a failure;
* `blastit check` keeps the page's §8 sentence: exit `2` naming pytest, because
  a PHP manifest has no request audit in `0.2.0`;
* `--self-test` parses the matrix and asserts the 24 non-`—` rows (`E1`–`E17`,
  `E19`–`E25`) match `PROBES` 1-to-1, that the `php2shell #` column's union is
  exactly `{1..20}`, and that every `200` carries a discriminating assertion —
  so a row cannot silently stop being proved.

Stdlib only, no network beyond loopback, no fixed sleep.

Which answer the row expects differs from §4.7 where `ADR-0010` deleted the
mechanism the row was written against (there is no CGI head to malform and no
FastCGI param budget); each such row carries that reason in its `note`, and the
row is *stricter*, never looser: a status the old table called `502` is asserted
as the `200`-and-nothing-happened it must be, never skipped.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Self

ROOT = Path(__file__).resolve().parents[1]
PAGE = ROOT / "docs" / "frameworks" / "php" / "index.md"
MATRIX = ROOT / "e2e" / "php" / "probes.md"
FIXTURE = ROOT / "e2e" / "php" / "fixture"

PORT = 8080
#: The hashed stylesheet `docs/frameworks/php/index.md` promises the static
#: responder answers for a file under `public/`.
ASSET = "/assets/app.4f1c2a9b7d3e.css"
ASSET_NAME = ASSET.rsplit("/", 1)[-1]

#: `/__inspect`'s document, exactly (ADR-0010 §2, `MB.3.7`'s fixture contract).
INSPECT_KEYS = (
    "allow_url_include",
    "content_length",
    "document_root",
    "http_proxy",
    "path_info",
    "query_string",
    "request_method",
    "request_uri",
    "script_filename",
    "script_name",
    "server_keys",
    "user_ini",
)

#: What each canary prints when something executes it. A response carrying one
#: means a probe made PHP run a file it must never run.
CANARIES = {
    "prepend": b"PREPEND-RAN",
    "evil": b"EVIL-RAN",
    "polyglot": b"JPG-RAN",
}

#: `$ ` commands the driver cannot run here, and what owns them instead. Every
#: other `$ ` command on the page is replayed; an unexplained one fails.
SKIPS = {
    "uv tool install blastit": "installs the published wheel from PyPI; the container runs the wheel this build produced",
    "blastit --preset compose-traefik": "a production preset; e2e/rootless (E1) proves it",
}

_CONSOLE = re.compile(
    r"^```console\n(?P<body>.*?)^```[ \t]*$", re.MULTILINE | re.DOTALL
)


class Checks:
    """One PASS/FAIL line per promise; any failure makes the driver exit 1."""

    def __init__(self) -> None:
        self.passed = 0
        self.failed: list[str] = []

    def that(self, condition: bool, promise: str) -> bool:
        if condition:
            self.passed += 1
            print(f"PASS: {promise}", flush=True)
        else:
            print(f"FAIL: {promise}", flush=True)
            self.failed.append(promise)
        return bool(condition)


def console_commands(text: str) -> list[tuple[str, list[str]]]:
    """Every `$ ` command in a ```` ```console ```` fence, with the lines under it."""
    found: list[tuple[str, list[str]]] = []
    for match in _CONSOLE.finditer(text):
        current: list[str] | None = None
        for line in match.group("body").splitlines():
            if line.startswith("$ "):
                current = []
                found.append((line[2:], current))
            elif current is not None:
                current.append(line)
    trimmed = []
    for command, output in found:
        while output and not output[-1].strip():
            output.pop()
        trimmed.append((command, output))
    return trimmed


def command_kind(command: str) -> str | None:
    if command in ("uv tool install blastit", "uv add blastit"):
        return "install"
    if command in ("blastit dev --learn", "uv run blastit dev --learn"):
        return "dev-learn"
    if command in ("blastit fmt --check", "uv run blastit fmt --check"):
        return "fmt-check"
    if command in SKIPS:
        return "skip"
    return None


# ---------------------------------------------------------------- the matrix


@dataclass(frozen=True)
class Row:
    """One `§4.7` row: its number, its `E` name (`None` when `—`) and attacks."""

    number: int
    probe: str | None
    attacks: tuple[int, ...]


_ROW = re.compile(r"^\|\s*(\d+)\s*\|", re.MULTILINE)


def matrix_rows(text: str) -> list[Row]:
    """Every row of the matrix table: `#`, the `E` cell and its attack numbers.

    The table is the §4.7 one, so a row is `| n | E<n> | attack (a, b) | … |`;
    the `E` cell is `—` exactly once (row 18, which has no client HTTP path) and
    the attack column names `php2shell #` from `docs/research/php2shell.md` §B.
    """
    rows: list[Row] = []
    for line in text.splitlines():
        if not _ROW.match(line):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) < 3:
            raise ValueError(f"a matrix row with {len(cells)} cells: {line!r}")
        number = int(cells[0])
        probe = None if cells[1] in ("—", "-", "") else cells[1].strip("`")
        # Only the parenthesised list is the `php2shell #` column: a row whose
        # attack is a CVE names it in prose (`CVE-2012-1823 (12)`), and those
        # digits are not attack ids.
        attacks = tuple(
            sorted(
                int(number)
                for group in re.findall(r"\(([\d,\s]+)\)", cells[2])
                for number in re.findall(r"\d+", group)
            )
        )
        rows.append(Row(number, probe, attacks))
    if not rows:
        raise ValueError("the matrix table has no rows")
    return rows


# ---------------------------------------------------------------- the probes


@dataclass(frozen=True)
class Exchange:
    """One request the probe sends, and what its answer must be."""

    target: bytes
    expect: tuple[int, ...] = (200,)
    method: str = "GET"
    headers: tuple[tuple[bytes, bytes], ...] = ()
    body: bytes = b""
    #: Send the head and then nothing, even though `Content-Length` promises a
    #: body: the slow-loris row.
    stall: bool = False
    #: `("eq", key, value)`, `("absent", key, None)` or `("count", key, n)` over
    #: the `/__inspect` document. Every `200` exchange that answers that
    #: document has at least one, and the document must parse: an empty or
    #: non-JSON body fails the exchange instead of vacuously passing `absent`.
    asserts: tuple[tuple[str, str, object], ...] = ()
    #: Response header names the answer must **not** carry (row 24: the
    #: proxy-control names the transport used to honour).
    head_absent: tuple[str, ...] = ()
    #: Bytes the answer must start with, for a control that is not the JSON
    #: document (row 4: the polyglot exists and is served as a file).
    body_starts: bytes = b""
    #: A `200` that is the row's *positive control* rather than its probe: it
    #: proves the thing the refusal names exists, so the refusal is the gate
    #: and not a missing file.
    control: bool = False


@dataclass(frozen=True)
class Probe:
    """One matrix row's client reach, and the `P` group that serves it."""

    group: str
    exchanges: tuple[Exchange, ...]
    note: str = ""


def _form() -> tuple[tuple[bytes, bytes], ...]:
    return ((b"Content-Type", b"application/x-www-form-urlencoded"),)


#: One entry per client-reachable `E` row (`E1`–`E17`, `E19`–`E25`), keyed by the
#: matrix's own name. `{root}` is the served manifest directory on the wire.
PROBES: dict[str, Probe] = {
    "E1": Probe(
        "P1",
        (
            Exchange(
                b"/__inspect",
                headers=((b"SCRIPT_FILENAME", b"{root}/elsewhere/evil.php"),),
                asserts=(
                    ("eq", "script_filename", b"{root}/public/index.php"),
                    ("count", "SCRIPT_FILENAME", 1),
                ),
            ),
        ),
        "row 1: a header cannot name the script; `EVIL-RAN` must not appear",
    ),
    "E2": Probe(
        "P1",
        (
            Exchange(b"/../etc/passwd", expect=(400,)),
            Exchange(b"/%2e%2e/%2e%2e/etc/passwd", expect=(400,)),
            Exchange(b"/__inspect/trailing/../junk", expect=(400,)),
        ),
        "row 2: traversal in the path is refused before a route is chosen",
    ),
    "E3": Probe(
        "P1",
        (
            Exchange(
                b"/__inspect?x=1",
                headers=((b"SCRIPT_FILENAME", b"{root}/public/index.php?x=1"),),
                asserts=(
                    ("eq", "script_filename", b"{root}/public/index.php"),
                    ("eq", "query_string", "x=1"),
                ),
            ),
        ),
        "row 3: the pinned script never absorbs the query string",
    ),
    "E4": Probe(
        "P2",
        (
            Exchange(b"/uploads/shell.jpg/x.php", expect=(404,)),
            # The control: the file the truncation row names is served as a
            # file, so the 404 above is the entry gate, not "no such file".
            Exchange(
                b"/uploads/shell.jpg",
                body_starts=b"\xff\xd8\xff\xe0",
                control=True,
            ),
        ),
        "row 4: `fix_pathinfo` truncation cannot reach the polyglot, which exists "
        "(`GET /uploads/shell.jpg` is the control)",
    ),
    "E5": Probe(
        "P3",
        (
            Exchange(
                b"/__inspect",
                headers=((b"PHP_VALUE", b"allow_url_include=1"),),
                asserts=(("eq", "allow_url_include", "0"),),
            ),
            Exchange(
                b"/__inspect?PHP_VALUE=allow_url_include%3d1",
                asserts=(("eq", "allow_url_include", "0"),),
            ),
        ),
        "row 5: `PHP_VALUE` is a header and a query byte, never an INI channel",
    ),
    "E6": Probe(
        "P3",
        (
            Exchange(
                b"/__inspect",
                headers=((b"PHP_ADMIN_VALUE", b"allow_url_include=1"),),
                asserts=(("eq", "allow_url_include", "0"),),
            ),
        ),
        "row 6: `PHP_ADMIN_VALUE` has no channel either",
    ),
    "E7": Probe(
        "P4",
        (
            Exchange(
                b"/__inspect?PHP_VALUE=auto_prepend_file%3d{root}%2fcanaries%2fprepend.php",
                method="POST",
                headers=_form(),
                body=b'<?php echo "PREPEND-RAN";',
                asserts=(
                    ("eq", "request_method", "POST"),
                    ("eq", "allow_url_include", "0"),
                ),
            ),
        ),
        "row 7: the prepend chain is request data, and its canary is absent",
    ),
    "E8": Probe(
        "P5",
        (
            Exchange(
                b"/__inspect?-d+allow_url_include=1+-d+auto_prepend_file=php://input",
                method="POST",
                headers=_form(),
                body=b'<?php echo "CVE-2012-1823";',
                asserts=(
                    (
                        "eq",
                        "query_string",
                        "-d+allow_url_include=1+-d+auto_prepend_file=php://input",
                    ),
                    ("eq", "allow_url_include", "0"),
                ),
            ),
        ),
        "row 8: CVE-2012-1823 — the query string is data, there is no argv",
    ),
    "E9": Probe(
        "P6",
        (
            Exchange(
                b"/api/__inspect?%ADd+allow_url_include%3d1+%ADd+auto_prepend_file%3dphp://input",
                asserts=(
                    ("eq", "allow_url_include", "0"),
                    ("eq", "script_filename", b"{root}/public/index.php"),
                ),
            ),
        ),
        "row 9: CVE-2024-4577's soft hyphen is a byte, not a best-fit minus. "
        "On Linux NTS this row can never fail — the vulnerable path needs a "
        "Windows ANSI->UCS-2 best-fit and php-cgi's argv, neither of which "
        "exists here (`docs/research/php2shell.md` §B #13) — so it is kept as "
        "the tripwire a port would trip, and only `P4b` carries a runtime "
        "claim",
    ),
    "E10": Probe(
        "P6",
        (
            Exchange(
                b"/api/__inspect?%ADD+allow_url_include%3d1",
                asserts=(("eq", "allow_url_include", "0"),),
            ),
            Exchange(
                b"/api/__inspect?%AD%ADd+auto_prepend_file%3dphp://input",
                asserts=(("eq", "allow_url_include", "0"),),
            ),
        ),
        "row 10: CVE-2024-8926's encoding variants change no directive; like "
        "row 9 it cannot fail on Linux NTS (no best-fit, no argv) and is a "
        "tripwire, not a runtime claim",
    ),
    "E11": Probe(
        "P9",
        (
            Exchange(
                b"/__inspect?REDIRECT_STATUS=200",
                headers=((b"Redirect-Status", b"200"),),
                # The header is a normal `HTTP_*` entry; what must not
                # exist is the bare CGI name `cgi.force_redirect` reads. The
                # `eq` is what makes a non-JSON body fail rather than pass.
                asserts=(
                    ("eq", "script_filename", b"{root}/public/index.php"),
                    ("absent", "REDIRECT_STATUS", None),
                ),
            ),
        ),
        "row 11: `REDIRECT_STATUS`/CVE-2024-8927 — the name is never emitted",
    ),
    "E12": Probe(
        "P8",
        (
            Exchange(
                b"/__inspect?DOCUMENT_ROOT=/etc",
                headers=((b"Document-Root", b"/etc"),),
                asserts=(
                    ("eq", "document_root", b"{root}"),
                    ("eq", "user_ini", ""),
                ),
            ),
        ),
        "row 12: `DOCUMENT_ROOT` is pinned and no `.user.ini` is read",
    ),
    "E13": Probe(
        "P7",
        (
            Exchange(
                b"/__inspect",
                headers=(
                    (b"Proxy", b"http://attacker:8080"),
                    (b"HTTP_PROXY", b"http://attacker:8080"),
                ),
                asserts=(
                    ("eq", "http_proxy", False),
                    ("absent", "HTTP_PROXY", None),
                ),
            ),
        ),
        "row 13: httpoxy — the header is dropped, nothing can set HTTP_PROXY",
    ),
    "E14": Probe(
        "P10",
        (
            Exchange(
                b"/__inspect",
                headers=((b"X-Foo", b"first"), (b"X-Foo", b"second")),
                asserts=(
                    ("count", "HTTP_X_FOO", 1),
                    ("eq", "script_filename", b"{root}/public/index.php"),
                ),
            ),
        ),
        "row 14: a duplicate name reaches `$_SERVER` exactly once. Which value "
        "wins is not observable through the fixture's twelve keys (a header is "
        "not one of them), so the first-wins half is Rust-only "
        "(`critical_*`), not this probe",
    ),
    "E15": Probe(
        "P10",
        (
            Exchange(b"/__inspect\x00.jpg", expect=(400,)),
            Exchange(
                b"/__inspect",
                headers=((b"X-Test", b"a\x00b"),),
                expect=(400,),
            ),
        ),
        "row 15: a NUL is refused by the head, never truncated in C",
    ),
    "E16": Probe(
        "P10",
        (Exchange(b"/__inspect", headers=((b"X" * 66000, b"v"),), expect=(431,)),),
        "row 16: a 66 KiB header name is a bounded refusal",
    ),
    "E17": Probe(
        "P10",
        (
            Exchange(
                b"/__inspect",
                headers=tuple((b"X-P%d" % index, b"v") for index in range(130)),
                expect=(431,),
            ),
        ),
        "row 17: an over-budget head is refused before a worker sees it",
    ),
    "E19": Probe(
        "P11",
        (
            Exchange(b"/index.php", expect=(404,)),
            Exchange(b"/prepend.php", expect=(404,)),
        ),
        "row 19: `.php` under the static root is 404, never its source",
    ),
    "E20": Probe(
        "P2",
        (
            Exchange(b"/x.phar", expect=(404,)),
            Exchange(b"/shell.jpg/x.php", expect=(404,)),
        ),
        "row 20: `.phar` and the polyglot suffix are 404; the polyglot's own "
        "positive control (`GET /uploads/shell.jpg` is 200) is E4's",
    ),
    "E21": Probe(
        "P10",
        (
            Exchange(b"/__inspect", headers=((b"Cookie", b"a\rb"),), expect=(400,)),
            Exchange(
                b"/__inspect",
                headers=((b"Cookie", b"a\r\n\tb"),),
                expect=(400,),
            ),
        ),
        "row 21: a bare CR or an obs-fold is a malformed head, not a new param",
    ),
    "E22": Probe(
        "P12",
        (
            Exchange(
                b"/__inspect",
                method="POST",
                headers=((b"Content-Length", b"5"), (b"Transfer-Encoding", b"chunked")),
                body=b"a=b&c",
                expect=(400,),
            ),
            Exchange(
                b"/__inspect",
                method="POST",
                headers=((b"Content-Length", b"5"), (b"Content-Length", b"6")),
                body=b"a=b&c",
                expect=(400,),
            ),
        ),
        "row 22: `CL`+`TE` and a duplicate `CL` are refused before the scheduler",
    ),
    "E23": Probe(
        "P1",
        (Exchange(b"/api/../../etc/passwd", expect=(400,)),),
        "row 23: `..` in a path the static mount claims is refused by the "
        "mount's own resolver before a view sees it (400) — it is never joined "
        "onto `root`, and the script a view reaches stays the pinned entry "
        "(rows 1 and 9)",
    ),
    "E24": Probe(
        "P12",
        (
            Exchange(
                b"/__inspect",
                headers=(
                    (b"X-Accel-Redirect", b"/etc/passwd"),
                    (b"X-Sendfile", b"/etc/passwd"),
                ),
                asserts=(
                    ("eq", "script_filename", b"{root}/public/index.php"),
                    ("eq", "request_uri", b"/__inspect"),
                ),
                head_absent=("X-Accel-Redirect", "X-Sendfile"),
            ),
            Exchange(b"/__inspect", headers=((b"Bad Name", b"v"),), expect=(400,)),
        ),
        "row 24: the response head carries neither proxy-control name (both are "
        "`head_absent`), the body is the app's own, and a bad header name is 400 "
        "(there is no CGI head to malform: ADR-0010 deleted it)",
    ),
    "E25": Probe(
        "P10",
        (
            Exchange(
                b"/api/__inspect",
                method="POST",
                headers=((b"Content-Type", b"application/x-www-form-urlencoded"),),
                body=b"",
                stall=True,
                expect=(503, 504),
            ),
        ),
        "row 25: a body that never arrives is answered by a bounded refusal",
    ),
}


# ------------------------------------------------------------------- HTTP


class Response:
    def __init__(
        self, status: int, reason: str, headers: list[tuple[str, str]], body: bytes
    ) -> None:
        self.status = status
        self.reason = reason
        self.headers = headers
        self.body = body

    def header(self, name: str) -> str | None:
        for header, value in self.headers:
            if header.lower() == name.lower():
                return value
        return None

    def document(self) -> dict | None:
        try:
            document = json.loads(self.body)
        except ValueError:
            return None
        return document if isinstance(document, dict) else None


def _read_until(sock: socket.socket, needle: bytes) -> bytes:
    buffer = b""
    while needle not in buffer:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buffer += chunk
    return buffer


def _dechunk(data: bytes) -> bytes:
    out = b""
    while True:
        line, _, rest = data.partition(b"\r\n")
        if not line:
            return out
        try:
            size = int(line.split(b";")[0], 16)
        except ValueError:
            return out + data
        if size == 0:
            return out
        out += rest[:size]
        data = rest[size + 2 :]


def request(
    host: str,
    port: int,
    exchange: Exchange,
    root: bytes,
    timeout: float = 20.0,
) -> Response:
    """One HTTP/1.1 exchange on a raw socket, with the probe's own bytes.

    `{root}` is substituted in the head and body so a row names the fixture's
    real directory; nothing else is normalised, which is the point.
    """

    def sub(value: bytes) -> bytes:
        return value.replace(b"{root}", root)

    lines = [b"%s %s HTTP/1.1" % (exchange.method.encode(), sub(exchange.target))]
    lines.append(b"Host: %s:%d" % (host.encode(), port))
    lines.append(b"Connection: close")
    body = sub(exchange.body)
    if exchange.stall:
        # A declared body that never arrives: the slow-loris row. The number is
        # larger than what is sent, which is the whole attack.
        lines.append(b"Content-Length: %d" % (len(body) + 64))
    elif body or exchange.method not in ("GET", "HEAD"):
        lines.append(b"Content-Length: %d" % len(body))
    lines += [b"%s: %s" % (sub(name), sub(value)) for name, value in exchange.headers]
    payload = b"\r\n".join(lines) + b"\r\n\r\n" + body

    with socket.create_connection((host, port), timeout=timeout) as sock:
        sock.settimeout(timeout)
        sock.sendall(payload)
        buffer = _read_until(sock, b"\r\n\r\n")
        head, _, rest = buffer.partition(b"\r\n\r\n")
        if not head:
            raise OSError(f"{exchange.method} {exchange.target!r}: no response head")
        head_lines = head.decode("latin-1").split("\r\n")
        protocol, _, remainder = head_lines[0].partition(" ")
        status, _, reason = remainder.partition(" ")
        if protocol != "HTTP/1.1" or not status.isdigit():
            raise OSError(f"bad status line {head_lines[0]!r}")
        parsed = []
        for line in head_lines[1:]:
            name, separator, value = line.partition(":")
            if not separator:
                raise OSError(f"bad header line {line!r}")
            parsed.append((name.strip(), value.strip()))
        response = Response(int(status), reason, parsed, b"")
        if exchange.method == "HEAD" or int(status) in (204, 304):
            return response
        if response.header("transfer-encoding") == "chunked":
            data = rest
            while b"\r\n0\r\n" not in data and not data.endswith(b"0\r\n\r\n"):
                chunk = sock.recv(65536)
                if not chunk:
                    break
                data += chunk
            response.body = _dechunk(data)
            return response
        length = response.header("content-length")
        if length is not None and length.isdigit():
            wanted = int(length)
            data = rest
            while len(data) < wanted:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                data += chunk
            response.body = data[:wanted]
            return response
        data = rest
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            data += chunk
        response.body = data
        return response


# ------------------------------------------------------------------ the boot


class Server:
    """A `blastit dev` the driver owns, with its JSON events on disk."""

    def __init__(self, site: Path, log: Path, learn: bool = False) -> None:
        self.log_path = log
        self.arguments = ["blastit", "dev"] + (["--learn"] if learn else [])
        environment = dict(os.environ)
        environment["BLASTIT_LOG_FORMAT"] = "json"
        self.log = open(log, "wb")  # noqa: SIM115 - closed in `stop`
        self.process = subprocess.Popen(
            self.arguments,
            cwd=site,
            env=environment,
            stdout=self.log,
            stderr=subprocess.STDOUT,
        )

    def wait_ready(self, timeout: float = 120.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                self.log.close()
                raise SystemExit(
                    f"php2shell: `{' '.join(self.arguments)}` exited "
                    f"{self.process.returncode}; log:\n{self.log_path.read_text()[-4000:]}"
                )
            try:
                if request("127.0.0.1", PORT, Exchange(b"/"), b"").status == 200:
                    return
            except OSError:
                pass
            time.sleep(0.1)
        self.stop()
        raise SystemExit(f"php2shell: `blastit dev` did not answer within {timeout}s")

    def events(self) -> list[dict]:
        found = []
        for line in self.log_path.read_text(errors="replace").splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if isinstance(event, dict):
                found.append(event)
        return found

    def stop(self, sig: int = signal.SIGTERM, timeout: float = 30.0) -> int:
        if self.process.poll() is None:
            self.process.send_signal(sig)
        try:
            code = self.process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.process.kill()
            code = self.process.wait(timeout=10)
        self.log.close()
        return code

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *_exc: object) -> None:
        if self.process.poll() is None:
            self.stop()


# ---------------------------------------------------------------- the checks


def assert_exchange(
    checks: Checks, label: str, exchange: Exchange, response: Response, root: bytes
) -> None:
    """The row's status, then every assertion the row makes about `$_SERVER`."""
    expected = "/".join(str(code) for code in exchange.expect)
    # One sentence per outcome: the expected list and the answer are the same
    # fact on a pass, and printing both read as "the answer is 200, not 200".
    message = (
        f"{label}: the answer is {response.status}"
        if response.status in exchange.expect
        else f"{label}: the answer is {expected}, not {response.status}"
    )
    checks.that(response.status in exchange.expect, message)
    for name in exchange.head_absent:
        present = response.header(name)
        checks.that(
            present is None,
            f"{label}: the response head carries no `{name}` ({present!r})",
        )
    if exchange.body_starts:
        checks.that(
            response.body.startswith(exchange.body_starts),
            f"{label}: the body is the file (`{exchange.body_starts!r}`…), not an "
            f"error page ({response.body[:16]!r})",
        )
    document = response.document()
    if exchange.asserts:
        # A body that does not parse would make every `absent` assertion pass
        # against an empty document: the answer has to *be* `/__inspect`'s.
        checks.that(
            document is not None,
            f"{label}: the answer is the twelve-key document, not {response.body[:40]!r}",
        )
    for kind, key, value in exchange.asserts:
        if kind == "eq":
            got = (document or {}).get(key)
            if isinstance(value, bytes):
                taken = value.replace(b"{root}", root)
                ok = isinstance(got, str) and got.encode() == taken
                shown = taken.decode(errors="replace")
            else:
                ok = got == value
                shown = value
            checks.that(ok, f"{label}: `{key}` is {shown!r}, not {got!r}")
        elif kind == "absent":
            keys = (document or {}).get("server_keys") or []
            checks.that(
                key not in keys,
                f"{label}: `{key}` is not in `array_keys($_SERVER)`",
            )
        elif kind == "count":
            keys = (document or {}).get("server_keys") or []
            checks.that(
                keys.count(key) == value,
                f"{label}: `{key}` appears {value} time(s) in `array_keys($_SERVER)`, not {keys.count(key)}",
            )
        else:
            raise ValueError(f"unknown assertion {kind!r}")


#: Rows whose *pass* is not evidence about this runtime, printed before the run
#: so the log cannot be read as more than it is.
CAVEATS = {
    "E9": "cannot fail on Linux NTS (no Windows best-fit, no argv): a tripwire for a port",
    "E10": "same as E9: CVE-2024-8926's encodings change no directive here",
    "E14": "only 'one entry' is observable; which duplicate value wins is Rust-only",
}


def run_probes(checks: Checks, matrix_root: bytes) -> None:
    """`P1`–`P12`: one request (or a few) per matrix row, and no canary."""
    for name, why in CAVEATS.items():
        print(f"INFO: {name} — {why}", flush=True)
    for name in sorted(PROBES, key=lambda item: int(item[1:])):
        probe = PROBES[name]
        for index, exchange in enumerate(probe.exchanges):
            label = f"{probe.group} {name}" + (f"[{index}]" if index else "")
            response = request("127.0.0.1", PORT, exchange, matrix_root)
            assert_exchange(checks, label, exchange, response, matrix_root)
            found = [marker for marker in CANARIES.values() if marker in response.body]
            checks.that(
                not found,
                f"{label}: no canary in the response ({found})",
            )
            if name == "E19":
                checks.that(
                    b"<?php" not in response.body,
                    f"{label}: the response carries no PHP source",
                )
            if name == "E24" and index == 0:
                checks.that(
                    b"root:x:0:0" not in response.body,
                    f"{label}: a proxy-control header names no host file",
                )


def positive_control(checks: Checks, site: Path) -> None:
    """`P4b`: the canary E7 watches, run by this runtime with `-d` — present.

    The control names the **same file** row 7's `PHP_VALUE` names
    (`canaries/prepend.php`) and the same directive (`auto_prepend_file`), so
    the marker E7 asserts absent is shown executable on this exact interpreter.
    That is what makes "the canary is absent" mean "the channel is absent" and
    not "the canary is broken".
    """
    target = site / "canaries" / "prepend-target.php"
    target.write_text('<?php echo "TARGET-RAN\\n";\n', encoding="utf-8")
    try:
        completed = subprocess.run(
            [
                "php",
                "-d",
                f"auto_prepend_file={site}/canaries/prepend.php",
                str(target),
            ],
            capture_output=True,
            timeout=60,
            check=False,
        )
    finally:
        target.unlink(missing_ok=True)
    output = completed.stdout + completed.stderr
    checks.that(
        CANARIES["prepend"] in output,
        "P4b: `php -d auto_prepend_file=…/canaries/prepend.php` runs it "
        "(`PREPEND-RAN` present)",
    )
    checks.that(
        b"TARGET-RAN" in output,
        "P4b: and then the script itself ran, so the prepend was not a crash",
    )


def check_static(checks: Checks, site: Path) -> None:
    """The page's §2 promise: a file under `public/` is answered by the responder."""
    response = request("127.0.0.1", PORT, Exchange(ASSET.encode()), b"")
    checks.that(response.status == 200, f"`{ASSET}` is 200")
    checks.that(
        response.body == (site / "public" / ASSET.lstrip("/")).read_bytes(),
        f"`{ASSET}` is `public/{ASSET_NAME}`'s bytes",
    )
    checks.that(
        (response.header("content-type") or "").startswith("text/css"),
        f"`{ASSET}` is served as `text/css`",
    )
    entry = request("127.0.0.1", PORT, Exchange(b"/__inspect"), b"")
    document = entry.document()
    checks.that(
        isinstance(document, dict) and sorted(document) == sorted(INSPECT_KEYS),
        f"`/__inspect` answers exactly the twelve keys ({sorted(document or {})})",
    )
    checks.that(
        (document or {}).get("script_filename", "").encode()
        == str(site / "public" / "index.php").encode(),
        "and `script_filename` is the file `entrypoint php` names",
    )


def check_learn(checks: Checks, site: Path) -> None:
    """The page's `blastit dev --learn`: serves, records on exit, exits `0`.

    The file declares no store grant, so there is no statement to record — the
    page's own sentence for a manifest whose views make no query. What is
    asserted is that the command runs, answers, and leaves the file it was
    pointed at intact, plus the `EmbeddedView` workers the ADR promises: one per
    view, each with its own pid and the one entrypoint.
    """
    before = (site / "Blastfile").read_bytes()
    with Server(site, site / "dev-learn.log", learn=True) as server:
        try:
            server.wait_ready()
        except SystemExit as error:
            checks.that(False, f"`blastit dev --learn` starts: {error}")
            return
        # The live process's own argv, not the vector this script built: stage 0
        # re-execs into stage 1 with `--dev-obs-fd` **only** for a `--learn`
        # boot (`stage0::exec_stage1_dev`), and the pid is stable across that
        # `execve`. A `blastit` that ignored `--learn` would leave no such
        # argument, while `command_kind` on the constructor's list could never notice.
        try:
            cmdline = [
                arg
                for arg in Path(f"/proc/{server.process.pid}/cmdline")
                .read_bytes()
                .split(b"\0")
                if arg
            ]
        except OSError as error:
            cmdline = []
            checks.that(False, f"reading the live argv: {error}")
        for target in (b"/users/42", b"/api/x"):
            response = request("127.0.0.1", PORT, Exchange(target), b"")
            checks.that(
                response.status == 200,
                f"`blastit dev --learn` serves `{target.decode()}`",
            )
        code = server.stop(sig=signal.SIGINT)
        events = server.events()
    checks.that(code == 0, f"`blastit dev --learn` exits 0 on SIGINT ({code})")
    # The boot's own `ready` event, read as a JSON event: a text grep for
    # `ready ` also matches an unrelated line, and one for the encoded spelling
    # is what a broken log format would not produce anyway.
    ready = [event for event in events if event.get("event") == "ready"]
    checks.that(
        len(ready) >= 1,
        f"`blastit dev --learn` reported itself ready ({len(ready)} `ready` event(s))",
    )
    spawned = [event for event in events if event.get("event") == "process.spawned"]
    routes = sorted({event.get("route") for event in spawned if event.get("route")})
    checks.that(
        routes == ["api", "app"],
        f"one worker per view, each its own pid ({routes})",
    )
    checks.that(
        len({event.get("pid") for event in spawned if event.get("route") in routes})
        >= 2,
        "and the two views are two processes, not one",
    )
    checks.that(
        (site / "Blastfile").read_bytes() == before,
        "`--learn` records nothing: this file's views run no query",
    )
    # `--learn`'s own observable, from the process that actually ran: stage 1's
    # argv carries `--dev-obs-fd=` only for a `--learn` boot. (`command_kind` on
    # `server.arguments` was the constructor's vector classified against a
    # literal — it could not fail if `blastit` ignored the flag.)
    flags = [arg.decode(errors="replace") for arg in cmdline if arg.startswith(b"--")]
    checks.that(
        any(arg.startswith(b"--dev-obs-fd=") for arg in cmdline),
        f"the live boot ran as `--learn` (`--dev-obs-fd` in its argv: {flags})",
    )


def check_check(checks: Checks, site: Path) -> None:
    """The page's §8 sentence: `check` exits `2` without pytest, audits nothing."""
    completed = subprocess.run(
        ["blastit", "check"],
        cwd=site,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    output = completed.stdout + completed.stderr
    checks.that(
        completed.returncode == 2,
        f"`blastit check` exits 2 on a PHP manifest with no pytest ({completed.returncode})",
    )
    checks.that(
        "pytest is not installed" in output,
        f"and names pytest as what is missing ({output.strip()[:200]!r})",
    )


def check_fmt(checks: Checks, site: Path) -> None:
    """The page's §What CI can prove: `blastit fmt --check` passes on the fixture.

    The page calls this gate the only CI proof in 0.2.0, so the leg runs it on
    the manifest the probes served from rather than trusting the prose. The
    file is read before and after because `--check` promises to name a file,
    never rewrite one.
    """
    before = (site / "Blastfile").read_bytes()
    completed = subprocess.run(
        ["blastit", "fmt", "--check"],
        cwd=site,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    output = (completed.stdout + completed.stderr).strip()
    checks.that(
        completed.returncode == 0,
        f"`blastit fmt --check` accepts the page's fixture ({completed.returncode}: {output[:200]!r})",
    )
    checks.that(
        (site / "Blastfile").read_bytes() == before,
        "and `--check` changed nothing (it names a file, never rewrites one)",
    )


def replay(checks: Checks, page: str, matrix: str, site: Path) -> None:
    commands = console_commands(page)
    checks.that(
        bool(commands), "the page still has ```console fences with `$ ` commands"
    )
    for command, _ in commands:
        checks.that(
            command_kind(command) is not None,
            f"the page's command is replayed or explained: {command}",
        )
    for command, _ in commands:
        if command_kind(command) == "skip":
            print(f"SKIP: {command} ({SKIPS[command]})", flush=True)

    # Before `--learn`: the gate is about the file as committed, and the page
    # presents `fmt --check` as the CI proof a clean tree passes.
    if any(command_kind(command) == "fmt-check" for command, _ in commands):
        check_fmt(checks, site)

    rows = matrix_rows(matrix)
    checks.that(
        len(rows) == 25 and sum(1 for row in rows if row.probe) == 24,
        f"the matrix this run replayed is the 24-reachable-row one ({len(rows)} rows)",
    )
    check_learn(checks, site)
    check_check(checks, site)


# --------------------------------------------------------------- self-test


def self_test(page: Path, matrix: Path) -> int:
    checks = Checks()

    rows = matrix_rows(matrix.read_text(encoding="utf-8"))
    reachable = [row for row in rows if row.probe is not None]
    checks.that(len(rows) == 25, f"the matrix has 25 rows ({len(rows)})")
    checks.that(
        len(reachable) == 24, f"24 of them are client-reachable ({len(reachable)})"
    )
    checks.that(
        [row.probe for row in reachable]
        == [f"E{n}" for n in list(range(1, 18)) + list(range(19, 26))],
        "they are `E1`–`E17` and `E19`–`E25`, in order",
    )
    checks.that(
        [(row.number, row.probe is None) for row in rows].count((18, True)) == 1,
        "row 18 is the one `—` (an internal guard with no HTTP path)",
    )
    attacks = {attack for row in rows for attack in row.attacks}
    checks.that(
        attacks == set(range(1, 21)),
        f"the `php2shell #` column's union is exactly 1..20 ({sorted(attacks)})",
    )
    checks.that(
        sorted(PROBES) == sorted(row.probe for row in reachable),
        "`PROBES` matches the matrix's reachable rows 1-to-1",
    )
    checks.that(
        len(PROBES) == 24, f"`PROBES` has one entry per reachable row ({len(PROBES)})"
    )

    groups = {probe.group for probe in PROBES.values()}
    checks.that(
        groups == {f"P{n}" for n in range(1, 13)},
        f"`P1`–`P12` all serve at least one row ({sorted(groups)})",
    )
    checks.that("P4" in groups, "`P4` is the prepend chain whose control is `P4b`")

    everything = [exchange for probe in PROBES.values() for exchange in probe.exchanges]
    checks.that(
        all(
            exchange.asserts or (exchange.control and exchange.body_starts)
            for exchange in everything
            if 200 in exchange.expect
        ),
        "every `200` request carries a discriminating assertion (keys, or a "
        "control's own bytes)",
    )
    checks.that(
        all(
            exchange.control and exchange.body_starts
            for exchange in everything
            if exchange.control
        ),
        "every positive control asserts the bytes it is a control for",
    )
    checks.that(
        any(exchange.control for exchange in everything),
        "the polyglot row keeps its positive control (row 4)",
    )
    checks.that(
        bool(PROBES["E24"].exchanges[0].head_absent),
        "row 24 asserts the response head, not only its body",
    )
    checks.that(
        all(
            key in INSPECT_KEYS
            for exchange in everything
            for kind, key, _ in exchange.asserts
            if kind == "eq"
        ),
        "and every `eq` assertion names a key the fixture answers",
    )
    for name, probe in PROBES.items():
        for exchange in probe.exchanges:
            for kind, key, value in exchange.asserts:
                if kind == "eq" and key == "script_filename":
                    checks.that(
                        value.endswith(b".php"),
                        f"{name}: `script_filename` is asserted as a script",
                    )
    checks.that(
        all(probe.note for probe in PROBES.values()),
        "every probe says which row it serves",
    )

    text = page.read_text(encoding="utf-8")
    commands = console_commands(text)
    kinds = [command_kind(command) for command, _ in commands]
    checks.that(
        commands and None not in kinds,
        f"every `$ ` command on the page is replayed or explained ({kinds})",
    )
    checks.that(
        kinds == ["dev-learn", "fmt-check"],
        f"the page's console commands are the two this leg replays ({kinds})",
    )
    checks.that(
        "unknown-linux" in text and "PyPI wheel ships none" in text,
        "the page still says where the binary comes from: a release tarball, not the wheel",
    )
    checks.that(
        "blastit check" in text and "pytest" in text,
        "the page still says `blastit check` runs pytest",
    )

    print(
        f"PASS: self-test, {checks.passed} checks"
        if not checks.failed
        else f"FAIL: self-test, {len(checks.failed)} of {checks.passed + len(checks.failed)} checks"
    )
    return 1 if checks.failed else 0


# -------------------------------------------------------------------- main


def prepare(fixture: Path, site: Path) -> None:
    shutil.rmtree(site, ignore_errors=True)
    shutil.copytree(fixture, site)
    # The canaries are installed at the paths the probes name: a `.php` under
    # the static root (refused), the polyglot the truncation row reaches, and
    # the prepend file an INI channel would load.
    (site / "public" / "prepend.php").write_bytes(
        (site / "canaries" / "prepend.php").read_bytes()
    )
    (site / "public" / "uploads").mkdir(parents=True, exist_ok=True)
    (site / "public" / "uploads" / "shell.jpg").write_bytes(
        (site / "canaries" / "shell.jpg").read_bytes()
    )
    (site / "public" / ".user.ini").write_text(
        f"auto_prepend_file={site}/canaries/prepend.php\n", encoding="utf-8"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--fixture", type=Path, default=FIXTURE)
    parser.add_argument("--site", type=Path, help="where the served copy is made")
    parser.add_argument("--page", type=Path, default=PAGE)
    parser.add_argument("--matrix", type=Path, default=MATRIX)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="check the matrix, `PROBES` and the page's shape, then exit",
    )
    arguments = parser.parse_args()

    if arguments.self_test:
        return self_test(arguments.page, arguments.matrix)

    site = arguments.site or Path(os.environ.get("TMPDIR", "/tmp")) / "php2shell-site"
    checks = Checks()
    page = arguments.page.read_text(encoding="utf-8")
    matrix = arguments.matrix.read_text(encoding="utf-8")
    checks.that(bool(page) and bool(matrix), "the page and the matrix are readable")
    prepare(arguments.fixture, site)
    root = str(site).encode()

    with Server(site, site / "dev.log") as server:
        server.wait_ready()
        check_static(checks, site)
        run_probes(checks, root)

    positive_control(checks, site)
    replay(checks, page, matrix, site)

    total = checks.passed + len(checks.failed)
    if checks.failed:
        print(f"FAIL: {len(checks.failed)} of {total} promises failed", flush=True)
        return 1
    print(
        f"PASS: the php2shell matrix and docs/frameworks/php/index.md replayed against a "
        f"real blastit dev ({total} checks)",
        flush=True,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
