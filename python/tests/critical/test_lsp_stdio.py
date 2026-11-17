"""Protected critical tier: `blastit lsp` on stdin/stdout, both distribution paths (R57).

Zed spawns `blastit lsp` one of two ways, and a release that breaks either one
silently removes the editor integration while the CLI keeps working: the
standalone binary (`make rs-no-python` -> `target/debug/blastit lsp`, no Python
linked) and the wheel's console script (`sys.executable -m blastit lsp`, which
reaches `_blastit.main(["lsp"], …)`). This file drives the **same** session
through both.

* By default it spawns `sys.executable -m blastit lsp`; `make test-critical`
  stages `target/python-critical` on `PYTHONPATH`, which is where the built
  `_blastit` extension and the `blastit/_bin/blastit-dbproxy` that
  `_blastit.main` stats live. A future reader must not "simplify" that away:
  the console script refuses before dispatch without the packaged binary, even
  though the LSP never uses it.
* When `BLASTIT_LSP_COMMAND` is set (a shell-split string), it drives that
  command instead, so the standalone binary is proved by the same suite:
  `BLASTIT_LSP_COMMAND="$PWD/target/debug/blastit lsp" …`.

One document, one `uri`, four texts: the broken buffer is exactly one
diagnostic at the published UTF-16 position and formatting is **declined**
(`null`); the canonical buffer is `[]` diagnostics and `[]` edits; the
two-space buffer is one whole-document `TextEdit`; and a `query "…"` buffer
proves the session never writes the document's SQL to stderr. `shutdown` then
`exit` is status `0`; a second session that sees EOF before `shutdown` is
status `1`.

The suite speaks the protocol with `subprocess` pipes and a reader thread, and
every read has a deadline: a server that stops answering fails the test instead
of hanging the critical tier.
"""

from __future__ import annotations

import json
import os
import queue
import shlex
import subprocess
import sys
import threading
import time
import unittest
from typing import Any

#: How long one frame or one process may take. The critical tier runs on a
#: loaded machine and the console script imports an interpreter, so this is
#: generous; a hung server still fails rather than hanging the suite.
PATIENCE = 60.0

URI = "file:///tmp/Blastfile"

#: `# ✓ ok` puts the error on 0-based line 2; `✓` is three UTF-8 bytes but one
#: UTF-16 unit, so the 1-based column 8 is UTF-16 `character` 7.
BROKEN = '# ✓ ok\nview v "/x" {\n  body =\n}\n'

#: The sentence `blastit check` prints for that buffer, with its
#: `Blastfile:<line>:<col>: ` prefix removed (design §5.2, §12.5).
EQUALS = "`=` is not Blastfile syntax; write the value after the keyword: `body json`"

#: `blastit fmt`'s output: two spaces become four.
CANONICAL = 'app "shop"\n\nview v "/x" {\n    body json\n}\n'
MISINDENTED = 'app "shop"\n\nview v "/x" {\n  body json\n}\n'

#: A document whose diagnostic comes from a `query "…"` literal. The SQL text
#: (and `s3cr3t`) must never reach stderr: a `Blastfile` is policy, and an
#: editor's log is still a log.
SQL = (
    "postgres $DATABASE_URL\n\n"
    'view v "/x" {\n'
    "    postgres {\n"
    "        query \"COPY s3cr3t FROM PROGRAM 'sh'\"\n"
    "    }\n"
    "}\n"
)
SQL_SECRETS = ("s3cr3t", "COPY", "PROGRAM")


def command() -> list[str]:
    """The server command this run proves: the pinned path or the console script."""
    configured = os.environ.get("BLASTIT_LSP_COMMAND", "").strip()
    if configured:
        return shlex.split(configured)
    return [sys.executable, "-m", "blastit", "lsp"]


def frame(message: Any) -> bytes:
    """One LSP frame: `Content-Length` in UTF-8 bytes, then the JSON body."""
    body = json.dumps(message, separators=(",", ":")).encode("utf-8")
    return b"Content-Length: %d\r\n\r\n" % len(body) + body


class _End:
    """The reader thread saw the server close stdout."""


END = _End()


