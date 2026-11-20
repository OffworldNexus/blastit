"""The DX-invariance replay: capture §1's seven rows, then diff after the mask.

The design (`docs/testing/AUDIT-MB.36.md` §2 and `view-crates-architecture.md`
§1) promises that moving the view crates changes nothing a developer types or
reads. This driver makes that falsifiable: it runs the same documented commands
against one tree's binary and writes a capture; the audit runs it on the tree
before the epic and on the epic head, and diffs the two captures after
`crates/blastit/tests/dx-mask.txt`'s mask.

    uv run --no-project e2e/dx_replay.py run --binary PATH --out DIR \
        [--binary-no-default PATH] [--python PATH]
    uv run --no-project e2e/dx_replay.py normalise --out DIR
    uv run --no-project e2e/dx_replay.py self-test

`run` also normalises what it wrote, so a capture that cannot be parsed is a
failure here, not an empty diff later. Stdlib only: the replay must run on a
machine that has nothing but Docker-free Python.

`self-test` is the parity check with the tested implementation: it is a second
implementation of the mask (it may not depend on a Rust test binary), so it
normalises `crates/blastit/tests/dx-parity.txt` and compares the result with
`dx-parity.expected` — the same vector `dx_capture.rs`'s
`the_replay_driver_and_this_normaliser_agree_on_one_golden_capture` asserts. A
rule added to one side only fails here.

The seven rows, as the design numbers them:

1. `blastit dev` on `static "/" { root "public" }`: `200` + bytes + `ETag`,
   `404`, `400`.
2. `blastit dev` on `entrypoint php "public/index.php"`: two `200`s and two
   `request` events with the same status.
3. `blastit dev` on `entrypoint python "app:application"`: the app's status and
   body on `GET`, a JSON `POST` that reaches the app, a `413` on a view that
   declares no body.
4. `blastit dev --no-reload`: no `reload: on` line.
5. `blastit check` and `blastit check --json` on a complete fixture and on one
   missing a grant: the exit codes and the document.
6. `BLASTIT_LIBPHP=/dev/null`: exit 1, one sentence naming the route, its
   `entry`, the distro package and `BLASTIT_LIBPHP`.
7. the `--no-default-features` build on the WSGI fixture: exit 1, one sentence
   naming `uv add blastit`.
"""

import argparse
import json
import os
import re
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MASK = ROOT / "crates/blastit/tests/dx-mask.txt"
HOST_MASK = ROOT / "crates/blastit/tests/dx-mask-host.txt"

# The documented ports: they are part of the banner, so both captures must use
# the same ones.
PORTS = {"static": 18191, "php": 18192, "wsgi": 18193, "no-reload": 18194}
CHECK_EXIT_FINDINGS = 1
EXIT_FAILED = 1

STATIC_MANIFEST = 'static "/" {\n    root "public"\n}\n'
STATIC_INDEX = b"<!doctype html><h1>dx-invariance</h1>\n"
WSGI_MANIFEST = """app "dx"

entrypoint python "app:application"

view echo POST "/echo" {
    body json
}

view bare POST "/bare" {}

view home GET "/" {}
"""
WSGI_APP = """def application(environ, start_response):
    length = int(environ.get("CONTENT_LENGTH") or 0)
    body = environ["wsgi.input"].read(length) if length else b""
    if environ.get("PATH_INFO") == "/echo":
        start_response("200 OK", [("Content-Type", "application/json")])
        return [b'{"received":' + str(len(body)).encode() + b"}"]
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"wsgi-fixture"]
"""

#: The `shop` project: one store with one granted statement, and a WSGI app that
#: runs it through Blastit's proxy inside the audited suite, so exit 0 means the
#: grant matched *and* the statement reached the upstream.
SHOP_MANIFEST = """app "shop"

entrypoint python "app:application"

postgres $DATABASE_URL

view home regex '.*' {
    postgres {
        query 'SELECT 1'
    }
}
"""

SHOP_APP = """import pgwire


def application(environ, start_response):
    if environ.get("PATH_INFO") in ("", "/", "/products/"):
        pgwire.simple(["SELECT 1"])
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"ok"]
"""

