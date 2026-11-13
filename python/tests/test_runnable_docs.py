"""Tests for the runnable-docs gate itself.

`make docs-test` executes 38 snippets and proves, one page at a time, that the
docs and the product agree. What it could not prove is that it *sees* every
snippet: `python/tests/runnable_docs/**` had no test of its own, so the parser,
the page list and the inventory were only ever exercised by a full gate run, and
five documented fence shapes (`~~~`, four backticks, an indented block, an info
suffix, a `bash` block with a prompt) plus a published page the nav does not name
were all silent passes (`docs/testing/AUDIT-MA.3.md`, `F1`/`F3`/`F5`/`F9`/`F10`).

These are not mocks of the harness: each test writes synthetic Markdown in the
shape it is about, hands it to the real `fences`/`run` functions, and asserts
that the gate goes red or stays green. Two of them are end-to-end: the page list
of a throwaway repository tree (orphan and excluded pages) and the real
inventory, so a page that drifts out of `inventory.toml` fails here as well as in
`make docs-test` — without a wheel build.

Run them with:

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_runnable_docs.py -q
```
"""

from __future__ import annotations

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "runnable_docs"))

import fences
import run
from harness import FenceSpec, Outcome, PageSpec, Report, load_inventory

BACKTICKS = "`" * 3
REASON = "Accounts for this synthetic page; the gate checks the inventory only."

#: Every spelling of a `console` block a page may use.
SHAPES: dict[str, dict[str, str]] = {
    "plain": {},
    "tilde": {"marker": "~~~"},
    "four backticks": {"marker": "````"},
    "two-space indented": {"indent": "  "},
    "three-space indented": {"indent": "   "},
    "info suffix": {"info": 'console title="start it"'},
    "tilde with an info suffix": {"marker": "~~~", "info": 'console title="x"'},
    "indented tilde with an info suffix": {
        "marker": "~~~~",
        "indent": "   ",
        "info": 'console title="start it"',
    },
}


def block(
    body: str,
    *,
    marker: str = BACKTICKS,
    info: str = "console",
    indent: str = "",
) -> str:
    """One fenced block written the way a page may write it."""
    lines = [indent + marker + info]
    lines.extend(indent + line if line else "" for line in body.splitlines())
    lines.append(indent + marker)
    return "\n".join(lines)


def page(*blocks: str) -> str:
    """A page that shows one or more blocks, separated by prose."""
    return "\n\nProse between the blocks.\n\n".join(blocks) + "\n"


class FenceShapeTest(unittest.TestCase):
    """Every spelling of a `console` block yields the same command."""

    def test_every_shape_yields_its_command(self) -> None:
        for name, shape in SHAPES.items():
            with self.subTest(shape=name):
                text = page(block("$ blastit dev\nlistening on 127.0.0.1", **shape))
                found = fences.console_fences("docs/shape.md", text)
                self.assertEqual(len(found), 1)
                self.assertEqual(found[0].index, 1)
                self.assertEqual(found[0].line, 1)
                commands = found[0].commands()
                self.assertEqual(len(commands), 1)
                self.assertEqual(commands[0].text, "blastit dev")
                self.assertEqual(commands[0].expected, ("listening on 127.0.0.1",))
                blocks, problems = fences.scan_page("docs/shape.md", text)
                self.assertEqual(problems, [])
                self.assertEqual(len(blocks), 1)

    def test_mixed_shapes_are_numbered_in_reading_order(self) -> None:
        text = page(
            block("$ blastit dev\ndev"),
            block("$ blastit check\nok", marker="~~~", info='console title="ci"'),
            block("$ blastit serve\nup", marker="````", indent="  "),
        )
        found = fences.console_fences("docs/mixed.md", text)
        self.assertEqual([fence.index for fence in found], [1, 2, 3])
        self.assertEqual([fence.line for fence in found], [1, 8, 15])
        self.assertEqual(
            [fence.commands()[0].text for fence in found],
            ["blastit dev", "blastit check", "blastit serve"],
        )

    def test_a_non_console_block_is_not_a_fence(self) -> None:
        text = page(
            block('name = "shop"', info='toml title="blastit.toml"'),
            block("body { margin: 0 }", marker="~~~", info="css"),
        )
        self.assertEqual(fences.console_fences("docs/other.md", text), [])
        blocks, problems = fences.scan_page("docs/other.md", text)
        self.assertEqual(problems, [])
        self.assertEqual([block.language for block in blocks], ["toml", "css"])

    def test_a_shorter_marker_inside_a_longer_block_is_body(self) -> None:
        # A four-backtick block is how a page shows a three-backtick one, so the
        # `text` block stays one block. Its `$ ` line is still a command a reader
        # would copy, and it is red for that reason alone.
        text = page(
            block(
                "Show a block like this:\n\n```console\n$ not a command\n```\n",
                marker="````",
                info="text",
            )
        )
        self.assertEqual(fences.console_fences("docs/nested.md", text), [])
        blocks, problems = fences.scan_page("docs/nested.md", text)
        self.assertEqual(len(blocks), 1)
        self.assertEqual(len(problems), 1)
        self.assertIn("label the block `console`", problems[0])

    def test_a_heredoc_command_keeps_its_body(self) -> None:
        text = page(
            block(
                "$ cat > blastit.toml <<'TOML'\n"
                "[project]\n"
                'name = "shop"\n'
                "TOML\n"
                "$ blastit check\n"
                "ok"
            )
        )
        commands = fences.console_fences("docs/heredoc.md", text)[0].commands()
        self.assertEqual(len(commands), 2)
        self.assertEqual(
            commands[0].text,
            "cat > blastit.toml <<'TOML'\n[project]\nname = \"shop\"\nTOML",
        )
        self.assertEqual(commands[0].expected, ())
        self.assertEqual(commands[1].text, "blastit check")
        self.assertEqual(commands[1].expected, ("ok",))


