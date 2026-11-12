"""The fixture app's PostgreSQL client: the wire protocol, not a driver.

`socket` is in the standard library, so the fixture needs no third-party
package; `blastit` hands it a local URL and every byte still crosses the proxy.
"""

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


def _until_ready(sock):
    while True:
        kind = _read(sock, 1)
        length = struct.unpack("!I", _read(sock, 4))[0]
        body = _read(sock, length - 4)
        if kind == b"E":
            raise RuntimeError(body)
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
    params = b"user\0blastit\0database\0" + database.encode() + b"\0\0"
    sock.sendall(
        struct.pack("!I", len(params) + 8) + struct.pack("!I", 196608) + params
    )
    _until_ready(sock)
    return sock


def simple(statements):
    """Send statements with the simple query protocol, one connection."""
    sock = _connect()
    try:
        for statement in statements:
            sock.sendall(_message(b"Q", statement.encode() + b"\0"))
            _until_ready(sock)
    finally:
        sock.close()


def extended(statement):
    """Send one statement with Parse/Bind/Describe/Execute/Sync."""
    sock = _connect()
    try:
        sock.sendall(
            _message(b"P", b"\0" + statement.encode() + b"\0" + struct.pack("!h", 0))
            + _message(b"B", b"\0\0" + struct.pack("!h", 0) + struct.pack("!h", 0))
            + _message(b"D", b"P\0")
            + _message(b"E", b"\0" + struct.pack("!i", 0))
            + _message(b"S")
        )
        _until_ready(sock)
    finally:
        sock.close()