SHOP_TEST = """import io

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
        gate.wrap(application)(environ, lambda code, headers: status.update(code=code))
    )
    return status.get("code"), payload


def test_products():
    code, payload = request(path="/products/")
    assert code == "200 OK"
    assert payload == b"ok"
"""

#: The `findings` project: the same shape, with `SELECT 9` run and never granted:
#: the audit must report it (exit 1) rather than pass or crash.
FINDINGS_MANIFEST = """app "findings"

entrypoint python "app:application"

postgres $DATABASE_URL

postgres analytics $ANALYTICS_URL

view catalog "/catalog" {
    postgres {
        query 'SELECT 1'
    }
}

view search "/search" {
    postgres {
        query 'SELECT 9'
    }
}

view checkout "/checkout" {}

view upload POST "/upload" {}
"""

FINDINGS_APP = """import pgwire


def application(environ, start_response):
    path = environ.get("PATH_INFO", "")
    if path == "/catalog":
        pgwire.simple(["SELECT 1", "SELECT 2"])
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"ok\\n"]
"""

FINDINGS_TEST = """import io

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
        gate.wrap(application)(environ, lambda code, headers: status.update(code=code))
    )
    return status.get("code"), payload


def test_catalog():
    assert request(path="/catalog")[0] == "200 OK"


def test_search():
    # No statement here: the grant the view declares is the unused one, and the
    # statement the catalog runs is the missing one. Either is a finding.
    assert request(path="/search")[0] == "200 OK"


def test_upload():
    assert request("POST", "/upload", b"name=blastit")[0] == "200 OK"
"""

PYTEST_INI = "[pytest]\naddopts = -p no:cacheprovider\n"

#: The fixture's PostgreSQL client: the wire protocol, not a driver. `socket` is
#: in the standard library, and `blastit` hands the app a local URL, so every
#: byte still crosses the proxy.
PGWIRE = '''"""The fixture app's PostgreSQL client: the wire protocol, not a driver."""

import os
import socket
import struct
from urllib.parse import parse_qs, urlsplit


def _message(kind, payload=b""):
    return kind + struct.pack("!I", len(payload) + 4) + payload


def _read(sock, count):
    data = b""
    while len(data) < count:
        chunk = sock.recv(count - len(data))
        if not chunk:
            raise EOFError
        data += chunk
    return data


def _error_message(body):
    fields = {}
    for field in body.split(b"\\0"):
        if len(field) >= 2:
            fields[field[0:1]] = field[1:].decode("utf-8", "replace")
    return f"SQLSTATE {fields.get(b'C', '?????')}: {fields.get(b'M', '')}"


def _until_ready(sock):
    while True:
        kind = _read(sock, 1)
        length = struct.unpack("!I", _read(sock, 4))[0]
        body = _read(sock, length - 4)
        if kind == b"E":
            raise RuntimeError(_error_message(body))
        if kind == b"Z":
            return


def _connect():
    url = urlsplit(os.environ["DATABASE_URL"])
    query = parse_qs(url.query)
    host = query["host"][0]
    port = query.get("port", ["5432"])[0]
    database = url.path.lstrip("/") or "shop"
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(f"{host}/.s.PGSQL.{port}")
    params = b"user\\0blastit\\0database\\0" + database.encode() + b"\\0\\0"
    sock.sendall(struct.pack("!I", len(params) + 8) + struct.pack("!I", 196608) + params)
    _until_ready(sock)
    return sock


def simple(statements):
    sock = _connect()
    try:
        for statement in statements:
            sock.sendall(_message(b"Q", statement.encode() + b"\\0"))
            _until_ready(sock)
    finally:
        sock.close()
'''

SHOP_FILES = {
    "Blastfile": SHOP_MANIFEST,
    "pgwire.py": PGWIRE,
    "pytest.ini": PYTEST_INI,
    "app.py": SHOP_APP,
    "tests/test_app.py": SHOP_TEST,
}

FINDINGS_FILES = {
    "Blastfile": FINDINGS_MANIFEST,
    "pgwire.py": PGWIRE,
    "pytest.ini": PYTEST_INI,
    "app.py": FINDINGS_APP,
    "tests/test_app.py": FINDINGS_TEST,
}