class SilentCommandTest(unittest.TestCase):
    """The five shapes that would otherwise hide a command are red."""

    def test_an_unclosed_block_is_red(self) -> None:
        text = "```console\n$ blastit dev\nlistening\n"
        _, problems = fences.scan_page("docs/open.md", text)
        self.assertEqual(len(problems), 1)
        self.assertIn("has no closing marker", problems[0])
        self.assertIn("docs/open.md:1", problems[0])

    def test_a_marker_inside_a_console_body_is_red(self) -> None:
        text = page("```console\n$ blastit dev\n```console\n$ blastit check\n```\n")
        blocks, problems = fences.scan_page("docs/dup.md", text)
        self.assertEqual(len(blocks), 1)
        self.assertEqual(len(problems), 1)
        self.assertIn("fence marker inside the console block", problems[0])
        self.assertIn("docs/dup.md:3", problems[0])

    def test_a_prompt_in_a_non_console_block_is_red(self) -> None:
        text = page(block("$ uv run blastit check\nok", info="bash"))
        _, problems = fences.scan_page("docs/bash.md", text)
        self.assertEqual(len(problems), 1)
        self.assertIn("label the block `console`", problems[0])

    def test_a_prompt_outside_every_block_is_red(self) -> None:
        text = (
            "```console\n$ blastit dev\n```\n\n"
            "The next one is not fenced at all:\n\n"
            "$ uv run blastit serve --preset nope\n"
        )
        _, problems = fences.scan_page("docs/loose.md", text)
        self.assertEqual(len(problems), 1)
        self.assertIn("outside any block", problems[0])
        self.assertIn("docs/loose.md:7", problems[0])

    def test_a_console_fence_without_prompts_stays_green(self) -> None:
        text = page(block("Starting blastit\nlistening on 127.0.0.1"))
        _, problems = fences.scan_page("docs/output.md", text)
        self.assertEqual(problems, [])


class ReasonFloorTest(unittest.TestCase):
    """An accounted fence has to say what the gate checks instead."""

    def scan(self, reason: str | None) -> list[str]:
        specs = (FenceSpec(index=1, mode="static", reason=reason),)
        return run.scan("docs/deploy.md", "", specs)

    def test_a_written_reason_is_green(self) -> None:
        self.assertEqual(self.scan(REASON), [])
        self.assertEqual(self.scan(None), [])

    def test_one_word_reasons_are_red(self) -> None:
        for reason in ("deploy", "Docker", "see above"):
            with self.subTest(reason=reason):
                problems = self.scan(reason)
                self.assertEqual(len(problems), 1)
                self.assertIn("too short", problems[0])

    def test_placeholders_are_red(self) -> None:
        for reason in ("TODO", "todo", "n/a", "N/A", "-"):
            with self.subTest(reason=reason):
                problems = self.scan(reason)
                self.assertEqual(len(problems), 1)
                self.assertIn("placeholder", problems[0])

    def test_a_blank_reason_is_red(self) -> None:
        for reason in ("", " ", "\n\t "):
            with self.subTest(reason=reason):
                problems = self.scan(reason)
                self.assertEqual(len(problems), 1)
                self.assertIn("blank", problems[0])


