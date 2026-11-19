"""The pure-stdlib bridge between this package and the `blastit` binary.

The wheel is `py3-none-*`: one artifact for every CPython ``>=3.12``. Nothing
here is compiled, so ``import blastit`` works on an interpreter no per-minor
build ever targeted, and the program itself is the universal binary the package
ships.

Every call is one line of NDJSON to a lazily started
``blastit --internal-json`` child (``crates/blastit/src/cli/internal_json.rs``)::

    -> {"op": "normalize", "args": {"sql": "SELECT 1"}}
    <- {"ok": true, "result": "SELECT 1"}

A refusal comes back as ``{"ok": false, "error": "..."}`` and is raised as
:class:`ValueError` naming the operation, which is what the retired extension
raised and what ``fold``, ``check --learn`` and the pytest plugin catch. Every
other way the bridge can fail — a package with no binary, a child that died, a
pipe that closed — is also a :class:`ValueError` with a next step in it, so no
caller ever sees a raw ``OSError`` or a traceback for an installation problem.
Design §3 is the whole contract: twelve operations, one argument document each.

Fork safety
-----------
A view is a ``fork`` of a process that may have used this bridge, and a pipe
shared with the parent would interleave two clients on one stream. The child is
therefore owned by ``(owner_pid == os.getpid(), threading.Lock)``, and
``os.register_at_fork`` drops the inherited streams in the child **without
flushing** (the bytes in flight are the parent's), so a call after a ``fork``
starts a child of its own. Calling a bridge function from a forked view is not
the intended path — ``blastit check`` and ``fold`` are the callers — but the
guard is what makes that mistake a fresh child instead of a corrupted protocol.
"""

from __future__ import annotations

import atexit
import base64
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
from typing import Any

__all__ = [
    "analyze",
    "audit",
    "bin_path",
    "blastfile_apply_edits",
    "blastfile_model",
    "check_audit",
    "classify",
    "covering",
    "has_quoted_literal",
    "lsp_bin_path",
    "main",
    "normalize",
    "plan_learn",
    "probe_ingress",
    "redact_literals",
]

#: The largest reply line this client accepts. A report is the biggest answer
#: the dispatcher sends, and it is far below this; anything longer is a
#: desynchronised stream, which is dropped rather than read.
MAX_REPLY = 64 * 1024 * 1024

#: The one child, and the reader in front of its stdout.
#:
#: ``_OWNER`` is the process that started it: another pid is a ``fork``, and
#: ``_after_fork`` has already replaced the lock and closed the descriptors.
_LOCK = threading.Lock()
_OWNER = os.getpid()
_CHILD: subprocess.Popen[bytes] | None = None
_READER: io.BufferedReader | None = None
#: The program the child runs, so a refusal can name the one that misbehaved.
_PROGRAM: str | None = None
#: The child's own stderr, kept so a refusal can repeat its last words instead
#: of discarding the sentence that explained a death.
_ERRORS: Any = None


def _executable(path: str) -> bool:
    """Whether `path` is a file this process may execute."""
    return os.path.isfile(path) and os.access(path, os.X_OK)


def _candidates(name: str) -> list[str]:
    """Where an installation may keep `name`, in resolution order."""
    package = os.path.dirname(os.path.abspath(__file__))
    return [
        os.path.join(package, "_bin", name),
        os.path.abspath(os.path.join(package, "..", "..", "lib", name)),
    ]


def _program(name: str) -> str:
    """The installation's `name`, from the package, `BLASTIT_BIN`, or this prefix."""
    candidates = _candidates(name)
    for candidate in candidates:
        if _executable(candidate):
            return candidate
    if name == "blastit":
        configured = os.environ.get("BLASTIT_BIN")
        if configured:
            if not _executable(configured):
                raise ValueError(
                    f"BLASTIT_BIN={configured} is not an executable file; unset "
                    "BLASTIT_BIN, or point it at the blastit binary"
                )
            return configured
    # An installed wheel puts the binary in the interpreter's own `bin/`
    # (`pip` installs a wheel's data scripts there), which is also where a
    # bundle's `bin/blastit` symlink lives. `MB.42.3`'s repack moves it to
    # `blastit/_bin/`, which the two candidates above already cover.
    beside = os.path.join(os.path.dirname(sys.executable), name)
    if _executable(beside):
        return beside
    found = shutil.which(name)
    if found is not None:
        return found
    looked = ", ".join([*candidates, "BLASTIT_BIN", "PATH"])
    if name == "blastit":
        raise ValueError(
            "this blastit package is incomplete: it ships no `blastit` (looked in "
            f"{looked}); reinstall it with `pip install --force-reinstall blastit`"
        )
    raise ValueError(
        f"this blastit package is incomplete: it ships no `{name}` (looked in {looked})"
    )


def bin_path() -> str:
    """The `blastit` program this package drives."""
    return _program("blastit")


def lsp_bin_path() -> str:
    """The `blastit-lsp` program a bundle ships beside `blastit`."""
    return _program("blastit-lsp")


