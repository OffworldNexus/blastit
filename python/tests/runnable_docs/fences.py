"""Parse the fenced code blocks of the public documentation.

The harness never copies a command into a second file: it reads the Markdown the
developer reads, splits each `console` block into the commands (`$ …`) and the
output lines that follow, and executes those commands in a clean directory.
Adding, removing or editing a fence therefore changes what runs, and the
inventory is only the map from a fence to *how* it runs (fixture, verdict
policy, a written reason).

Recognition is the page's, not a regex's. Every spelling of a fenced block a
Markdown page can use is a block: three or more backticks or tildes, any
indentation (a list item or an admonition holds one), and an info string after
the language (```` ```console title="start it" ````). A block is dedented by its
own indentation, so an indented `console` block still yields its `$ ` commands.

What recognition cannot account for is not ignored. `scan_page` reports the
shapes that would otherwise hide a command behind prose — a block that never
closes, a fence marker left inside a `console` body, a `$ ` prompt in a block
that is not `console`, a `$ ` prompt outside every block — so the inventory
check goes red instead of quiet.
"""

from __future__ import annotations

import fnmatch
import re
import tomllib
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True, slots=True)
class Command:
    """One `$ …` command and the output the page shows for it."""

    line: int
    text: str
    expected: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class Fence:
    """One `console` block, numbered from 1 in reading order."""

    page: str
    index: int
    line: int
    body: str

    def commands(self) -> tuple[Command, ...]:
        return tuple(_commands(self.body, self.line + 1))


@dataclass(frozen=True, slots=True)
class Block:
    """One fenced code block, whatever its marker, language or indentation."""

    page: str
    line: int
    marker: str
    info: str
    body: str
    closed: bool
    end: int

    @property
    def language(self) -> str:
        """The language an info string starts with: `console title="…"` -> console."""
        return self.info.split(None, 1)[0] if self.info.strip() else ""


# An opening marker: optional indentation, three or more identical backticks or
# tildes, then the info string the page writes after the language.
_OPEN = re.compile(r"^(?P<indent>[ \t]*)(?P<marker>`{3,}|~{3,})(?P<info>.*)$")
# A line that looks like a fence marker wherever it appears, dedented or not.
_MARKER = re.compile(r"^[ \t]*(?:`{3,}|~{3,})")
# A documented command. `_commands` only splits `$ ` at the start of a line, so
# this is the same shape: anything else is output that a reader would copy as a
# command and the gate would never run.
_PROMPT = re.compile(r"^[ \t]*\$ ")
_HEREDOC = re.compile(r"<<-?[ \t]*['\"]?([A-Za-z_][A-Za-z0-9_]*)['\"]?")


def console_fences(page: str, text: str) -> list[Fence]:
    """Every `console` block of one page, numbered from 1 in reading order."""
    blocks = [
        block for block in scan_page(page, text)[0] if block.language == "console"
    ]
    return [
        Fence(page, index, block.line, block.body)
        for index, block in enumerate(blocks, start=1)
    ]


def scan_page(page: str, text: str) -> tuple[list[Block], list[str]]:
    """Every fenced block of one page, plus the shapes that hide a command.

    One pass, left to right: a line outside a block opens one; inside a block
    only a matching closing marker ends it, so a `console` body is never split by
    a line that merely looks like a marker — that line is reported instead.
    """
    blocks: list[Block] = []
    problems: list[str] = []
    lines = text.splitlines()
    at = 0
    while at < len(lines):
        opener = _OPEN.match(lines[at])
        if opener is None:
            if _PROMPT.match(lines[at]):
                problems.append(
                    f"{page}:{at + 1}: a `$ ` command outside any block; the block "
                    "around it is missing, or an earlier line closed it early"
                )
            at += 1
            continue
        marker = opener.group("marker")
        indent = len(opener.group("indent"))
        opened = at + 1
        at += 1
        body: list[str] = []
        closed = False
        while at < len(lines):
            if _closes(lines[at], marker):
                closed = True
                break
            body.append(_dedent(lines[at], indent))
            at += 1
        block = Block(
            page=page,
            line=opened,
            marker=marker,
            info=opener.group("info").strip(),
            body="\n".join(body).rstrip("\n"),
            closed=closed,
            end=at + 1 if closed else len(lines),
        )
        blocks.append(block)
        problems.extend(_block_problems(block))
        if closed:
            at += 1
    return blocks, problems


