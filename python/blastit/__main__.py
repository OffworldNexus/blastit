"""`blastit` and `python -m blastit`: the console script.

The entry point runs the server on the interpreter the wheel was installed into
(the project's virtualenv), so the application's dependencies are importable for
the same reason they are for ``python app.py``. From there the stages of §4:

* stage 0 re-executes *this* interpreter (``python -I -m blastit``) for stage 1,
  carrying the interpreter's ``sys.path`` across the barrier;
* it ``execve``s ``blastit/_bin/blastit-dbproxy`` — a binary with no interpreter
  in it — for the role that holds the real credentials;
* and the configuration stays environment-only, as §8.2 says: the usual
  ``BLASTIT_*`` variables, and nothing else.
"""

import os
import sys


def main() -> None:
    """Runs blastit: it becomes the server, or exits non-zero.

    Raises `SystemExit`, which is what a console script's exit status is.
    """
    import blastit._blastit as _blastit

    here = os.path.dirname(os.path.abspath(__file__))
    dbproxy = os.path.join(here, "_bin", "blastit-dbproxy")
    raise SystemExit(_blastit.main(sys.argv[1:], sys.executable, dbproxy))


if __name__ == "__main__":
    main()
