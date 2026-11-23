"""Check that every `sh -c '…'` / `bash -c '…'` word in a workflow is well formed.

A workflow hands a script to a container as **one single-quoted word**:

    sh -eu -c '
        ...
    '

A stray `'` inside that word ends it early: the runner then runs the rest as
runner commands and the container receives a truncated script (`syntax error:
unexpected end of file`). That is a quoting bug in the workflow, not in the
container's script, so this check reads each workflow as text, finds every
`sh`/`bash` invocation that ends in `-c '`, takes the text between that opening
quote and the next `'` — exactly the word the outer shell sees — and parses it
with `bash -n -c`. bash's grammar is a superset of sh's, so one check covers
both, and an unterminated word is reported at its own `file:line`.

This is a parser plus shell-syntax check on the *inner* script, not a lint of
the YAML source: nothing here greps for a pattern it likes.

Stdlib only (run it with `uv run --no-project scripts/workflow_script_syntax.py`),
importable (`shell_words`, `audit`), and side-effect free at import time.
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
from pathlib import Path

DEFAULT_DIR = Path(__file__).resolve().parent.parent / ".github" / "workflows"
_USAGE = "usage: scripts/workflow_script_syntax.py [WORKFLOW_DIR]\n"

# `<sh|bash>` at a word boundary, any flags (and `-o` values) before `-c`, then
# the opening single quote of the container script. The character before the
# shell name must end a command word, so `mybash` and `"sh"` are not matches.
_INVOCATION = re.compile(
    r"(?:^|[\s;|&(])(sh|bash)(?:[ \t]+[^\s'\"]+)*?[ \t]+-c[ \t]+'",
    re.MULTILINE,
)


def shell_words(text: str) -> list[tuple[int, str | None]]:
    """`(line, body)` for every `sh`/`bash -c '…'` word in `text`.

    `body` is `None` when the word has no closing quote, which is exactly the
    truncation this check exists for.
    """
    words: list[tuple[int, str | None]] = []
    for match in _INVOCATION.finditer(text):
        line = text.count("\n", 0, match.start(1)) + 1
        opening = match.end()
        closing = text.find("'", opening)
        words.append((line, None if closing < 0 else text[opening:closing]))
    return words


def audit(paths: list[Path]) -> tuple[list[str], int]:
    """Every problem across `paths`, and how many shell words were parsed."""
    problems: list[str] = []
    words = 0
    for path in paths:
        for line, body in shell_words(path.read_text(encoding="utf-8")):
            words += 1
            where = f"{path}:{line}"
            if body is None:
                problems.append(f"{where}: unterminated single-quoted `-c` word")
                continue
            result = subprocess.run(
                ["bash", "-n", "-c", body],
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode != 0:
                detail = (result.stderr or result.stdout).strip() or "bash -n failed"
                problems.append(f"{where}: {detail}")
    return problems, words


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    target = DEFAULT_DIR
    for arg in args:
        if arg in ("-h", "--help"):
            sys.stdout.write(_USAGE)
            return 0
        if arg.startswith("-"):
            sys.stderr.write(f"workflow-script-syntax: unknown option {arg}\n{_USAGE}")
            return 2
        target = Path(arg)
    if shutil.which("bash") is None:
        sys.stderr.write(
            "workflow-script-syntax: bash is required to parse the words\n"
        )
        return 2
    paths = sorted(Path(target).glob("*.yml")) + sorted(Path(target).glob("*.yaml"))
    if not paths:
        sys.stderr.write(f"workflow-script-syntax: no workflow YAML under {target}\n")
        return 2
    problems, words = audit(paths)
    if problems:
        for problem in problems:
            sys.stderr.write(problem + "\n")
        sys.stderr.write(
            f"workflow-script-syntax: {len(problems)} bad shell word(s) in "
            f"{len(paths)} workflow(s)\n"
        )
        return 1
    print(
        f"workflow-script-syntax: ok ({words} shell word(s) in {len(paths)} workflow(s))"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