class Session:
    """One server process, its framed stdout, and the deadlines around both."""

    def __init__(self) -> None:
        self.process = subprocess.Popen(
            command(),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self.messages: queue.Queue[Any] = queue.Queue()
        self.failure: Exception | None = None
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _read_exact(self, count: int) -> bytes:
        """Reads exactly `count` bytes of a frame body."""
        chunks = bytearray()
        while len(chunks) < count:
            chunk = self.process.stdout.read(count - len(chunks))
            if not chunk:
                raise AssertionError("the server closed stdout mid-frame")
            chunks.extend(chunk)
        return bytes(chunks)

    def _read_frame(self) -> dict[str, Any] | None:
        """One framed JSON message, or `None` at the end of the stream."""
        line = self.process.stdout.readline()
        if not line:
            return None
        declared = None
        while line not in (b"\r\n", b"\n"):
            if not line:
                raise AssertionError("the server closed stdout mid-header")
            name, _, value = line.partition(b":")
            if name.strip().lower() == b"content-length":
                declared = int(value.strip())
            line = self.process.stdout.readline()
        if declared is None:
            raise AssertionError("a frame carried no Content-Length header")
        return json.loads(self._read_exact(declared).decode("utf-8"))

    def _read(self) -> None:
        """Pumps stdout into the queue until the process ends."""
        try:
            while True:
                message = self._read_frame()
                if message is None:
                    return
                self.messages.put(message)
        except Exception as error:  # noqa: BLE001 - reported by `receive`
            self.failure = error
        finally:
            self.messages.put(END)

    def send(self, message: Any) -> None:
        """Writes one frame; the pipe is the only channel."""
        if self.process.stdin is None:
            raise AssertionError("the server's stdin is closed")
        self.process.stdin.write(frame(message))
        self.process.stdin.flush()

    def receive(self, timeout: float = PATIENCE) -> dict[str, Any]:
        """The next server message, or a test failure naming the deadline."""
        try:
            item = self.messages.get(timeout=timeout)
        except queue.Empty:
            self.kill()
            raise AssertionError(
                f"the server sent no message within {timeout:.0f}s "
                f"(command: {command()!r})"
            ) from None
        if item is END:
            if self.failure is not None:
                raise AssertionError(
                    f"the server's stdout is not framed JSON: {self.failure}"
                ) from self.failure
            raise AssertionError("the server closed stdout before the session ended")
        return item

    def response(self, identifier: Any) -> dict[str, Any]:
        """The reply carrying `identifier`, skipping notifications."""
        deadline = time.monotonic() + PATIENCE
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                self.kill()
                raise AssertionError(f"no reply to id {identifier!r}")
            message = self.receive(remaining)
            if message.get("id") == identifier:
                return message

    def publish(self, uri: str = URI) -> list[dict[str, Any]]:
        """The next `publishDiagnostics` set for `uri`."""
        deadline = time.monotonic() + PATIENCE
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                self.kill()
                raise AssertionError(f"no publishDiagnostics for {uri}")
            message = self.receive(remaining)
            params = message.get("params")
            if (
                message.get("method") == "textDocument/publishDiagnostics"
                and isinstance(params, dict)
                and params.get("uri") == uri
            ):
                return params.get("diagnostics", [])

    def handshake(self) -> dict[str, Any]:
        """`initialize` + `initialized`, returning the capability document."""
        self.send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
        result = self.response(1).get("result", {})
        self.send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
        return result

    def open(self, text: str, version: int = 0) -> list[dict[str, Any]]:
        """`didOpen` one buffer and returns its first published diagnostic set."""
        self.send(
            {
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": URI,
                        "languageId": "blastfile",
                        "version": version,
                        "text": text,
                    }
                },
            }
        )
        return self.publish()

    def change(self, text: str, version: int) -> list[dict[str, Any]]:
        """`didChange` the whole buffer (the server advertises full sync)."""
        self.send(
            {
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": {"uri": URI, "version": version},
                    "contentChanges": [{"text": text}],
                },
            }
        )
        return self.publish()

    def formatting(self, identifier: Any) -> Any:
        """`textDocument/formatting` for the one open buffer."""
        self.send(
            {
                "jsonrpc": "2.0",
                "id": identifier,
                "method": "textDocument/formatting",
                "params": {"textDocument": {"uri": URI}, "options": {}},
            }
        )
        return self.response(identifier).get("result")

    def shutdown_and_exit(self) -> int:
        """`shutdown` -> `exit`, then the process status."""
        self.send({"jsonrpc": "2.0", "id": 5, "method": "shutdown", "params": None})
        if self.response(5).get("result") is not None:
            raise AssertionError("shutdown must answer null")
        self.send({"jsonrpc": "2.0", "method": "exit", "params": None})
        return self.wait()

    def wait(self) -> int:
        """Closes stdin, waits for the exit status, and reads stderr."""
        try:
            if self.process.stdin is not None:
                self.process.stdin.close()
        except OSError:
            pass
        try:
            return self.process.wait(timeout=PATIENCE)
        except subprocess.TimeoutExpired:
            self.kill()
            raise AssertionError(
                f"the server did not exit within {PATIENCE:.0f}s"
            ) from None

    def stderr_text(self) -> str:
        """Everything the server wrote to stderr, as text."""
        if self.process.stderr is None:
            return ""
        return self.process.stderr.read().decode("utf-8", "replace")

    def kill(self) -> None:
        """Ends a server that overran its deadline and releases its pipes."""
        if self.process.poll() is None:
            self.process.kill()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                pass
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None:
                try:
                    stream.close()
                except OSError:
                    pass


