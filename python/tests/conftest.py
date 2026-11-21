"""The package under test is not installed: the suite imports it from `python/`.

`python/tests/test_launcher.py` copies `blastit/` into a throwaway tree and runs
each module as a subprocess, but the gate's own tests (`test_check_pytest_gate`,
`critical/`) import `blastit` directly, so the source root goes on the path here
rather than requiring `pip install` or a `PYTHONPATH` on every invocation.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
