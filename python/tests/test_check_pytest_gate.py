"""`blastit check`'s WSGI gate over a real socket, with and without a proxy.

`python/blastit/_check_pytest.py` is the one place a `blastit check` run learns
which request the statements that follow belong to. It talks to dbproxy over an
inherited `AF_UNIX` descriptor, so its tests use a real
:func:`socket.socketpair` — never a mock server: what is asserted is the bytes
on the wire and the observation file the orchestrator reads.

Two shapes are covered, because a store-less application declares no `postgres`
proxy at all (`MB.40.2`):

* **no proxy** — no ``BLASTIT_CHECK_CTL_FD`` is handed over: the request is
  recorded and nothing is pushed, and the gate must not raise;
* **a proxy** — the descriptor is a real socketpair, and the gate pushes
  ``push_unrouted`` before the application runs and ``pop`` once the body is
  exhausted, waiting for the proxy's ``{"ok": true}`` reply to each.

The adversarial half is the dead channel: a proxy that has closed its end is
exit `3` (`ControlError`), never a request silently attributed to nobody.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import socket
import tempfile
import threading
import unittest
from pathlib import Path

from blastit import _check_pytest


def _wsgi_app(environ: dict, start_response) -> list[bytes]:
    """A one-shot body, the shape a hello-world WSGI app returns."""
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"hello\n"]


def _environ(path: str, body: bytes = b"") -> dict:
    return {
        "REQUEST_METHOD": "GET",
        "PATH_INFO": path,
        "CONTENT_LENGTH": str(len(body)) if body else "",
        "CONTENT_TYPE": "text/plain" if body else "",
        "wsgi.input": io.BytesIO(body),
    }


class GateTest(unittest.TestCase):
    """The gate, driven over the descriptors a `blastit check` run hands it."""

    def setUp(self) -> None:
        # The channel is cached at module scope (`_CONTROL`) so one session
        # opens it once; every test starts from "not opened yet".
        _check_pytest._CONTROL.clear()
        self._directory = tempfile.TemporaryDirectory()
        self.addCleanup(self._directory.cleanup)
        self.observation = Path(self._directory.name) / "http_obs.jsonl"
        self._saved = {
            name: os.environ.get(name)
            for name in (_check_pytest.HTTP_OBS, _check_pytest.CTL_FD)
        }
        self.addCleanup(self._restore)

    def _restore(self) -> None:
        for name, value in self._saved.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value
        # The gate adopts the descriptor it was handed (`socket.socket(fileno=…)`)
        # and owns it from then on; each test hands it its own `os.dup`, so this
        # close can never race the test's own end. `suppress` keeps a run whose
        # channel already died from failing here instead of at the assertion.
        for channel in _check_pytest._CONTROL:
            with contextlib.suppress(OSError):
                channel.close()
        _check_pytest._CONTROL.clear()

    def _lines(self) -> list[dict]:
        if not self.observation.exists():
            return []
        return [
            json.loads(line)
            for line in self.observation.read_text(encoding="utf-8").splitlines()
            if line
        ]

    def test_a_store_less_run_records_the_request_and_pushes_nothing(self) -> None:
        # Allowed: no proxy, so no descriptor; the observation still exists.
        os.environ[_check_pytest.HTTP_OBS] = str(self.observation)
        os.environ.pop(_check_pytest.CTL_FD, None)

        gated = _check_pytest.wrap(_wsgi_app)
        body = gated(_environ("/hello/"), lambda status, headers: None)
        self.assertEqual(list(body), [b"hello\n"])

        observed = self._lines()
        self.assertEqual(len(observed), 1)
        self.assertEqual(observed[0]["method"], "GET")
        self.assertEqual(observed[0]["path"], "/hello/")
        self.assertEqual(observed[0]["body_len"], 0)

    def test_a_proxied_run_pushes_the_request_and_pops_it_on_the_channel(self) -> None:
        # Allowed: push_unrouted before the app runs, pop after the body. The
        # gate answers the proxy synchronously, so the proxy is driven from this
        # thread while the request runs in another — exactly the two processes a
        # real run has.
        os.environ[_check_pytest.HTTP_OBS] = str(self.observation)
        proxy_end, suite_end = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(proxy_end.close)
        os.environ[_check_pytest.CTL_FD] = str(os.dup(suite_end.fileno()))

        bodies: dict[str, list[bytes]] = {}

        def run() -> None:
            gated = _check_pytest.wrap(_wsgi_app)
            bodies["body"] = list(
                gated(_environ("/notes/7/"), lambda status, headers: None)
            )

        request = threading.Thread(target=run, daemon=True)
        request.start()

        pushed = json.loads(self._read(proxy_end))
        self.assertEqual(
            pushed, {"op": "push_unrouted", "method": "GET", "path": "/notes/7/"}
        )
        proxy_end.sendall(b'{"ok": true}\n')
        self.assertEqual(json.loads(self._read(proxy_end)), {"op": "pop"})
        proxy_end.sendall(b'{"ok": true}\n')
        request.join(timeout=5)
        self.assertFalse(request.is_alive(), "the gate never finished the request")
        self.assertEqual(bodies["body"], [b"hello\n"])
        self.assertEqual(self._lines()[0]["path"], "/notes/7/")
        suite_end.close()

    def test_a_refusing_proxy_is_a_control_error(self) -> None:
        # Denied twin: the proxy answers `{"ok": false}`, and the run stops
        # rather than attribute statements to a request nobody accepted.
        os.environ[_check_pytest.HTTP_OBS] = str(self.observation)
        proxy_end, suite_end = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(proxy_end.close)
        os.environ[_check_pytest.CTL_FD] = str(os.dup(suite_end.fileno()))

        # Nothing reads the reply inside this test: answer it from a thread.
        def answer() -> None:
            self._read(proxy_end)
            proxy_end.sendall(b'{"ok": false, "error": "no"}\n')

        responder = threading.Thread(target=answer, daemon=True)
        responder.start()
        gated = _check_pytest.wrap(_wsgi_app)
        with self.assertRaises(_check_pytest.ControlError):
            gated(_environ("/notes/"), lambda status, headers: None)
        responder.join(timeout=5)
        suite_end.close()

    def test_a_closed_channel_is_a_control_error(self) -> None:
        # Denied twin: dbproxy is gone, so no request can be attributed.
        os.environ[_check_pytest.HTTP_OBS] = str(self.observation)
        proxy_end, suite_end = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
        os.environ[_check_pytest.CTL_FD] = str(os.dup(suite_end.fileno()))
        proxy_end.close()
        gated = _check_pytest.wrap(_wsgi_app)
        with self.assertRaises(_check_pytest.ControlError):
            list(gated(_environ("/notes/"), lambda status, headers: None))
        suite_end.close()

    def test_a_body_is_recorded_and_replayed_unchanged(self) -> None:
        # Allowed: the bounded probe never replaces the bytes the app reads.
        os.environ[_check_pytest.HTTP_OBS] = str(self.observation)
        os.environ.pop(_check_pytest.CTL_FD, None)
        seen: list[bytes] = []

        def app(environ: dict, start_response) -> list[bytes]:
            seen.append(environ["wsgi.input"].read())
            start_response("200 OK", [])
            return [b"ok"]

        gated = _check_pytest.wrap(app)
        environ = _environ("/notes/", body=b'{"name": "x"}')
        list(gated(environ, lambda status, headers: None))
        self.assertEqual(seen, [b'{"name": "x"}'])
        observed = self._lines()
        self.assertEqual(observed[0]["body_len"], len(b'{"name": "x"}'))
        self.assertEqual(observed[0]["content_type"], "text/plain")

    @staticmethod
    def _read(channel: socket.socket) -> str:
        """Reads one newline-terminated line from the proxy's end."""
        buffered = b""
        while not buffered.endswith(b"\n"):
            chunk = channel.recv(4096)
            if not chunk:
                raise AssertionError("the gate closed the channel")
            buffered += chunk
        return buffered.decode("utf-8").strip()


if __name__ == "__main__":
    unittest.main()