def _pg_message(kind, payload=b""):
    return kind + struct.pack("!I", len(payload) + 4) + payload


def _pg_cstring(text):
    return text.encode() + b"\0"


def _pg_row_description(name="?column?", oid=23):
    body = (
        struct.pack("!h", 1)
        + _pg_cstring(name)
        + struct.pack("!ihihih", 0, 0, oid, -1, -1, 0)
    )
    return _pg_message(b"T", body)


def _pg_data_row(value=b"1"):
    return _pg_message(
        b"D", struct.pack("!h", 1) + struct.pack("!i", len(value)) + value
    )


def _pg_command_complete(tag):
    return _pg_message(b"C", _pg_cstring(tag))


def _pg_ready():
    return _pg_message(b"Z", b"I")


def _pg_run(sql):
    head = sql.strip().lower()
    if head.startswith("select"):
        return _pg_row_description() + _pg_data_row() + _pg_command_complete("SELECT 1")
    if head.startswith("begin"):
        return _pg_command_complete("BEGIN")
    if head.startswith(("commit", "end")):
        return _pg_command_complete("COMMIT")
    if head.startswith("rollback"):
        return _pg_command_complete("ROLLBACK")
    if head.startswith("set"):
        return _pg_command_complete("SET")
    return _pg_command_complete("OK")


def _pg_startup():
    """AuthenticationOk, the ParameterStatus set libpq needs, ReadyForQuery."""
    return (
        _pg_message(b"R", struct.pack("!i", 0))
        + _pg_message(b"S", _pg_cstring("server_version") + _pg_cstring("16.0"))
        + _pg_message(b"S", _pg_cstring("client_encoding") + _pg_cstring("UTF8"))
        + _pg_message(
            b"S", _pg_cstring("standard_conforming_strings") + _pg_cstring("on")
        )
        + _pg_message(b"S", _pg_cstring("DateStyle") + _pg_cstring("ISO, MDY"))
        + _pg_message(b"S", _pg_cstring("integer_datetimes") + _pg_cstring("on"))
        + _pg_message(b"S", _pg_cstring("TimeZone") + _pg_cstring("UTC"))
        + _pg_message(b"K", struct.pack("!ii", 1234, 5678))
        + _pg_ready()
    )


class _PgConnection:
    """One client: startup, then the simple and extended query protocols."""

    def __init__(self, sock):
        self.sock = sock
        self.buffer = b""
        self.statements = {}

    def read(self, count):
        while len(self.buffer) < count:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise EOFError
            self.buffer += chunk
        data, self.buffer = self.buffer[:count], self.buffer[count:]
        return data

    def serve(self):
        while True:
            length = struct.unpack("!I", self.read(4))[0]
            body = self.read(length - 4)
            code = struct.unpack("!I", body[:4])[0]
            if code == 80877103:
                # SSLRequest: refused, exactly as a server with no TLS answers.
                self.sock.sendall(b"N")
                continue
            if code == 196608:
                self.sock.sendall(_pg_startup())
                break
            raise EOFError(f"unknown startup packet {code}")
        while True:
            kind = self.read(1)
            length = struct.unpack("!I", self.read(4))[0]
            body = self.read(length - 4)
            if kind == b"Q":
                self.sock.sendall(_pg_run(body[:-1].decode()) + _pg_ready())
            elif kind == b"P":
                name, sql = [part.decode() for part in body.split(b"\0")[:2]]
                self.statements[name] = sql
                self.sock.sendall(_pg_message(b"1"))
            elif kind == b"B":
                self.sock.sendall(_pg_message(b"2"))
            elif kind == b"D":
                answer = self.statements.get(body[1:-1].decode(), "")
                selected = answer.strip().lower().startswith("select")
                rows = _pg_row_description() if selected else _pg_message(b"n")
                if body[:1] == b"S":
                    self.sock.sendall(_pg_message(b"t", struct.pack("!h", 0)) + rows)
                else:
                    self.sock.sendall(rows)
            elif kind == b"E":
                name = body.split(b"\0")[0].decode()
                self.sock.sendall(_pg_run(self.statements.get(name, "")))
            elif kind == b"S":
                self.sock.sendall(_pg_ready())
            elif kind == b"C":
                self.sock.sendall(_pg_message(b"3"))
            elif kind in (b"H", b"p"):
                continue
            elif kind == b"X":
                return
            else:
                raise EOFError(f"unknown frontend message {kind!r}")


