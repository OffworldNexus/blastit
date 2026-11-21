#!/usr/bin/env python3
"""Drive one real LSP session against a built `blastit-lsp` (`MB.35.5`).

The artifact has to be **executed**, not inspected: "it compiled" is a claim, and
the archive an editor downloads is only proved by a session that speaks the
protocol through it. This script is that session, and it is the same script on
all five legs — a platform with a different framing bug is found by the leg that
runs it, never by a per-platform copy of the assertions.

The session is the minimum that can tell a working server from a silent one:

1. `initialize` → the capability document, including `codeActionProvider: true`
   (the boolean, not a kind list: `docs/design/blastfile-lsp-zed.md` §2.3);
2. `didOpen` a `Blastfile` whose `query` misspells `SELECT` → exactly one
   severity-1 `publishDiagnostics` at the typo's own span, line 4 characters
   15–20, with the sentence that names the fix;
3. `shutdown` (`result: null`) → `exit` → status `0`, and **not one stderr
   byte**.

It is stdlib only, spawns with an argument list (so Windows needs no shell and no
`shell=True`), and takes exactly one option. `scripts/build-lsp.sh` produces the
archive; unpack it and hand this script the executable inside it (§14.4.1):

    scripts/lsp_smoke.py --binary target/release/blastit-lsp
    lsp_smoke: ok — target/release/blastit-lsp: ... 0 bytes on stderr

Exit `0` with one line on stdout, or `1` with one line on stderr.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import threading
from pathlib import Path

#: How long any single frame may take, and how long the child may take to exit.
#: A server that hangs is a failure, not a hung CI job.
PATIENCE = 30.0

#: The `Blastfile` the session opens. `SELCT` is the misspelt `SELECT`, at
#: 0-based line 4, characters 15–20 — its own span, not the block's.
BLASTFILE = (
    "postgres $DATABASE_URL\n"
    "\n"
    'view v "/x" {\n'
    "    postgres {\n"
    "        query 'SELCT id FROM t'\n"
    "    }\n"
    "}\n"
)
TYPO_LINE = 4
TYPO_START = 15
TYPO_END = 20

#: §2.3's capability document, byte for byte. `codeActionProvider` is `true`
#: because `true` means "the default kinds", which is the only set the server
#: offers; an object would advertise a kind list it does not filter by.
CAPABILITIES = {
    "positionEncoding": "utf-16",
    "textDocumentSync": {"openClose": True, "change": 1, "save": False},
    "documentFormattingProvider": True,
    "hoverProvider": True,
    "codeActionProvider": True,
}


class Failure(Exception):
    """One thing the session did not do, in a sentence naming the fix."""


class Session:
    """One live `blastit-lsp`: framed stdin/stdout, and its stderr for the verdict."""

    def __init__(self, binary: Path) -> None:
        try:
            # A list, not a string: no shell, so Windows needs no quoting rules.
            self.child = subprocess.Popen(
                [str(binary)],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
        except OSError as error:
            raise Failure(f"cannot spawn {binary}: {error}") from error
        self.stdin = self.child.stdin
        self.stdout = self.child.stdout
        self.stderr = self.child.stderr
        self.timed_out = False
        self._watchdog = threading.Timer(PATIENCE, self._kill)
        self._watchdog.daemon = True
        self._watchdog.start()

    def _kill(self) -> None:
        self.timed_out = True
        self.child.kill()

    def send(self, message: dict) -> None:
        body = json.dumps(message, separators=(",", ":")).encode("utf-8")
        header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
        assert self.stdin is not None
        self.stdin.write(header + body)
        self.stdin.flush()

    def recv(self, what: str) -> dict:
        """The next frame, or a `Failure` naming what the session waited for."""
        assert self.stdout is not None
        length = None
        while True:
            line = self.stdout.readline()
            if not line:
                raise Failure(
                    f"stdout closed while waiting for {what}"
                    + (f" after {PATIENCE:g}s" if self.timed_out else "")
                )
            header = line.decode("utf-8", "replace").strip()
            if not header:
                break
            name, _, value = header.partition(":")
            if name.lower() == "content-length":
                try:
                    length = int(value.strip())
                except ValueError as error:
                    raise Failure(f"{what}: `{header}` is not a length") from error
        if length is None:
            raise Failure(f"{what}: a header block with no `Content-Length`")
        body = self.stdout.read(length)
        if len(body) != length:
            raise Failure(f"{what}: the declared body ended after {len(body)} bytes")
        try:
            message = json.loads(body)
        except json.JSONDecodeError as error:
            raise Failure(f"{what}: the body is not JSON: {error}") from error
        if not isinstance(message, dict):
            raise Failure(f"{what}: the frame is not a JSON object")
        return message

    def finish(self) -> tuple[int, str]:
        """Close stdin, wait for the child, and read every stderr byte."""
        try:
            self._watchdog.cancel()
        finally:
            if self.stdin is not None:
                self.stdin.close()
                self.stdin = None
        try:
            status = self.child.wait(timeout=PATIENCE)
        except subprocess.TimeoutExpired:
            self._kill()
            self.child.wait()
            raise Failure(f"the server did not exit within {PATIENCE:g}s") from None
        self._watchdog.cancel()
        assert self.stderr is not None
        stderr = self.stderr.read().decode("utf-8", "replace")
        return status, stderr


def ok(value: object, message: str) -> None:
    if not value:
        raise Failure(message)


def drive(binary: Path) -> str:
    """The whole session, and the one line that reports it."""
    session = Session(binary)
    try:
        session.send(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"capabilities": {}, "processId": None},
            }
        )
        reply = session.recv("the reply to `initialize`")
        ok(
            reply.get("jsonrpc") == "2.0",
            f"`initialize` answered {reply!r}, not JSON-RPC 2.0",
        )
        ok(reply.get("id") == 1, f"`initialize` did not echo id 1: {reply!r}")
        ok("error" not in reply, f"`initialize` returned an error: {reply!r}")
        result = reply.get("result")
        ok(
            isinstance(result, dict),
            f"`initialize` returned no result object: {reply!r}",
        )
        capabilities = result.get("capabilities")
        ok(
            capabilities == CAPABILITIES,
            "the capability document is not §2.3's: "
            f"got {json.dumps(capabilities, sort_keys=True)}, "
            f"want {json.dumps(CAPABILITIES, sort_keys=True)}",
        )
        # Equality already implies `true`; asserting `is True` keeps a future
        # `{}` (a kind list) from being read as "the same" by a truthy compare.
        ok(
            capabilities.get("codeActionProvider") is True,
            "`codeActionProvider` must be the boolean `true`, not a kind object",
        )
        server = result.get("serverInfo")
        ok(
            isinstance(server, dict) and server.get("name") == "blastit",
            f"`serverInfo` does not name the server: {server!r}",
        )

        session.send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
        uri = "file:///Blastfile"
        session.send(
            {
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "blastfile",
                        "version": 1,
                        "text": BLASTFILE,
                    }
                },
            }
        )
        notification = session.recv("`textDocument/publishDiagnostics`")
        ok(
            notification.get("method") == "textDocument/publishDiagnostics",
            f"`didOpen` answered {notification.get('method')!r}, not a diagnostics push",
        )
        params = notification.get("params") or {}
        ok(
            params.get("uri") == uri,
            f"the diagnostics are for {params.get('uri')!r}, not {uri}",
        )
        diagnostics = params.get("diagnostics")
        ok(
            isinstance(diagnostics, list),
            f"`diagnostics` is not a list: {diagnostics!r}",
        )
        ok(
            len(diagnostics) == 1,
            f"the misspelt `SELECT` produced {len(diagnostics)} diagnostic(s), not 1: "
            f"{json.dumps(diagnostics, sort_keys=True)}",
        )
        diagnostic = diagnostics[0]
        ok(
            diagnostic.get("severity") == 1,
            f"the diagnostic's severity is {diagnostic.get('severity')!r}, not 1 (an error)",
        )
        ok(
            diagnostic.get("source") == "blastit",
            f"the diagnostic's source is {diagnostic.get('source')!r}, not `blastit`",
        )
        start = (diagnostic.get("range") or {}).get("start") or {}
        end = (diagnostic.get("range") or {}).get("end") or {}
        ok(
            (start.get("line"), start.get("character")) == (TYPO_LINE, TYPO_START)
            and (end.get("line"), end.get("character")) == (TYPO_LINE, TYPO_END),
            "the diagnostic is not at the typo's own span: "
            f"got {start.get('line')}:{start.get('character')}–"
            f"{end.get('line')}:{end.get('character')}, "
            f"want {TYPO_LINE}:{TYPO_START}–{TYPO_LINE}:{TYPO_END}",
        )
        message = diagnostic.get("message")
        ok(
            isinstance(message, str) and "does not parse" in message,
            f"the diagnostic does not say what is wrong: {message!r}",
        )

        session.send({"jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": None})
        reply = session.recv("the reply to `shutdown`")
        ok(reply.get("id") == 2, f"`shutdown` did not echo id 2: {reply!r}")
        ok(
            reply.get("result") is None,
            f"`shutdown` answered {reply.get('result')!r}, not null",
        )
        session.send({"jsonrpc": "2.0", "method": "exit", "params": None})

        status, stderr = session.finish()
    except Failure:
        session._watchdog.cancel()
        session.child.kill()
        session.child.wait()
        raise
    except BaseException:
        session._watchdog.cancel()
        session.child.kill()
        session.child.wait()
        raise

    ok(
        status == 0,
        f"the server exited {status}, not 0 (a clean `shutdown` then `exit` is 0)"
        + (f"; stderr: {stderr.strip()[:300]!r}" if stderr.strip() else ""),
    )
    ok(
        stderr == "",
        f"the server wrote {len(stderr)} byte(s) to stderr, and it must write none: "
        f"{stderr.strip()[:300]!r}",
    )
    return (
        f"lsp_smoke: ok — {binary}: initialize + codeActionProvider, one severity-1 "
        f"diagnostic at {TYPO_LINE}:{TYPO_START}, shutdown/exit 0, 0 bytes on stderr"
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--binary",
        required=True,
        help="the blastit-lsp executable to drive (the file inside the archive)",
    )
    args = parser.parse_args(argv)
    binary = Path(args.binary)
    if not binary.is_file():
        print(
            f"lsp_smoke: FAIL — {binary} does not exist: build it or unpack the archive first",
            file=sys.stderr,
        )
        return 1
    try:
        print(drive(binary))
    except Failure as failure:
        print(f"lsp_smoke: FAIL — {binary}: {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
