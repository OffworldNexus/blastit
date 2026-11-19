"""`blastit` and `python -m blastit`: the console script.

The package is pure Python: the command line is handed to the universal `blastit`
binary the wheel ships at ``blastit/_bin/blastit`` (a bundle installs it at
``<prefix>/share/blastit/lib/blastit``), so one ``py3-none-*`` wheel serves every
CPython ``>=3.12``. Blastit therefore has no interpreter of its own in the
process, and three things follow from that:

* stage 0 records ``sys.executable`` in ``BLASTIT_WHEEL_PYTHON`` — the
  interpreter ``blastit check`` runs the project's ``pytest`` on — in the
  environment the binary inherits;
* the credentials-holding role is the same binary with ``--role=dbproxy``: it
  runs no interpreter at all;
* the configuration comes from the environment, as §8.2 says: the usual
  ``BLASTIT_*`` variables, plus an optional ``--preset NAME``.
"""

import os
import sys


def main() -> None:
    """Runs blastit: it becomes the server, or exits non-zero.

    Raises `SystemExit`, which is what a console script's exit status is.
    """
    from blastit import _blastit

    os.environ.setdefault("BLASTIT_WHEEL_PYTHON", sys.executable)
    os.environ["BLASTIT_HANDOFF_DONE"] = "1"
    raise SystemExit(_blastit.main(sys.argv[1:], sys.executable, ""))


def main_lsp() -> None:
    """`blastit-lsp`: the language server a bundle ships beside the binary."""
    from blastit import _blastit

    program = _blastit.lsp_bin_path()
    os.execvpe(program, [program, *sys.argv[1:]], dict(os.environ))


if __name__ == "__main__":
    main()