class PgStub:
    """A deterministic PostgreSQL wire stub bound to one loopback port.

    The audited fixture app and `blastit-dbproxy` both need a real socket, so the
    row serves one: startup, simple queries and the extended protocol answer the
    way a server does, and every connection is served on its own daemon thread.
    """

    def __init__(self, host="127.0.0.1", port=0):
        self._listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._listener.bind((host, port))
        self.port = self._listener.getsockname()[1]
        self._listener.listen(64)
        self._closed = threading.Event()
        self._thread = threading.Thread(target=self._accept, daemon=True)

    def __enter__(self):
        self._thread.start()
        return self

    def __exit__(self, *_):
        self._closed.set()
        try:
            self._listener.close()
        except OSError:
            pass

    def _accept(self):
        while not self._closed.is_set():
            try:
                sock, _ = self._listener.accept()
            except OSError:
                return
            threading.Thread(target=self._serve, args=(sock,), daemon=True).start()

    def _serve(self, sock):
        try:
            _PgConnection(sock).serve()
        except (EOFError, OSError, ValueError):
            pass
        finally:
            try:
                sock.close()
            except OSError:
                pass


def project(root, files):
    """Writes one fixture project from `files` and returns its directory."""
    root.mkdir(parents=True, exist_ok=True)
    for relative, text in files.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    return root


# ---------------------------------------------------------------------------
# the mask (read from the one file the Rust self-tests assert is the design's)
# ---------------------------------------------------------------------------


def masked_fields(path=MASK):
    fields = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            fields.append(line)
    return fields


ETAG = re.compile(r'^"([0-9a-f]+)-([0-9a-f]+)"$')
PYTEST_SUMMARY = re.compile(r"^\d+ passed(?:, [^.]+)? in \d+(?:\.\d+)?s$")
ETAG_LINE = re.compile(r"^etag:\s*(.*)$", re.IGNORECASE)
CLOCK = re.compile(r"^\d\d:\d\d:\d\d\.\d\d\d ")
DURATION = re.compile(r"\d+(?:\.\d+)?(?:ms|ns|\u00b5s)")
PID = re.compile(r"pid=\d+")
# The host/run values the preload line prints alongside their unit.
HOST_KEYS = ("mem_used", "max_view_processes")
VERSION = re.compile(r"^blastit \d+\.\d+\.\d+")


class Failure(Exception):
    """A capture that cannot be normalised: never an empty diff."""


def normalise(text, fields):
    """The same rules as `crates/blastit/tests/dx_capture.rs`."""
    if not text.strip():
        raise Failure("the capture is empty; there is nothing to compare")
    try:
        document = json.loads(text)
    except json.JSONDecodeError:
        pass
    else:
        return (
            json.dumps(
                mask_json(document, fields), sort_keys=True, separators=(",", ":")
            )
            + "\n"
        )
    out = []
    for number, line in enumerate(text.splitlines(), start=1):
        stripped = line.lstrip()
        if stripped.startswith(("{", "[")):
            try:
                value = json.loads(stripped)
            except json.JSONDecodeError as error:
                raise Failure(
                    f"line {number} looks like JSON but does not parse: {error}"
                )
            out.append(
                json.dumps(
                    mask_json(value, fields), sort_keys=True, separators=(",", ":")
                )
            )
            continue
        match = ETAG_LINE.match(line)
        if match:
            value = match.group(1).strip()
            plain = value[2:] if value.startswith("W/") else value
            if not ETAG.match(plain):
                raise Failure(
                    f"line {number} is an ETag header whose value is not the documented "
                    f'"<hex>-<hex>" shape: {value}'
                )
            out.append('ETag: "<hex>-<hex>"')
            continue
        if VERSION.match(line):
            out.append("<version banner>")
            continue
        if PYTEST_SUMMARY.match(line.strip()):
            out.append("<pytest summary>")
            continue
        line = CLOCK.sub("<ts> ", line)
        line = PID.sub("pid=<masked>", line)
        for key in fields:
            line = re.sub(rf"\b{key}=[0-9][0-9.]*[A-Za-z]*", f"{key}=<masked>", line)
        out.append(DURATION.sub("<duration>", line))
    if len(out) != len(text.splitlines()):
        raise Failure("normalising lost a line")
    return "\n".join(sort_concurrent_exits(out)) + "\n"


