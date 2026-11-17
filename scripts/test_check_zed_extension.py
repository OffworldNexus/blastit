"""Negative self-tests for `scripts/check_zed_extension.py` (MB.22.5, R56).

`make zed-check` runs this beside the guard, because a guard nobody can make
fail is worse than no guard: the extension would install on the day the `rev`
became a branch name, the captures drifted outside §10.1's fifteen names, or a
wasm was committed. Each case below builds a real, faithful copy of
`editors/`, `.zed/settings.json`, `LICENSE` and the `docs/local.md` fence in a
temporary directory, doctors exactly one property, and requires the guard to go
red naming **that** property.

The temp tree is a `git init` + `git add -A` checkout so the guard's
"committed `extension.wasm`/`target/`/`grammars/`" rule is the git fact it is in
the repository: writing an artefact *and staging it* is a commit, writing it
untracked is an install-time leftover. The baseline itself must be green, so a
guard that fails everything cannot pass this file.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
GUARD = HERE / "check_zed_extension.py"

EXTENSION = "editors/zed/extension.toml"
CARGO = "editors/zed/Cargo.toml"
LIB = "editors/zed/src/lib.rs"
GITIGNORE = "editors/zed/.gitignore"
HIGHLIGHTS = "editors/zed/languages/blastfile/highlights.scm"
INDENTS = "editors/zed/languages/blastfile/indents.scm"
OUTLINE = "editors/zed/languages/blastfile/outline.scm"
PARSER = "editors/tree-sitter-blastfile/src/parser.c"
SETTINGS = ".zed/settings.json"
LOCAL_PAGE = "docs/local.md"

#: The guard's properties, by the name each `ok`/`FAIL` line carries. The list
#: is asserted **exactly**, so deleting a property from the guard, renaming one,
#: or adding one without this file fails here — a threshold like "at least 40"
#: would let two properties vanish silently.
EXPECTED_PROPERTIES = (
    "extension: keys",
    "extension: id",
    "extension: schema_version",
    "extension: version",
    "extension: description",
    "grammars: key",
    "grammars: repository",
    "grammars: rev",
    "grammars: path",
    "language_servers: languages",
    "language_servers: language_ids",
    "language_servers: no name",
    "config: name",
    "config: grammar",
    "config: path_suffixes",
    "config: line_comments",
    "config: tab_size",
    "config: hard_tabs",
    "grammar: files",
    "grammar: export",
    "grammar: name",
    "grammar: no scanner",
    "grammar: no externals",
    "grammar: generator pin",
    "highlights: captures",
    "highlights: variable pair",
    "indents: capture",
    "brackets: captures",
    "outline: captures",
    "cargo: workspace",
    "cargo: crate-type",
    "license: inside",
    "tree: no build artifacts",
    "tree: gitignore",
    "lib: which order",
    "lib: argv",
    "lib: shell_env",
    "lib: no process",
    "lib: error string",
    "lib: no capabilities",
    "settings: format_on_save",
    "docs: settings fence",
)


class GuardCase(unittest.TestCase):
    """One fresh copy of the extension tree per case."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        shutil.copytree(
            REPO / "editors",
            self.root / "editors",
            symlinks=True,
            ignore=shutil.ignore_patterns("target", "__pycache__"),
        )
        shutil.copytree(REPO / ".zed", self.root / ".zed")
        shutil.copy2(REPO / "LICENSE", self.root / "LICENSE")
        (self.root / "docs").mkdir()
        shutil.copy2(REPO / "docs" / "local.md", self.root / LOCAL_PAGE)
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        self.stage()

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def stage(self) -> None:
        """Stages every path, so the guard sees the tree as committed."""
        subprocess.run(
            ["git", "-C", str(self.root), "add", "-A"],
            check=True,
            capture_output=True,
        )

    def run_guard(self) -> subprocess.CompletedProcess[str]:
        """The guard, exactly as `make zed-check` runs it, against this tree."""
        return subprocess.run(
            [sys.executable, str(GUARD), "--root", str(self.root)],
            capture_output=True,
            text=True,
            check=False,
        )

    def rewrite(self, relative: str, old: str, new: str) -> None:
        """Replaces the first `old` with `new` in one file."""
        path = self.root / relative
        text = path.read_text(encoding="utf-8")
        self.assertIn(old, text, f"{relative} does not contain {old!r}")
        path.write_text(text.replace(old, new, 1), encoding="utf-8")
        self.stage()

    def assert_red(self, prop: str) -> None:
        """The guard exits 1 and names `prop` on stderr."""
        result = self.run_guard()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f"FAIL {prop}", result.stderr, result.stderr)

    def test_the_repository_passes(self) -> None:
        result = self.run_guard()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("zed-check: OK", result.stdout)
        properties = sorted(
            line[len("ok   ") :]
            for line in result.stdout.splitlines()
            if line.startswith("ok   ")
        )
        self.assertEqual(len(properties), 42, "the guard checks exactly 42 properties")
        self.assertEqual(
            properties,
            sorted(EXPECTED_PROPERTIES),
            "every guard property, by name: a deleted or renamed check fails here",
        )

    def test_branch_shaped_rev_is_red(self) -> None:
        path = self.root / EXTENSION
        text = path.read_text(encoding="utf-8")
        text = re.sub(r'rev = "[^"]*"', 'rev = "release/0.2.0"', text)
        path.write_text(text, encoding="utf-8")
        self.stage()
        self.assert_red("grammars: rev")

    def test_short_rev_is_red(self) -> None:
        path = self.root / EXTENSION
        text = path.read_text(encoding="utf-8")
        text = re.sub(r'rev = "[^"]*"', 'rev = "053b56e"', text)
        path.write_text(text, encoding="utf-8")
        self.stage()
        self.assert_red("grammars: rev")

    def test_description_not_longer_than_name_is_red(self) -> None:
        path = self.root / EXTENSION
        text = path.read_text(encoding="utf-8")
        text = re.sub(r'description = "[^"]*"', 'description = "Blastfile"', text)
        path.write_text(text, encoding="utf-8")
        self.stage()
        self.assert_red("extension: description")

    def test_missing_workspace_is_red(self) -> None:
        self.rewrite(CARGO, "\n[workspace]\n", "\n")
        self.assert_red("cargo: workspace")

    def test_path_suffixes_typo_is_red(self) -> None:
        self.rewrite(
            "editors/zed/languages/blastfile/config.toml",
            'path_suffixes = ["Blastfile", "blastfile"]',
            'path_suffixes = ["Blastfile"]',
        )
        self.assert_red("config: path_suffixes")

    def test_missing_language_ids_is_red(self) -> None:
        self.rewrite(
            EXTENSION,
            '\n[language_servers.blastit-lsp.language_ids]\nBlastfile = "blastfile"\n',
            "\n",
        )
        self.assert_red("language_servers: language_ids")

    def test_capture_outside_allowlist_is_red(self) -> None:
        path = self.root / HIGHLIGHTS
        path.write_text(
            path.read_text(encoding="utf-8") + "\n(comment) @variable.builtin\n",
            encoding="utf-8",
        )
        self.stage()
        self.assert_red("highlights: captures")

    def test_missing_indent_is_red(self) -> None:
        (self.root / INDENTS).write_text("; no indent capture\n", encoding="utf-8")
        self.stage()
        self.assert_red("indents: capture")

    def test_missing_item_is_red(self) -> None:
        (self.root / OUTLINE).write_text("(app_decl) @name\n", encoding="utf-8")
        self.stage()
        self.assert_red("outline: captures")

    def test_unexported_grammar_is_red(self) -> None:
        self.rewrite(PARSER, "tree_sitter_blastfile", "tree_sitter_other")
        self.assert_red("grammar: export")

    def test_committed_wasm_is_red(self) -> None:
        wasm = self.root / "editors" / "zed" / "extension.wasm"
        wasm.write_bytes(b"\0asm")
        # `-f`: `.gitignore` matches it, and a commit of an ignored path is
        # exactly the mistake this case is about (the install writes it there).
        subprocess.run(
            ["git", "-C", str(self.root), "add", "-f", "editors/zed/extension.wasm"],
            check=True,
            capture_output=True,
        )
        self.assert_red("tree: no build artifacts")

    def test_two_shell_env_calls_is_red(self) -> None:
        path = self.root / LIB
        path.write_text(
            path.read_text(encoding="utf-8")
            + "\n// a second shell_env() is a second env\n",
            encoding="utf-8",
        )
        self.stage()
        self.assert_red("lib: shell_env")

    def test_swapped_which_order_is_red(self) -> None:
        path = self.root / LIB
        text = path.read_text(encoding="utf-8")
        text = text.replace('which("blastit")', 'which("TMP")')
        text = text.replace('which("uv")', 'which("blastit")')
        text = text.replace('which("TMP")', 'which("uv")')
        path.write_text(text, encoding="utf-8")
        self.stage()
        self.assert_red("lib: which order")

    def test_settings_off_is_red(self) -> None:
        path = self.root / SETTINGS
        document = json.loads(path.read_text(encoding="utf-8"))
        document["languages"]["Blastfile"]["format_on_save"] = "off"
        path.write_text(json.dumps(document), encoding="utf-8")
        self.stage()
        self.assert_red("settings: format_on_save")

    def test_missing_license_is_red(self) -> None:
        licence = self.root / "editors" / "zed" / "LICENSE"
        licence.unlink()
        self.stage()
        self.assert_red("license: inside")

    def test_license_escaping_the_repository_is_red(self) -> None:
        outside = (
            Path(self.temporary.name).parent
            / f"{Path(self.temporary.name).name}-license"
        )
        outside.write_text("not the project's license\n", encoding="utf-8")
        self.addCleanup(outside.unlink)
        licence = self.root / "editors" / "zed" / "LICENSE"
        licence.unlink()
        licence.symlink_to(outside)
        self.stage()
        self.assert_red("license: inside")

    def test_docs_fence_off_is_red(self) -> None:
        self.rewrite(LOCAL_PAGE, '"format_on_save": "on"', '"format_on_save": "off"')
        self.assert_red("docs: settings fence")

    def test_a_missing_file_is_red_not_a_traceback(self) -> None:
        (self.root / GITIGNORE).unlink()
        self.stage()
        self.assert_red("tree: gitignore")


if __name__ == "__main__":
    unittest.main()