def _closes(line: str, marker: str) -> bool:
    """Is this line the closing marker for a block opened with `marker`?"""
    run = re.match(r"^[ \t]*([`~]+)[ \t]*$", line)
    if run is None:
        return False
    text = run.group(1)
    return len(set(text)) == 1 and text[0] == marker[0] and len(text) >= len(marker)


def _dedent(line: str, width: int) -> str:
    """Remove up to `width` leading whitespace characters from one body line."""
    cut = 0
    while cut < width and cut < len(line) and line[cut] in " \t":
        cut += 1
    return line[cut:]


def _block_problems(block: Block) -> list[str]:
    """The shapes of one block that would hide a documented command."""
    language = block.language
    problems: list[str] = []
    if not block.closed:
        problems.append(
            f"{block.page}:{block.line}: the {block.marker}{language} block opened "
            "here has no closing marker; everything after it is never read"
        )
    for offset, line in enumerate(block.body.splitlines()):
        number = block.line + 1 + offset
        if language == "console":
            if _MARKER.match(line):
                problems.append(
                    f"{block.page}:{number}: a fence marker inside the console block "
                    f"opened at line {block.line}; the block was truncated or "
                    "duplicated, and the commands after this line are read as output"
                )
        elif _PROMPT.match(line):
            problems.append(
                f"{block.page}:{number}: a `$ ` command inside a "
                f"{language or 'plain'} block; label the block `console` so the "
                "gate runs this command"
            )
    return problems


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


def public_pages(
    repo: Path, zensical_toml: str, readme: str = "README.md"
) -> list[str]:
    """`README.md` plus every Markdown page the site publishes.

    The list is the *published* tree, not the navigation. A page under
    `docs_dir` that `exclude_docs` does not exclude is built and served by
    `zensical build` even when no nav entry names it, so it has to be
    inventoried (and executed) like any other page; the nav is a menu, not the
    boundary of what a reader can reach.
    """
    project = tomllib.loads(zensical_toml)["project"]
    docs_dir = str(project.get("docs_dir", "docs")).strip("/")
    patterns = _exclude_patterns(str(project.get("exclude_docs", "")))
    root = repo / docs_dir
    pages = [readme]
    if root.is_dir():
        for path in sorted(root.rglob("*.md")):
            relative = path.relative_to(root).as_posix()
            if not _excluded(relative, patterns):
                pages.append(f"{docs_dir}/{relative}")
    return pages


def _exclude_patterns(text: str) -> list[str]:
    return [
        line.strip()
        for line in text.splitlines()
        if line.strip() and not line.strip().startswith("#")
    ]


def _excluded(relative: str, patterns: list[str]) -> bool:
    """Does `exclude_docs` keep this page (relative to `docs_dir`) out of the site?"""
    excluded = False
    for pattern in patterns:
        keep = pattern.startswith("!")
        rule = pattern[1:] if keep else pattern
        if _matches(relative, rule):
            excluded = not keep
    return excluded


def _matches(relative: str, rule: str) -> bool:
    """One `exclude_docs` rule, in the subset Zensical documents.

    A leading `/` anchors the rule at `docs_dir`; a rule that contains a `/`
    anywhere is relative to `docs_dir` too, and a bare name matches a file (or
    a directory) at any depth. A trailing `/` excludes a whole directory.
    `*` and `?` are shell wildcards.
    """
    anchored = rule.startswith("/") or "/" in rule.lstrip("/")
    rule = rule.lstrip("/")
    if not rule:
        return False
    if rule.endswith("/"):
        directory = rule.rstrip("/")
        if anchored:
            return relative == directory or relative.startswith(directory + "/")
        return directory in relative.split("/")[:-1]
    if anchored:
        return fnmatch.fnmatchcase(relative, rule)
    return fnmatch.fnmatchcase(relative.rsplit("/", 1)[-1], rule)