def sort_concurrent_exits(lines):
    """Sorts each consecutive run of `process.exited` lines; the rest keeps order."""
    out, block = [], []
    for line in lines:
        if "process.exited" in line:
            block.append(line)
            continue
        if block:
            out.extend(sorted(block))
            block = []
        out.append(line)
    if block:
        out.extend(sorted(block))
    return out


def mask_json(value, fields):
    if isinstance(value, dict):
        return {
            key: "<masked>"
            if key in fields and item is not None
            else mask_json(item, fields)
            for key, item in value.items()
        }
    if isinstance(value, list):
        return [mask_json(item, fields) for item in value]
    return value


# ---------------------------------------------------------------------------
# processes and one HTTP/1.1 client
# ---------------------------------------------------------------------------


def clean_env(extra):
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": os.environ.get("HOME", ""),
        "BLASTIT_MEM_HIGH_WATERMARK": "100",
        "BLASTIT_MAX_VIEW_PROCESSES": "8",
    }
    for name in ("LD_LIBRARY_PATH", "PYTHONHOME"):
        if os.environ.get(name):
            env[name] = os.environ[name]
    env.update(extra)
    return env


def raw_http(port, method, target, body=b"", content_type=None, timeout=10):
    head = (
        f"{method} {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n"
        f"Content-Length: {len(body)}\r\n"
    )
    if content_type:
        head += f"Content-Type: {content_type}\r\n"
    with socket.create_connection(("127.0.0.1", port), timeout) as sock:
        sock.sendall(head.encode() + b"\r\n" + body)
        chunks = []
        while True:
            data = sock.recv(65536)
            if not data:
                break
            chunks.append(data)
    return b"".join(chunks)


def wait_for(port, target="/", deadline=60.0):
    end = time.monotonic() + deadline
    while time.monotonic() < end:
        try:
            response = raw_http(port, "GET", target, timeout=2)
            if b" 200 " in response.split(b"\r\n", 1)[0]:
                return response
        except OSError:
            pass
        time.sleep(0.05)
    raise Failure(f"nothing answered GET {target} on 127.0.0.1:{port} in {deadline}s")


