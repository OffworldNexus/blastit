"""The wheel's launcher: one `execvpe` of the binary the package ships.

A shell-script stand-in for `blastit/_bin/blastit` is staged in a temporary
package, so what is asserted is the process a console script becomes: the argv it
was handed, the exit status it returns, and the sentence a package with no
binary prints instead of a traceback.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

#: `python/tests/test_launcher.py` -> the published package.
SOURCE = Path(__file__).resolve().parents[1] / "blastit"
#: The stand-in prints its argv and exits with a status the launcher must keep.
STUB = "#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 7\n"


class LauncherTest(unittest.TestCase):
    """`python -m blastit` over the real `execvpe` seam."""

    def setUp(self) -> None:
        root = tempfile.TemporaryDirectory()
        self.addCleanup(root.cleanup)
        self.root = Path(root.name)
        self.package = self.root / "blastit"
        self.package.mkdir()
        for module in SOURCE.glob("*.py"):
            shutil.copy2(module, self.package / module.name)
        (self.package / "_bin").mkdir()

    def _environment(self) -> dict[str, str]:
        """A clean environment: no `BLASTIT_*` leak, no `PATH` fallback."""
        environment = dict(os.environ)
        for name in [name for name in environment if name.startswith("BLASTIT_")]:
            del environment[name]
        environment["PYTHONPATH"] = str(self.root)
        environment["PATH"] = ""
        return environment

    def _run(
        self, *arguments: str, mode: int | None = 0o644
    ) -> subprocess.CompletedProcess:
        """Stages the stand-in at `mode` (0644 is what `pip` writes), then runs it."""
        program = self.package / "_bin" / "blastit"
        if mode is not None:
            program.write_text(STUB, encoding="utf-8")
            program.chmod(mode)
        return subprocess.run(
            [sys.executable, "-m", "blastit", *arguments],
            env=self._environment(),
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )

    def test_the_argv_and_the_exit_status_are_the_binary_s(self) -> None:
        # `pip` writes 0644 outside `*.data/scripts/`: the launcher repairs its own
        # `_bin/` mode instead of refusing to start.
        done = self._run("check", "--json")
        self.assertEqual(done.returncode, 7, done.stderr)
        self.assertEqual(done.stdout.split(), ["check", "--json"])

    def test_a_package_with_no_binary_names_the_fix(self) -> None:
        done = self._run("--version", mode=None)
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertIn("reinstall", done.stderr)
        self.assertNotIn("Traceback", done.stderr)

    def test_the_language_server_names_the_fix_instead_of_looping(self) -> None:
        """`blastit-lsp` with an empty `_bin/`: one sentence, never an `exec` loop.

        In a venv the missing program's fallback is the console script that is
        running, so this is where a second `exec` would turn into a relaunch per
        interpreter: the guard and the sentence are the oracle.
        """
        done = subprocess.run(
            [
                sys.executable,
                "-c",
                "from blastit.__main__ import main_lsp; main_lsp()",
            ],
            env=self._environment(),
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertIn("pip install --force-reinstall blastit", done.stderr, done.stderr)
        self.assertNotIn("Traceback", done.stderr)


if __name__ == "__main__":
    unittest.main()