class ReportTest(unittest.TestCase):
    """The verdict tally counts what ran and what was only accounted for."""

    def test_counts_and_exit_code(self) -> None:
        report = Report()
        report.add(Outcome("docs/a.md", 1, "static", True, False, "accounted"))
        report.add(Outcome("docs/a.md", 2, "dev", True, True, "ran"))
        report.add(Outcome("docs/b.md", 1, "dev", False, True, "broke"))
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            code = run.report_result(report, 12.5)
        printed = stream.getvalue()
        self.assertEqual(code, 1)
        self.assertIn("3 fences: 2 executed, 1 accounted, 1 failed", printed)
        self.assertIn("FAIL  docs/b.md  fence 1", printed)

    def test_a_clean_run_exits_zero(self) -> None:
        report = Report()
        report.add(Outcome("docs/a.md", 1, "dev", True, True, "ran"))
        report.add(Outcome("docs/a.md", 2, "example", True, False, "accounted"))
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            code = run.report_result(report, 30.0)
        self.assertEqual(code, 0)
        self.assertIn("2 fences: 1 executed, 1 accounted, 0 failed", stream.getvalue())


class PageListTest(unittest.TestCase):
    """The page list is what the site publishes, not what its menu names."""

    ZENSICAL = """[project]
docs_dir = "docs"

nav = [{ "Home" = "index.md" }]

exclude_docs = '''
/design/
/secret.md
*.tmp.md
!keep.tmp.md
'''
"""

    def build(self, files: dict[str, str]) -> Path:
        holder = tempfile.TemporaryDirectory()
        self.addCleanup(holder.cleanup)
        root = Path(holder.name)
        (root / "zensical.toml").write_text(self.ZENSICAL, encoding="utf-8")
        for name, text in files.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
        return root

    def test_published_pages_include_an_orphan_and_drop_the_excluded(self) -> None:
        root = self.build(
            {
                "README.md": page(block("$ blastit dev\nok")),
                "docs/index.md": page(block("$ blastit dev\nok")),
                "docs/orphan.md": page(block("$ blastit check\nok")),
                "docs/design/notes.md": page(block("$ blastit dev\nok")),
                "docs/secret.md": page(block("$ blastit dev\nok")),
                "docs/notes.tmp.md": page(block("$ blastit dev\nok")),
                "docs/keep.tmp.md": page(block("$ blastit dev\nok")),
            }
        )
        pages = fences.public_pages(
            root, (root / "zensical.toml").read_text(encoding="utf-8")
        )
        self.assertEqual(
            pages,
            [
                "README.md",
                "docs/index.md",
                "docs/keep.tmp.md",
                "docs/orphan.md",
            ],
        )

    def test_an_orphan_page_without_an_entry_is_red(self) -> None:
        root = self.build(
            {
                "README.md": page(block("$ blastit dev\nok")),
                "docs/index.md": page(block("$ blastit dev\nok")),
                "docs/orphan.md": page(block("$ blastit check\nok")),
                "docs/design/notes.md": "```console\n$ unclosed on an excluded page\n",
            }
        )
        spec = (FenceSpec(index=1, mode="static", reason=REASON),)
        pages = [PageSpec("README.md", spec), PageSpec("docs/index.md", spec)]
        problems = run.check_inventory(pages, root)
        self.assertEqual(len(problems), 1)
        self.assertIn("docs/orphan.md", problems[0])
        self.assertIn("has no entry", problems[0])

        pages.append(PageSpec("docs/orphan.md", spec))
        self.assertEqual(run.check_inventory(pages, root), [])

    def test_an_entry_for_an_excluded_page_is_red(self) -> None:
        root = self.build(
            {
                "README.md": page(block("$ blastit dev\nok")),
                "docs/index.md": page(block("$ blastit dev\nok")),
                "docs/design/notes.md": page(block("$ blastit dev\nok")),
            }
        )
        spec = (FenceSpec(index=1, mode="static", reason=REASON),)
        pages = [
            PageSpec("README.md", spec),
            PageSpec("docs/index.md", spec),
            PageSpec("docs/design/notes.md", spec),
        ]
        problems = run.check_inventory(pages, root)
        self.assertEqual(len(problems), 1)
        self.assertIn("is not a page the site publishes", problems[0])

    def test_a_snippet_shape_in_the_repository_is_red(self) -> None:
        root = self.build(
            {
                "README.md": "```console\n$ blastit dev\n",
                "docs/index.md": page(block("$ blastit dev\nok")),
            }
        )
        spec = (FenceSpec(index=1, mode="static", reason=REASON),)
        pages = [PageSpec("README.md", spec), PageSpec("docs/index.md", spec)]
        problems = run.check_inventory(pages, root)
        self.assertEqual(len(problems), 1)
        self.assertIn("has no closing marker", problems[0])


class RepositoryInventoryTest(unittest.TestCase):
    """The repository's own inventory is exactly the gate's precondition."""

    def test_the_real_inventory_is_green(self) -> None:
        problems = run.check_inventory(load_inventory(run.INVENTORY))
        self.assertEqual(problems, [])


if __name__ == "__main__":
    unittest.main()