def serve(binary, cwd, port, extra_env=None, flags=(), requests=()):
    """Boot `blastit dev`, issue `requests`, stop it, return the transcript.

    Readiness is the boot's own `INFO  ready` line, not a probe request: a probe
    would appear in the capture as a third `request` event and make row 2's "two
    requests, two events" oracle depend on the driver.
    """
    argv = [str(binary), "dev", "--port", str(port), *flags]
    process = subprocess.Popen(
        argv,
        cwd=cwd,
        env=clean_env(extra_env or {}),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    stdout_lines, stderr_lines = [], []
    ready = threading.Event()

    def pump(stream, sink, mark):
        for line in stream:
            sink.append(line)
            if mark and "INFO  ready" in line:
                ready.set()

    pumps = [
        threading.Thread(
            target=pump, args=(process.stdout, stdout_lines, True), daemon=True
        ),
        threading.Thread(
            target=pump, args=(process.stderr, stderr_lines, False), daemon=True
        ),
    ]
    for thread in pumps:
        thread.start()
    transcript = []
    try:
        if not ready.wait(60):
            raise Failure(
                "`blastit dev` never printed its ready line: " + "".join(stderr_lines)
            )
        for request in requests:
            transcript.append(raw_http(port, *request).decode("utf-8", "replace"))
    finally:
        process.terminate()
        try:
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        for thread in pumps:
            thread.join(timeout=5)
    # The capture ends where the documented interaction ends: the shutdown block
    # is emitted by children that exit concurrently, so its order and its
    # `status=`/`signal=` detail race. Dropping it is deliberate and named in the
    # audit, not a silent loss: everything a request can observe is before it.
    stdout = "".join(stdout_lines)
    marker = stdout.find("INFO  shutdown")
    if marker != -1:
        stdout = stdout[:marker]
    return stdout, "".join(stderr_lines), "\n".join(transcript)


def finish(binary, cwd, args, env=None, timeout=300):
    return subprocess.run(
        [str(binary), *args],
        cwd=cwd,
        env=clean_env(env or {}),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=timeout,
    )


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


# ---------------------------------------------------------------------------
# the seven rows
# ---------------------------------------------------------------------------


def fixtures(run):
    static = run / "fixtures/static"
    (static / "public").mkdir(parents=True, exist_ok=True)
    write(static / "Blastfile", STATIC_MANIFEST)
    (static / "public/index.html").write_bytes(STATIC_INDEX)

    php = run / "fixtures/php"
    shutil.copytree(ROOT / "e2e/php/fixture", php, dirs_exist_ok=True)

    wsgi = run / "fixtures/wsgi"
    wsgi.mkdir(parents=True, exist_ok=True)
    write(wsgi / "Blastfile", WSGI_MANIFEST)
    write(wsgi / "app.py", WSGI_APP)
    # A fixture file's mtime is `Last-Modified` and half of the `ETag`; pinning it
    # turns a clock into a value both captures can compare (the ETag *value* is
    # still compared by shape only, as the design says).
    for path in run.rglob("*"):
        if path.is_file():
            os.utime(path, (1700000000, 1700000000))
    return static, php, wsgi


def shim_dir():
    explicit = os.environ.get("BLASTIT_PHP_SHIM_DIR")
    if explicit:
        return explicit
    candidate = ROOT / "target/php-sapi"
    if candidate.is_dir() and any(candidate.glob("libblastit_php*.so")):
        return str(candidate)
    return None


def row_static(binary, run, out):
    static, _, _ = fixtures(run)
    requests = (
        ("GET", "/index.html", b"", None),
        ("GET", "/missing", b"", None),
        ("GET", "/..%2fetc", b"", None),
    )
    stdout, stderr, transcript = serve(
        binary, static, PORTS["static"], requests=requests
    )
    write(out / "row1.stdout", stdout)
    write(out / "row1.stderr", stderr)
    write(out / "row1.http", transcript)
    if (
        "200 OK" not in transcript
        or "404 Not Found" not in transcript
        or "400 Bad Request" not in transcript
    ):
        raise Failure("row 1 did not show 200, 404 and 400")


def row_php(binary, run, out):
    _, php, _ = fixtures(run)
    requests = (("GET", "/", b"", None), ("GET", "/", b"", None))
    shims = shim_dir()
    env = {"BLASTIT_PHP_SHIM_DIR": shims} if shims else {}
    stdout, stderr, transcript = serve(
        binary, php, PORTS["php"], env, requests=requests
    )
    write(out / "row2.stdout", stdout)
    write(out / "row2.stderr", stderr)
    write(out / "row2.http", transcript)
    events = re.findall(r"INFO  request\s+GET / → \S+  200 ", stdout)
    if transcript.count("200 OK") != 2 or len(events) != 2:
        raise Failure(
            "row 2 must show two 200s and two request events with the same status: "
            f"{transcript.count('200 OK')} responses, {len(events)} events"
        )


def row_wsgi(binary, run, out):
    _, _, wsgi = fixtures(run)
    body = b'{"n":1}'
    requests = (
        ("GET", "/", b"", None),
        ("POST", "/echo", body, "application/json"),
        ("POST", "/bare", body, "application/json"),
    )
    stdout, stderr, transcript = serve(binary, wsgi, PORTS["wsgi"], requests=requests)
    write(out / "row3.stdout", stdout)
    write(out / "row3.stderr", stderr)
    write(out / "row3.http", transcript)
    if "wsgi-fixture" not in transcript or "413 Content Too Large" not in transcript:
        raise Failure("row 3 did not show the app's body and the 413")


def row_no_reload(binary, run, out):
    static, _, _ = fixtures(run)
    stdout, stderr, transcript = serve(
        binary,
        static,
        PORTS["no-reload"],
        flags=("--no-reload",),
        requests=(("GET", "/index.html", b"", None),),
    )
    write(out / "row4.stdout", stdout)
    write(out / "row4.stderr", stderr)
    write(out / "row4.http", transcript)
    if "reload: on" in stdout or "dev.reload" in stdout:
        raise Failure("row 4: `--no-reload` still announced a reload")


def row_check(binary, run, out, python):
    """Row 5: the complete fixture, then one missing a grant."""
    fixtures = {
        "shop": project(run / "fixtures/shop", SHOP_FILES),
        "findings": project(run / "fixtures/findings", FINDINGS_FILES),
    }
    env = {"PYTHONPATH": str(ROOT / "python")}
    if python:
        env["BLASTIT_PYTHON"] = str(python)
    with PgStub() as stub:
        url = f"postgresql://shop:secret@127.0.0.1:{stub.port}/shop"
        env = {**env, "DATABASE_URL": url, "ANALYTICS_URL": url}
        for name, expected in (("shop", 0), ("findings", CHECK_EXIT_FINDINGS)):
            fixture = fixtures[name]
            for suffix, args in (("", ["check"]), ("-json", ["check", "--json"])):
                completed = finish(binary, fixture, args, env)
                write(out / f"row5-{name}{suffix}.stdout", completed.stdout)
                write(out / f"row5-{name}{suffix}.stderr", completed.stderr)
                write(out / f"row5-{name}{suffix}.exit", f"{completed.returncode}\n")
                if completed.returncode != expected:
                    raise Failure(
                        f"row 5: `blastit check` on {name} exited "
                        f"{completed.returncode}, expected {expected}: "
                        f"{completed.stderr.strip() or completed.stdout.strip()}"
                    )


def row_libphp_refusal(binary, run, out):
    _, php, _ = fixtures(run)
    completed = finish(
        binary,
        php,
        ["dev", "--port", str(PORTS["php"] + 10)],
        {"BLASTIT_LIBPHP": "/dev/null"},
    )
    write(out / "row6.stdout", completed.stdout)
    write(out / "row6.stderr", completed.stderr)
    write(out / "row6.exit", f"{completed.returncode}\n")
    sentence = completed.stdout + completed.stderr
    if completed.returncode != EXIT_FAILED:
        raise Failure(f"row 6: exited {completed.returncode}, expected 1")
    for word in ("BLASTIT_LIBPHP", "entry", "libphp"):
        if word not in sentence:
            raise Failure(f"row 6: the refusal does not name {word!r}: {sentence}")


def row_no_interpreter(binary, run, out):
    _, _, wsgi = fixtures(run)
    completed = finish(binary, wsgi, ["dev", "--port", str(PORTS["wsgi"] + 10)])
    write(out / "row7.stdout", completed.stdout)
    write(out / "row7.stderr", completed.stderr)
    write(out / "row7.exit", f"{completed.returncode}\n")
    sentence = completed.stdout + completed.stderr
    if completed.returncode != EXIT_FAILED:
        raise Failure(f"row 7: exited {completed.returncode}, expected 1")
    if "uv add blastit" not in sentence:
        raise Failure(f"row 7: the refusal does not name `uv add blastit`: {sentence}")


def run_all(args):
    run = Path(args.run_root)
    out = Path(args.out)
    if run.exists():
        shutil.rmtree(run)
    run.mkdir(parents=True)
    out.mkdir(parents=True, exist_ok=True)
    binary = Path(args.binary).resolve()
    rows = args.rows.split(",")
    handlers = {
        "1": lambda: row_static(binary, run, out),
        "2": lambda: row_php(binary, run, out),
        "3": lambda: row_wsgi(binary, run, out),
        "4": lambda: row_no_reload(binary, run, out),
        "5": lambda: row_check(binary, run, out, args.python),
        "6": lambda: row_libphp_refusal(binary, run, out),
        "7": lambda: row_no_interpreter(
            Path(args.binary_no_default).resolve(), run, out
        ),
    }
    for row in rows:
        handlers[row]()
    normalise_captures(out, Path(args.mask), Path(args.mask_host))
    print(f"captured rows {args.rows} into {out}")


def normalise_captures(out, mask, host_mask=HOST_MASK):
    fields = masked_fields(mask) + masked_fields(host_mask)
    for path in sorted(out.iterdir()):
        if path.suffix in (".http", ".stdout", ".stderr"):
            text = path.read_text()
            if not text:
                # `normalise` refuses an empty capture, and so does the Rust
                # normaliser (`CaptureError::Empty`): an empty stdout or HTTP
                # transcript means the command observed nothing at all, which is
                # a driver bug, not a comparable capture. An empty *stderr* is a
                # real observation (the run wrote no diagnostics), and is
                # recorded by the one sentinel both trees then compare.
                if path.suffix == ".http":
                    raise Failure(
                        f"{path.name} is empty: a row that made no request cannot be compared"
                    )
                if path.suffix == ".stdout":
                    # `docs/ci.md`: "an empty stdout is a refusal, never a clean
                    # audit". A successful run that printed nothing is a driver
                    # bug; a refusal (the sibling `.exit` is non-zero) is the
                    # documented shape and is compared as `<empty>`.
                    exit_file = path.with_suffix(".exit")
                    code = exit_file.read_text().strip() if exit_file.exists() else ""
                    if code in ("", "0"):
                        raise Failure(
                            f"{path.name} is empty but its exit status is {code or '0'}: "
                            "a successful run that printed nothing cannot be compared"
                        )
                path.with_suffix(path.suffix + ".normalised").write_text("<empty>\n")
                print(f"normalised {path.name}: <empty>")
                continue
            normalised = normalise(text, fields)
            path.with_suffix(path.suffix + ".normalised").write_text(normalised)
            print(f"normalised {path.name}: {len(normalised.splitlines())} lines")


def self_test(vectors=ROOT / "crates/blastit/tests", mask=MASK, host_mask=HOST_MASK):
    """Normalise the golden capture the Rust suite also asserts.

    This file is a *second* implementation of the mask, so the invariance claim
    is only as strong as their agreement: one vector, two readers, and a
    divergence is red here.
    """
    fields = masked_fields(mask) + masked_fields(host_mask)
    got = normalise((vectors / "dx-parity.txt").read_text(), fields)
    expected = (vectors / "dx-parity.expected").read_text()
    if got != expected:
        raise Failure(
            "the normaliser diverges from crates/blastit/tests/dx_capture.rs on "
            f"dx-parity.txt:\n--- expected\n{expected}--- got\n{got}"
        )
    try:
        normalise("", fields)
    except Failure:
        pass
    else:
        raise Failure(
            "an empty capture must be refused, as `CaptureError::Empty` refuses it"
        )
    print(
        "dx-replay self-test: parity with dx_capture.rs on dx-parity.txt, empty refused"
    )


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run", help="capture the seven rows from one tree")
    run.add_argument("--binary", required=True)
    run.add_argument("--binary-no-default", default="")
    run.add_argument("--python", default="")
    run.add_argument("--out", required=True)
    run.add_argument(
        "--run-root", default=str(Path(tempfile.gettempdir()) / "blastit-dx-run")
    )
    run.add_argument("--mask", default=str(MASK))
    run.add_argument("--mask-host", default=str(HOST_MASK))
    run.add_argument("--rows", default="1,2,3,4,5,6,7")
    normalise = commands.add_parser(
        "normalise", help="normalise captures another driver wrote"
    )
    normalise.add_argument("--out", required=True)
    normalise.add_argument("--mask", default=str(MASK))
    normalise.add_argument("--mask-host", default=str(HOST_MASK))
    parity = commands.add_parser(
        "self-test", help="prove parity with crates/blastit/tests/dx_capture.rs"
    )
    parity.add_argument("--mask", default=str(MASK))
    parity.add_argument("--mask-host", default=str(HOST_MASK))
    args = parser.parse_args(argv)
    try:
        if args.command == "run":
            if "7" in args.rows.split(",") and not args.binary_no_default:
                raise Failure("row 7 needs --binary-no-default")
            run_all(args)
        elif args.command == "normalise":
            normalise_captures(Path(args.out), Path(args.mask), Path(args.mask_host))
        else:
            self_test(mask=Path(args.mask), host_mask=Path(args.mask_host))
    except Failure as error:
        print(f"dx-replay: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
