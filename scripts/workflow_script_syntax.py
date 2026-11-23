"""Check that every `sh -c '…'` / `bash -c '…'` word is well formed.

The workflows and `scripts/*.sh` hand a script to a container as **one
single-quoted word**:

    sh -eu -c '
        ...
    '

A stray `'` inside that word ends it early: the runner then runs the rest as
runner commands and the container receives a truncated script (`syntax error:
unexpected end of file`). That is a quoting bug in the caller, not in the
container's script, so this check reads each workflow and each `scripts/*.sh`
as text, finds every `sh`/`bash` invocation that ends in `-c '`, and reads the
word exactly as the *outer* shell does.

Splices are part of that reading: `scripts/*.sh` interpolate host values into
the word as `…'"${arch}"'…`, i.e. the single-quoted segment closes, a
double-quoted segment follows, and a `'` re-opens it. Such a splice contributes
one placeholder bare word to the body and the word continues; anything else
after a closing `'` (a `"` with no matching `"'`, no closing quote at all) is
reported rather than dropped. The `'` that ends a word must be followed by
something that can end a shell word (whitespace, `\\`, `;`, `|`, `&`, `)`, or
EOF): a `'` followed by anything else — `# the caller's list` — stopped where
the shell could not stop it, and is reported at the line of that quote.

The extracted body is then parsed with `bash -n -c`. bash's grammar is a
superset of sh's, so one check covers both. This is a parser plus a shell
syntax check on the *inner* script, not a lint of the caller's source: nothing
here greps for a pattern it likes.

Stdlib only (run it with `uv run --no-project scripts/workflow_script_syntax.py`),
importable (`shell_words`, `unchecked_c`, `audit`), and side-effect free at
import time.
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import NamedTuple

_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DIR = _ROOT / ".github" / "workflows"
SCRIPTS_DIR = _ROOT / "scripts"
_USAGE = (
    "usage: scripts/workflow_script_syntax.py [WORKFLOW_DIR|FILE]\n"
    "  no argument: .github/workflows plus every scripts/*.sh\n"
    "  a directory: its *.yml and *.yaml; a file: that text\n"
)

# `<sh|bash>` at a word boundary, then any flags/arguments (quoted ones
# included, so `docker run --entrypoint sh "$image" -c '…'` is reached), then
# `-c` and the opening single quote of the container script. The character
# before the shell name must end a command word, so `mybash` and `"sh"` are
# not matches.
_INVOCATION = re.compile(
    r"(?:^|[\s;|&(])(sh|bash)"
    r"(?:[ \t]+(?:\"[^\"\n]*\"|'[^'\n]*'|[^\s'\"|;&()<>]+))*?"
    r"[ \t]+-c[ \t]+'",
    re.MULTILINE,
)

# A shell `-c` whose script is not a single-quoted word: nothing above extracts
# it, so it is reported instead of being passed over in silence.
_UNQUOTED_C = re.compile(
    r"(?:^|[\s;|&(])(sh|bash)"
    r"(?:[ \t]+(?:\"[^\"\n]*\"|'[^'\n]*'|[^\s'\"|;&()<>]+))*?"
    r"[ \t]+-c(?![\t ]*')",
    re.MULTILINE,
)

# What may follow the `'` that ends a word for the outer shell to be able to
# end it there.
_WORD_ENDERS = frozenset(" \t\r\n\\;|&)")

# One bare word stands in for a `'"${host}"'` splice in the inner body.
SPLICE = "SPLICE"


class Word(NamedTuple):
    """One `sh`/`bash -c '…'` word as the outer shell reads it."""

    line: int
    body: str | None
    error: str | None
    error_line: int


def _parse_word(text: str, opening: int, line: int) -> Word:
    """Read the single-quoted word whose opening quote is just past `opening`."""
    pieces: list[str] = []
    index = opening
    while True:
        closing = text.find("'", index)
        if closing < 0:
            return Word(line, None, "unterminated single-quoted `-c` word", line)
        pieces.append(text[index:closing])
        quote_line = text.count("\n", 0, closing) + 1
        after = closing + 1
        if after < len(text) and text[after] == '"':
            stop = text.find('"', after + 1)
            line_end = text.find("\n", after + 1)
            if stop < 0 or 0 <= line_end <= stop:
                return Word(
                    line,
                    None,
                    '`\'"` splice with no closing `"` on the same line',
                    quote_line,
                )
            if stop + 1 >= len(text) or text[stop + 1] != "'":
                return Word(
                    line,
                    None,
                    "`'\"$x\"'` splice whose re-opening `'` is missing",
                    quote_line,
                )
            pieces.append(SPLICE)
            index = stop + 2
            continue
        body = "".join(pieces)
        if after < len(text) and text[after] not in _WORD_ENDERS:
            return Word(
                line,
                body,
                f"stray quote ends the `-c` word early (next character {text[after]!r})",
                quote_line,
            )
        return Word(line, body, None, line)


def shell_words(text: str) -> list[Word]:
    """Every `sh`/`bash -c '…'` word in `text`, in source order."""
    return [
        _parse_word(text, match.end(), text.count("\n", 0, match.start(1)) + 1)
        for match in _INVOCATION.finditer(text)
    ]


def unchecked_c(text: str) -> list[tuple[int, str]]:
    """`(line, message)` for every shell `-c` this checker cannot read."""
    return [
        (
            text.count("\n", 0, match.start(1)) + 1,
            "shell `-c` script is not a single-quoted word (not checked)",
        )
        for match in _UNQUOTED_C.finditer(text)
    ]


def audit(paths: list[Path]) -> tuple[list[str], int]:
    """Every problem across `paths`, and how many shell words were parsed."""
    problems: list[str] = []
    words = 0
    for path in paths:
        text = path.read_text(encoding="utf-8")
        parsed = shell_words(text)
        words += len(parsed)
        for word in parsed:
            where = f"{path}:{word.error_line}"
            if word.error is not None:
                problems.append(f"{where}: {word.error}")
                continue
            result = subprocess.run(
                ["bash", "-n", "-c", word.body or ""],
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode != 0:
                detail = (result.stderr or result.stdout).strip() or "bash -n failed"
                problems.append(f"{path}:{word.line}: {detail}")
        problems.extend(
            f"{path}:{line}: {message}" for line, message in unchecked_c(text)
        )
    return problems, words


def _default_paths() -> list[Path]:
    """Workflows plus every shell script the release tooling ships."""
    workflows = sorted(DEFAULT_DIR.glob("*.yml")) + sorted(DEFAULT_DIR.glob("*.yaml"))
    return workflows + sorted(SCRIPTS_DIR.glob("*.sh"))


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    target: str | None = None
    for arg in args:
        if arg in ("-h", "--help"):
            sys.stdout.write(_USAGE)
            return 0
        if arg.startswith("-"):
            sys.stderr.write(f"workflow-script-syntax: unknown option {arg}\n{_USAGE}")
            return 2
        target = arg
    if shutil.which("bash") is None:
        sys.stderr.write(
            "workflow-script-syntax: bash is required to parse the words\n"
        )
        return 2
    if target is None:
        paths = _default_paths()
    else:
        chosen = Path(target)
        if chosen.is_dir():
            paths = sorted(chosen.glob("*.yml")) + sorted(chosen.glob("*.yaml"))
        elif chosen.is_file():
            paths = [chosen]
        else:
            sys.stderr.write(
                f"workflow-script-syntax: no such file or directory: {target}\n"
            )
            return 2
    if not paths:
        sys.stderr.write(
            f"workflow-script-syntax: no workflow YAML or shell script under {target}\n"
        )
        return 2
    problems, words = audit(paths)
    if problems:
        for problem in problems:
            sys.stderr.write(problem + "\n")
        sys.stderr.write(
            f"workflow-script-syntax: {len(problems)} problem(s) in "
            f"{len(paths)} file(s)\n"
        )
        return 1
    print(f"workflow-script-syntax: ok ({words} shell word(s) in {len(paths)} file(s))")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