def _spawn() -> None:
    """Starts the NDJSON child, unbuffered so a fork can close it without flushing."""
    global _CHILD, _READER, _PROGRAM, _ERRORS
    program = bin_path()
    errors = tempfile.TemporaryFile()  # noqa: SIM115 - owned by _discard/_close
    try:
        child = subprocess.Popen(
            [program, "--internal-json"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=errors,
            bufsize=0,
            close_fds=True,
        )
    except OSError as error:
        errors.close()
        raise ValueError(
            f"the blastit at {program} could not be started: {error}"
        ) from error
    if (
        child.stdin is None or child.stdout is None
    ):  # pragma: no cover - PIPE implies both
        _discard()
        raise ValueError(
            "the blastit bridge could not be started: no stdin/stdout pipe"
        )
    _CHILD = child
    _READER = io.BufferedReader(child.stdout)
    _PROGRAM = program
    _ERRORS = errors


def _stderr_tail() -> str:
    """The child's own last line, as a `": …"` suffix (empty when it said nothing)."""
    if _ERRORS is None:
        return ""
    try:
        _ERRORS.seek(0)
        text = _ERRORS.read(4096).decode("utf-8", "replace").strip()
    except (OSError, ValueError):
        return ""
    lines = text.splitlines()
    return f": {lines[-1]}" if lines else ""


def _discard() -> int | None:
    """Ends a child that desynchronised, so the next call starts a fresh one.

    Returns the status it exited with, which is what a refusal quotes when the
    child died before answering.
    """
    global _CHILD, _READER, _PROGRAM
    child, _CHILD = _CHILD, None
    reader, _READER = _READER, None
    _PROGRAM = None
    if reader is not None:
        try:
            reader.close()
        except (OSError, ValueError):
            pass
    if child is None:
        return None
    status = child.poll()
    if status is not None:
        return status
    try:
        child.kill()
    except OSError:
        pass
    try:
        return child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        return None


def _after_fork() -> None:
    """Drops the parent's child in a `fork`ed process, without flushing it.

    Every buffered object is closed through its own ``close()``: with
    ``bufsize=0`` there is no buffer to flush, so the parent's in-flight request
    is left exactly as it was and the child never writes a descriptor number the
    object still believes it owns.
    """
    global _LOCK, _OWNER, _CHILD, _READER, _PROGRAM, _ERRORS
    _LOCK = threading.Lock()
    _OWNER = os.getpid()
    child, _CHILD = _CHILD, None
    reader, _READER = _READER, None
    errors, _ERRORS = _ERRORS, None
    _PROGRAM = None
    if reader is not None:
        try:
            reader.close()
        except (OSError, ValueError):
            pass
    if child is not None and child.stdin is not None:
        try:
            child.stdin.close()
        except (OSError, ValueError):
            pass
    if errors is not None:
        try:
            errors.close()
        except (OSError, ValueError):
            pass


def _close() -> None:
    """Ends the child at exit: closing stdin is its end of input."""
    global _CHILD, _READER, _PROGRAM, _ERRORS
    if _OWNER != os.getpid():
        return
    child, _CHILD = _CHILD, None
    reader, _READER = _READER, None
    errors, _ERRORS = _ERRORS, None
    _PROGRAM = None
    if reader is not None:
        try:
            reader.close()
        except (OSError, ValueError):
            pass
    if child is not None and child.stdin is not None:
        try:
            child.stdin.close()
        except (OSError, ValueError):
            pass
    if child is not None:
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
    if errors is not None:
        try:
            errors.close()
        except (OSError, ValueError):
            pass


def _write_all(stream: Any, data: bytes) -> None:
    """Writes every byte to an unbuffered pipe, which may accept a partial write."""
    written = 0
    while written < len(data):
        count = stream.write(data[written:])
        if not count:
            raise ValueError("the blastit bridge closed its input")
        written += count


def _request(operation: str, arguments: dict[str, Any]) -> Any:
    """Sends one request and returns its ``result``, or raises :class:`ValueError`."""
    if _OWNER != os.getpid():
        _after_fork()
    payload = json.dumps({"op": operation, "args": arguments}).encode("utf-8") + b"\n"
    with _LOCK:
        if _CHILD is None or _CHILD.poll() is not None:
            _spawn()
        child, reader = _CHILD, _READER
        if child is None or child.stdin is None or reader is None:
            raise ValueError("the blastit bridge is not running")
        try:
            _write_all(child.stdin, payload)
            reply = reader.readline(MAX_REPLY)
        except (BrokenPipeError, OSError, ValueError) as error:
            _discard()
            raise ValueError(
                f"{operation}: the blastit bridge closed its input"
            ) from error
        if not reply:
            status = _discard()
            tail = _stderr_tail()
            raise ValueError(
                f"`blastit --internal-json` exited before answering `{operation}` "
                f"(status {status}){tail}; reinstall this package with "
                "`pip install --force-reinstall blastit`"
            )
        if not reply.endswith(b"\n"):
            _discard()
            raise ValueError(
                f"{operation}: the blastit bridge sent no newline-terminated line "
                f"({MAX_REPLY} bytes is its limit)"
            )
    try:
        answer = json.loads(reply.decode("utf-8", "replace"))
    except ValueError as error:
        program = _PROGRAM or "the blastit on PATH"
        raise ValueError(
            f"the blastit at {program} answered `{operation}` with invalid JSON "
            f"({error}); this package needs the blastit it ships — run "
            "`pip install --force-reinstall blastit`"
        ) from error
    if isinstance(answer, dict) and answer.get("ok") is True:
        return answer.get("result")
    message = answer.get("error") if isinstance(answer, dict) else None
    if isinstance(message, str) and message:
        raise ValueError(f"{operation}: {message}")
    raise ValueError(f"{operation}: the blastit bridge refused the request")


def _text(operation: str, arguments: dict[str, Any]) -> str:
    """One operation whose result is a `str`."""
    result = _request(operation, arguments)
    if not isinstance(result, str):
        raise TypeError(
            f"the blastit bridge answered `{operation}` with a non-string result"
        )
    return result


def _boolean(operation: str, arguments: dict[str, Any]) -> bool:
    """One operation whose result is a `bool`."""
    result = _request(operation, arguments)
    if not isinstance(result, bool):
        raise TypeError(
            f"the blastit bridge answered `{operation}` with a non-boolean result"
        )
    return result


def normalize(sql: str) -> str:
    """The normalised text a grant stores and compares (design §3)."""
    return _text("normalize", {"sql": sql})


def has_quoted_literal(sql: str) -> bool:
    """Whether one SQL text carries any quoted literal (``R24``, ``R25``)."""
    return _boolean("has_quoted_literal", {"sql": sql})


def redact_literals(sql: str) -> str:
    """One SQL text with every quoted literal replaced by ``…``."""
    return _text("redact_literals", {"sql": sql})


def audit(manifest_text: str, observations_json: str) -> str:
    """Diffs a test suite's observations against a manifest; returns JSON text."""
    return _text(
        "audit",
        {"manifest_text": manifest_text, "observations_json": observations_json},
    )


def analyze(sql: str) -> str:
    """The smallest table grant covering one statement; returns JSON text."""
    return _text("analyze", {"sql": sql})


def covering(statements: list[str], locator: str) -> str:
    """The smallest table grant covering several statements; returns JSON text."""
    return _text(
        "covering",
        {"statements": list(statements), "locator": locator},
    )


def classify(sql: str) -> str:
    """Names one statement's kind with PostgreSQL's parser; returns JSON text."""
    return _text("classify", {"sql": sql})


def check_audit(manifest_text: str, http_obs_jsonl: str, sql_obs_jsonl: str) -> str:
    """Diffs one `blastit check` run against the manifest; returns JSON text."""
    return _text(
        "check_audit",
        {
            "manifest_text": manifest_text,
            "http_obs_jsonl": http_obs_jsonl,
            "sql_obs_jsonl": sql_obs_jsonl,
        },
    )


def plan_learn(
    manifest_text: str,
    http_obs_jsonl: str,
    sql_obs_jsonl: str,
    dry_run: bool = False,
) -> str:
    """Plans what `--learn` would write; returns JSON text."""
    return _text(
        "plan_learn",
        {
            "manifest_text": manifest_text,
            "http_obs_jsonl": http_obs_jsonl,
            "sql_obs_jsonl": sql_obs_jsonl,
            "dry_run": bool(dry_run),
        },
    )


def probe_ingress(content_type: str | None, prefix: bytes, total_len: int) -> str:
    """The bounded body sample of one request; returns JSON text."""
    return _text(
        "probe_ingress",
        {
            "content_type": content_type,
            "prefix_b64": base64.b64encode(bytes(prefix)).decode("ascii"),
            "total_len": int(total_len),
        },
    )


def blastfile_apply_edits(source: str, edits_json: str) -> str:
    """Applies a `BlastfileEdits` document with the one writer; returns the source."""
    return _text("blastfile_apply_edits", {"source": source, "edits_json": edits_json})


def blastfile_model(source: str) -> str:
    """Reads a `Blastfile` into its lowered model; returns JSON text."""
    return _text("blastfile_model", {"source": source})


def main(argv: list[str], executable: str, dbproxy: str = "") -> int:
    """Replaces this process with `blastit`, on the command line's own argv.

    `executable` is the interpreter the console script ran on. It is recorded in
    ``BLASTIT_WHEEL_PYTHON`` so ``blastit check`` audits with the project's own
    environment, and ``blastit-env`` strips it from the application's
    environment like every other ``BLASTIT_*`` variable. `dbproxy` is kept for
    the console script's signature: the binary plays that role itself
    (``--role=dbproxy``), so no second program ships.

    An installation the command line cannot start is :class:`SystemExit` with
    the one sentence an operator can act on, never a traceback.
    """
    del dbproxy
    environment = dict(os.environ)
    environment.setdefault("BLASTIT_WHEEL_PYTHON", executable)
    environment["BLASTIT_HANDOFF_DONE"] = "1"
    try:
        program = bin_path()
        os.execvpe(program, [program, *argv], environment)
    except (OSError, ValueError) as error:
        raise SystemExit(f"blastit: {error}") from error
    return 0  # `execvpe` never returns


atexit.register(_close)
os.register_at_fork(after_in_child=_after_fork)
