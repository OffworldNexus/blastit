"""A deterministic PostgreSQL wire stub for the runnable-docs harness.

The documentation's `--learn` and `blastit check` snippets need a server behind
`[proxy.db]`. Docker is not available (and would not be deterministic), so this
module speaks enough of the PostgreSQL v3 protocol for the fixture app's raw
client and for `blastit-dbproxy`'s upstream connection:

* startup: SSLRequest is refused (`N`), AuthenticationOk (trust), the
  ParameterStatus set libpq needs, BackendKeyData, ReadyForQuery;
* simple queries (`Q`): a `SELECT` answers one int4 column with `1`; `BEGIN`,
  `COMMIT`, `ROLLBACK` and `SET` answer their command tag;
* the extended protocol (`Parse`/`Bind`/`Describe`/`Execute`/`Sync`), so a
  client that binds server-side still works.

Every accepted connection is served on its own daemon thread. The stub is a test
double for a third-party server, exactly like a fake clock: the SQL policy, the
proxy and the audit all run for real in front of it.
"""

from __future__ import annotations

import socket
import struct
import threading
from dataclasses import dataclass, field


def _cstring(text: str) -> bytes:
    return text.encode() + b"\0"


def _message(kind: bytes, payload: bytes = b"") -> bytes:
    return kind + struct.pack("!I", len(payload) + 4) + payload


def _row_description(name: str = "?column?", oid: int = 23) -> bytes:
    body = (
        struct.pack("!h", 1)
        + _cstring(name)
        + struct.pack("!ihihih", 0, 0, oid, -1, -1, 0)
    )
    return _message(b"T", body)


def _data_row(value: bytes = b"1") -> bytes:
    return _message(b"D", struct.pack("!h", 1) + struct.pack("!i", len(value)) + value)


def _command_complete(tag: str) -> bytes:
    return _message(b"C", _cstring(tag))


def _ready() -> bytes:
    return _message(b"Z", b"I")


def _run_sql(sql: str) -> bytes:
    head = sql.strip().lower()
    if head.startswith("select"):
        return _row_description() + _data_row() + _command_complete("SELECT 1")
    if head.startswith("begin"):
        return _command_complete("BEGIN")
    if head.startswith(("commit", "end")):
        return _command_complete("COMMIT")
    if head.startswith("rollback"):
        return _command_complete("ROLLBACK")
    if head.startswith("set"):
        return _command_complete("SET")
    return _command_complete("OK")


@dataclass
class _Connection:
    sock: socket.socket
    buffer: bytes = b""
    statements: dict[str, str] = field(default_factory=dict)

    def read(self, count: int) -> bytes:
        while len(self.buffer) < count:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise EOFError
            self.buffer += chunk
        data, self.buffer = self.buffer[:count], self.buffer[count:]
        return data

    def serve(self) -> None:
        while True:
            length = struct.unpack("!I", self.read(4))[0]
            body = self.read(length - 4)
            code = struct.unpack("!I", body[:4])[0]
            if code == 80877103:
                self.sock.sendall(b"N")
                continue
            if code == 196608:
                self.sock.sendall(_startup())
                break
            raise EOFError(f"unknown startup packet {code}")
        while True:
            kind = self.read(1)
            length = struct.unpack("!I", self.read(4))[0]
            body = self.read(length - 4)
            if kind == b"Q":
                self.sock.sendall(_run_sql(body[:-1].decode()) + _ready())
            elif kind == b"P":
                name, sql = [part.decode() for part in body.split(b"\0")[:2]]
                self.statements[name] = sql
                self.sock.sendall(_message(b"1"))
            elif kind == b"B":
                self.sock.sendall(_message(b"2"))
            elif kind == b"D":
                kind_byte = body[:1]
                name = body[1:-1].decode()
                answer = self.statements.get(name, "")
                if kind_byte == b"S":
                    self.sock.sendall(
                        _message(b"t", struct.pack("!h", 0))
                        + (
                            _row_description()
                            if answer.strip().lower().startswith("select")
                            else _message(b"n")
                        )
                    )
                else:
                    self.sock.sendall(
                        _row_description()
                        if answer.strip().lower().startswith("select")
                        else _message(b"n")
                    )
            elif kind == b"E":
                name = body.split(b"\0")[0].decode()
                self.sock.sendall(_run_sql(self.statements.get(name, "")))
            elif kind == b"S":
                self.sock.sendall(_ready())
            elif kind == b"C":
                self.sock.sendall(_message(b"3"))
            elif kind in (b"H", b"p"):
                continue
            elif kind == b"X":
                return
            else:
                raise EOFError(f"unknown frontend message {kind!r}")


def _startup() -> bytes:
    return (
        _message(b"R", struct.pack("!i", 0))
        + _message(b"S", _cstring("server_version") + _cstring("16.0"))
        + _message(b"S", _cstring("client_encoding") + _cstring("UTF8"))
        + _message(b"S", _cstring("standard_conforming_strings") + _cstring("on"))
        + _message(b"S", _cstring("DateStyle") + _cstring("ISO, MDY"))
        + _message(b"S", _cstring("integer_datetimes") + _cstring("on"))
        + _message(b"S", _cstring("TimeZone") + _cstring("UTC"))
        + _message(b"K", struct.pack("!ii", 1234, 5678))
        + _ready()
    )


class PgStub:
    """A PostgreSQL wire stub bound to one loopback address."""

    def __init__(self, host: str = "127.0.0.1", port: int = 5432) -> None:
        self.host = host
        self.port = port
        self._listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._listener.bind((host, port))
        self.port = self._listener.getsockname()[1]
        self._listener.listen(64)
        self._thread = threading.Thread(target=self._accept, daemon=True)
        self._closed = threading.Event()

    def start(self) -> None:
        self._thread.start()

    def _accept(self) -> None:
        while not self._closed.is_set():
            try:
                sock, _ = self._listener.accept()
            except OSError:
                return
            threading.Thread(target=self._serve, args=(sock,), daemon=True).start()

    def _serve(self, sock: socket.socket) -> None:
        connection = _Connection(sock)
        try:
            connection.serve()
        except (EOFError, OSError, ValueError):
            pass
        finally:
            try:
                sock.close()
            except OSError:
                pass

    def ping(self, user: str = "shop", database: str = "shop") -> None:
        """Prove the stub answers a startup and a `SELECT 1` like a server."""
        with socket.create_connection((self.host, self.port), timeout=5) as sock:
            params = (
                b"user\0"
                + user.encode()
                + b"\0database\0"
                + database.encode()
                + b"\0\0"
            )
            sock.sendall(
                struct.pack("!I", len(params) + 8) + struct.pack("!I", 196608) + params
            )
            _read_until_ready(sock)
            sock.sendall(b"Q" + struct.pack("!I", 13) + b"SELECT 1\0")
            _read_until_ready(sock)

    def stop(self) -> None:
        self._closed.set()
        try:
            self._listener.close()
        except OSError:
            pass


def _read_until_ready(sock: socket.socket) -> None:
    while True:
        header = _read_exact(sock, 5)
        kind = header[:1]
        length = struct.unpack("!I", header[1:])[0]
        body = _read_exact(sock, length - 4)
        if kind == b"E":
            raise AssertionError(body)
        if kind == b"Z":
            return


def _read_exact(sock: socket.socket, count: int) -> bytes:
    data = b""
    while len(data) < count:
        chunk = sock.recv(count - len(data))
        if not chunk:
            raise EOFError
        data += chunk
    return data
