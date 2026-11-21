"""`blastit`, `blastit-lsp` and `python -m blastit`: the console scripts.

The package carries no server: every entry point ends in one ``os.execvpe`` of a
binary it ships (``blastit/_bin/blastit``, and ``blastit/_bin/blastit-lsp``), so
one ``py3-none-*`` wheel serves every CPython ``>=3.12`` and no interpreter hosts
a stage. The command line and the exit status are the binary's own.
"""

import os
import sys


def main() -> None:
    """Runs blastit: it becomes the server, or exits non-zero.

    Raises `SystemExit`, which is what a console script's exit status is.
    """
    from blastit import _blastit

    os.environ.setdefault("BLASTIT_WHEEL_PYTHON", sys.executable)
    raise SystemExit(_blastit.main(sys.argv[1:], sys.executable, ""))


def main_lsp() -> None:
    """`blastit-lsp`: the language server a bundle ships beside the binary."""
    from blastit import _blastit

    try:
        # Resolved before the environment is copied, and the guard is in the
        # environment this process becomes: asked for a program while `_bin/`
        # has none, the fallbacks would find this very console script and
        # `exec` it again, one interpreter deep per turn.
        program = _blastit.lsp_bin_path()
        environment = dict(os.environ)
        environment.setdefault(_blastit.EXEC_GUARD, "1")
        os.execvpe(program, [program, *sys.argv[1:]], environment)
    except (OSError, ValueError) as error:
        raise SystemExit(f"blastit-lsp: {error}") from error


if __name__ == "__main__":
    main()
