"""The thin bridge between this package and the universal binary the wheel ships.

The wheel is ``py3-none-*``: one artifact for every CPython ``>=3.12``, with no
compiled code in the package at all. The program is the Rust binary the package
carries at ``blastit/_bin/blastit`` (a bundle keeps it at
``<prefix>/share/blastit/lib/``), so every console script ends in one
``os.execvpe``: the interpreter is gone before any server work starts.

A wheel cannot carry a file mode — ``pip`` writes ``0644`` outside
``*.data/scripts/`` — so :func:`main` gives the programs under the package's own
``_bin/`` their execute bit back, and never a path a caller named.
"""

from __future__ import annotations

import os
import shutil
import stat
import sys

__all__ = ["EXEC_GUARD", "bin_path", "lsp_bin_path", "main"]

#: Set by :func:`main` in the environment of the binary it `exec`s. Asked for a
#: program while it is set, the package's own ``_bin/`` did not answer, and the
#: ``PATH`` fallbacks would find the script that is running — an `exec` loop.
EXEC_GUARD = "BLASTIT_WHEEL_EXEC"

#: The programs a wheel keeps in ``blastit/_bin/`` (§6.1). One program plays
#: every role — stage 0 reaches the credentials-holding one as
#: ``blastit --role=dbproxy`` — so there is nothing else here to repair.
_PROGRAMS = ("blastit", "blastit-lsp")


def _executable(path: str, *, repair: bool = False) -> bool:
    """Whether ``path`` is a file this process may execute.

    ``repair`` is set only for paths under this package's own ``_bin/``: a
    ``chmod`` on a path a caller named is not this package's business, and a
    read-only ``site-packages`` cannot be repaired at all.
    """
    if not os.path.isfile(path):
        return False
    if os.access(path, os.X_OK):
        return True
    if not repair:
        return False
    try:
        # The three execute bits on the mode the file already has: a `0600` file
        # must not become world-readable just because it became runnable.
        os.chmod(path, stat.S_IMODE(os.stat(path).st_mode) | 0o111)
    except OSError:
        return False
    return os.access(path, os.X_OK)


def _bin_dir() -> str:
    """``<package>/_bin`` — the only directory whose modes this package repairs."""
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), "_bin")


def _repair_modes(program: str) -> None:
    """Give the package's own ``_bin/`` programs their execute bit back."""
    directory = _bin_dir()
    if os.path.dirname(os.path.abspath(program)) != directory:
        return
    for name in _PROGRAMS:
        _executable(os.path.join(directory, name), repair=True)


def _candidates(name: str) -> list[tuple[str, bool]]:
    """``(path, repair)`` where an installation may keep ``name``, in order.

    Only the package's own ``_bin/`` may have its mode repaired; a bundle's
    ``<prefix>/share/blastit/lib`` belongs to its owner.
    """
    package = os.path.dirname(os.path.abspath(__file__))
    return [
        (os.path.join(package, "_bin", name), True),
        (os.path.abspath(os.path.join(package, "..", "..", "lib", name)), False),
    ]


def _program(name: str) -> str:
    """The installation's ``name``: this package, ``BLASTIT_BIN``, or this prefix."""
    candidates = _candidates(name)
    for candidate, repair in candidates:
        if _executable(candidate, repair=repair):
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
    if os.environ.get(EXEC_GUARD):
        looked = ", ".join(path for path, _ in candidates)
        raise ValueError(
            "this blastit package is incomplete: it ships no executable "
            f"`{name}` under {looked or 'its own _bin/'} and {EXEC_GUARD} is set, so "
            f"no other `{name}` on PATH was tried; reinstall it with "
            "`pip install --force-reinstall blastit`"
        )
    beside = os.path.join(os.path.dirname(sys.executable), name)
    if _executable(beside):
        return beside
    found = shutil.which(name)
    if found is not None:
        return found
    looked = ", ".join([*(path for path, _ in candidates), "BLASTIT_BIN", "PATH"])
    raise ValueError(
        f"this blastit package is incomplete: it ships no `{name}` (looked in "
        f"{looked}); reinstall it with `pip install --force-reinstall blastit`"
    )


def bin_path() -> str:
    """The ``blastit`` program this package runs."""
    return _program("blastit")


def lsp_bin_path() -> str:
    """The ``blastit-lsp`` program the wheel ships beside it."""
    return _program("blastit-lsp")


def main(argv: list[str], executable: str, dbproxy: str = "") -> int:
    """Replaces this process with ``blastit``, on the command line's own argv.

    ``executable`` is the interpreter the console script ran on; it is recorded
    in ``BLASTIT_WHEEL_PYTHON`` so ``blastit check`` audits with the project's own
    environment. ``dbproxy`` keeps the console script's signature: the binary
    plays that role itself (``--role=dbproxy``).

    ``BLASTIT_HANDOFF_DONE`` is **not** set here: §4's handoff belongs to the
    binary, and setting it first would stop a pip-installed ``blastit`` from ever
    using ``.venv/bin/blastit``.
    """
    del dbproxy
    environment = dict(os.environ)
    environment.setdefault("BLASTIT_WHEEL_PYTHON", executable)
    environment.setdefault(EXEC_GUARD, "1")
    try:
        program = bin_path()
        _repair_modes(program)
        os.execvpe(program, [program, *argv], environment)
    except (OSError, ValueError) as error:
        raise SystemExit(f"blastit: {error}") from error
    return 0  # `execvpe` never returns
