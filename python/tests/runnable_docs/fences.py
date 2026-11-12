"""Parse the `console` fences of the public documentation.

The harness never copies a command into a second file: it reads the Markdown the
developer reads, splits each fence into the commands (`$ …`) and the output lines
that follow, and executes those commands in a clean directory. Adding, removing
or editing a fence therefore changes what runs, and the inventory is only the
map from a fence to *how* it runs (fixture, verdict policy, a written reason).
"""

from __future__ import annotations

import re
from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class Command:
    """One `$ …` command and the output the page shows for it."""

    line: int
    text: str
    expected: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class Fence:
    """One triple-backtick `console` fence."""

    page: str
    index: int
    line: int
    body: str

    def commands(self) -> tuple[Command, ...]:
        return tuple(_commands(self.body, self.line + 1))


_FENCE = re.compile(
    r"^```[ \t]*console[ \t]*\n(.*?)^```[ \t]*$", re.DOTALL | re.MULTILINE
)
_HEREDOC = re.compile(r"<<-?[ \t]*['\"]?([A-Za-z_][A-Za-z0-9_]*)['\"]?")


def console_fences(page: str, text: str) -> list[Fence]:
    """Every `console` fence of one page, numbered from 1 in reading order."""
    fences: list[Fence] = []
    for index, match in enumerate(_FENCE.finditer(text), start=1):
        line = text.count("\n", 0, match.start()) + 1
        fences.append(Fence(page, index, line, match.group(1).rstrip("\n")))
    return fences


def _commands(body: str, first_line: int) -> list[Command]:
    """Split a fence body into commands and their expected output."""
    lines = body.splitlines()
    commands: list[Command] = []
    at = 0
    pending: list[str] = []
    while at < len(lines):
        line = lines[at]
        if not line.startswith("$ "):
            pending.append(line)
            at += 1
            continue
        command = [line[2:]]
        start = first_line + at
        at += 1
        while at < len(lines) and command[-1].rstrip().endswith("\\"):
            command.append(lines[at])
            at += 1
        heredoc = _HEREDOC.search(command[-1])
        if heredoc is not None:
            marker = heredoc.group(1)
            while at < len(lines) and lines[at].strip() != marker:
                command.append(lines[at])
                at += 1
            if at < len(lines):
                command.append(lines[at])
                at += 1
        expected: list[str] = []
        while at < len(lines) and not lines[at].startswith("$ "):
            expected.append(lines[at])
            at += 1
        while expected and not expected[0].strip():
            expected.pop(0)
        while expected and not expected[-1].strip():
            expected.pop()
        commands.append(Command(start, "\n".join(command), tuple(expected)))
    if pending and not commands and any(line.strip() for line in pending):
        # An output-only fence (a log line or a JSON document) has no command.
        return []
    return commands


def public_pages(zensical_toml: str, readme: str) -> list[str]:
    """README plus every documentation page the site's navigation names."""
    import tomllib

    document = tomllib.loads(zensical_toml)
    navigation = document["project"]["nav"]
    pages = [readme]

    def walk(entry: object) -> None:
        if isinstance(entry, str):
            pages.append("docs/" + entry)
        elif isinstance(entry, dict):
            for value in entry.values():
                walk(value)
        elif isinstance(entry, list):
            for value in entry:
                walk(value)

    walk(navigation)
    return pages