class LspStdioTest(unittest.TestCase):
    """The session the editor runs, over real pipes."""

    def session(self) -> Session:
        """A started server, killed at the end of the test if it survives."""
        instance = Session()
        self.addCleanup(instance.kill)
        return instance

    def test_the_capability_document_is_the_accepted_one(self) -> None:
        session = self.session()
        result = session.handshake()
        capabilities = result.get("capabilities", {})
        self.assertEqual(capabilities.get("positionEncoding"), "utf-16")
        self.assertEqual(
            capabilities.get("textDocumentSync"),
            {"openClose": True, "change": 1, "save": False},
        )
        self.assertIs(capabilities.get("documentFormattingProvider"), True)
        self.assertIs(capabilities.get("hoverProvider"), True)
        for absent in (
            "definitionProvider",
            "diagnosticProvider",
            "completionProvider",
            "documentSymbolProvider",
        ):
            self.assertNotIn(absent, capabilities)
        self.assertEqual(result.get("serverInfo", {}).get("name"), "blastit")
        self.assertEqual(session.wait(), 1, "EOF without shutdown is exit 1")

    def test_one_document_three_formatting_outcomes_and_a_clean_stderr(self) -> None:
        session = self.session()
        session.handshake()

        # The broken buffer: exactly one diagnostic at the documented position,
        # and formatting is declined (null), never a JSON-RPC error.
        diagnostics = session.open(BROKEN, 0)
        self.assertEqual(len(diagnostics), 1, diagnostics)
        diagnostic = diagnostics[0]
        self.assertEqual(diagnostic.get("severity"), 1)
        self.assertEqual(diagnostic.get("source"), "blastit")
        self.assertEqual(
            diagnostic.get("range"),
            {
                "start": {"line": 2, "character": 7},
                "end": {"line": 2, "character": 8},
            },
        )
        self.assertEqual(diagnostic.get("message"), EQUALS)
        self.assertIsNone(session.formatting(2), "a broken buffer is declined")

        # The canonical buffer: nothing to publish and nothing to edit.
        self.assertEqual(session.change(CANONICAL, 1), [])
        self.assertEqual(session.formatting(3), [])

        # The two-space buffer: exactly one whole-document edit.
        self.assertEqual(session.change(MISINDENTED, 2), [])
        self.assertEqual(
            session.formatting(4),
            [
                {
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 5, "character": 0},
                    },
                    "newText": CANONICAL,
                }
            ],
        )

        # A document whose SQL would be a secret in a log line.
        diagnostics = session.change(SQL, 3)
        self.assertEqual(len(diagnostics), 1, diagnostics)
        message = diagnostics[0].get("message", "")
        for secret in SQL_SECRETS:
            self.assertNotIn(secret, message)

        self.assertEqual(session.shutdown_and_exit(), 0, "shutdown then exit is 0")
        stderr = session.stderr_text()
        for secret in SQL_SECRETS:
            self.assertNotIn(secret, stderr)
        self.assertNotIn("FROM PROGRAM", stderr)

    def test_eof_before_shutdown_exits_1(self) -> None:
        session = self.session()
        session.handshake()
        self.assertEqual(
            session.wait(), 1, "a client that vanishes is a failed session"
        )


if __name__ == "__main__":
    unittest.main()
