"""Negative self-tests for `scripts/check_zed_extension.py` (MB.22.5, R56).

`make zed-check` runs this beside the guard, because a guard nobody can make
fail is worse than no guard: the extension would install on the day the `rev`
became a branch name, the captures drifted outside §10.1's fifteen names, or a
wasm was committed. Each case below builds a real, faithful copy of
`editors/`, `.zed/settings.json`, `LICENSE` and the `docs/local.md` fence in a
temporary directory, doctors exactly one property, and requires the guard's
`FAIL` lines to be **exactly** the set that mutation must produce -- one, or the
two whose causes genuinely overlap, written down per case.

The temp tree is a `git init` + `git add -A` checkout so the guard's
"committed `extension.wasm`/`target/`/`grammars/`" rule is the git fact it is in
the repository: writing an artefact *and staging it* is a commit, writing it
untracked is an install-time leftover. The baseline itself must be green, so a
guard that fails everything cannot pass this file.

Where this host has the pinned generator (`pinned_parse`), every run passes
`--require-parse`, so the §9.2a loop must really parse all twenty-three
`Blastfile`s for a case to pass; on a host without it the tests fall back with
the guard, so CI stays green and the fallback stays honest.
"""

from __future__ import annotations

import json
import os
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
GRAMMAR_HIGHLIGHTS = "editors/tree-sitter-blastfile/highlights.scm"
INDENTS = "editors/zed/languages/blastfile/indents.scm"
OUTLINE = "editors/zed/languages/blastfile/outline.scm"
PARSER = "editors/tree-sitter-blastfile/src/parser.c"
GRAMMAR_JS = "editors/tree-sitter-blastfile/grammar.js"
SETTINGS = ".zed/settings.json"
LOCAL_PAGE = "docs/local.md"

#: The cache hit `check_zed_extension.py` pins, carried here a second time on
#: purpose: these cases mutate *this* literal, so a guard whose own copy is
#: weakened no longer matches `src/lib.rs` and every cache case goes red. It is
#: also the whole point of `lib: cache` — a real early return, not a probe.
CACHE_HIT = "if std::fs::metadata(&command).is_ok_and(|entry| entry.is_file()) {"

#: The directories the guard's §9.2a parse loop covers. The temp tree must hold
#: the same `Blastfile`s: "grammar: fixture count" is red when the loop finds
#: fewer than the design's twenty-three, and a baseline that is red for a
#: missing fixture would make every other case here meaningless.
BLASTFILE_DIRS = (
    "editors/tree-sitter-blastfile/test/fixtures",
    "e2e",
    "python/tests",
)


def pinned_parse() -> bool:
    """Whether this host has the pinned generator, so the guard can be strict.

    `check_zed_extension.py` discovers the binary itself; this repeats its
    candidate order (`$BLASTIT_TREE_SITTER`, the design's
    `/tmp/opencode/bin/tree-sitter`, then `PATH`) and its pin, so these tests
    pass `--require-parse` exactly when the guard will find the binary and never
    when it will not. The guard's own `note` line cannot answer that question:
    it also appears when the binary is there but the grammar does not load (the
    unexported-symbol case), and reading it as "no binary" was how the first
    draft of this helper let `test_unexported_grammar_is_red` skip the strict
    path it was supposed to exercise.
    """
    try:
        package = json.loads(
            (REPO / "editors" / "tree-sitter-blastfile" / "package.json").read_text(
                encoding="utf-8"
            )
        )
        pin = package["devDependencies"]["tree-sitter-cli"]
    except (OSError, ValueError, KeyError, TypeError):
        return False
    for candidate in (
        os.environ.get("BLASTIT_TREE_SITTER"),
        "/tmp/opencode/bin/tree-sitter",
        shutil.which("tree-sitter"),
    ):
        if not candidate:
            continue
        try:
            version = subprocess.run(
                [candidate, "--version"], capture_output=True, text=True, check=False
            )
        except OSError:
            continue
        words = version.stdout.split()
        if version.returncode == 0 and words[:1] == ["tree-sitter"] and pin in words:
            return True
    return False


#: Whether the guard is run with `--require-parse` in this process. One probe,
#: so every case is strict on a host that has the pin and falls back on one that
#: does not (reviewer finding 2).
PINNED_PARSE = pinned_parse()

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
    "extension: capabilities",
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
    "grammar: sql_string",
    "grammar: fixture count",
    "grammar: fixtures parse",
    "highlights: captures",
    "highlights: variable pair",
    "highlights: sql captures",
    "highlights: copies agree",
    "indents: capture",
    "brackets: captures",
    "outline: captures",
    "cargo: workspace",
    "cargo: crate-type",
    "license: inside",
    "tree: no build artifacts",
    "tree: gitignore",
    "lib: order",
    "lib: which order",
    "lib: arguments",
    "lib: no uv",
    "lib: shell_env",
    "lib: env allowlist",
    "lib: no process",
    "lib: platform",
    "lib: asset map",
    "lib: zip windows",
    "lib: executable",
    "lib: download destination",
    "lib: cache",
    "lib: status",
    "lib: failure",
    "lib: asset url",
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
        for relative in BLASTFILE_DIRS:
            base = REPO / relative
            if not base.is_dir():
                continue
            for source in sorted(base.rglob("*")):
                if not source.is_file():
                    continue
                if source.name != "Blastfile" and not source.name.endswith(
                    ".Blastfile"
                ):
                    continue
                target = self.root / source.relative_to(REPO)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, target)
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

    def invoke(self, *extra: str) -> subprocess.CompletedProcess[str]:
        """The guard binary against this tree, with extra argv."""
        return subprocess.run(
            [sys.executable, str(GUARD), "--root", str(self.root), *extra],
            capture_output=True,
            text=True,
            check=False,
        )

    def run_guard(self) -> subprocess.CompletedProcess[str]:
        """The guard as `make zed-check` runs it -- strict when the pin exists.

        `--require-parse` is passed when `PINNED_PARSE` found the pinned
        generator, so on a host with the pin (this one) a green baseline proves
        §9.2a's loop really ran instead of reading a `note`, while a host
        without it keeps the local fallback and still gets a green baseline
        (reviewer finding 2).
        """
        return self.invoke("--require-parse") if PINNED_PARSE else self.invoke()

    def rewrite(self, relative: str, old: str, new: str) -> None:
        """Replaces the first `old` with `new` in one file."""
        path = self.root / relative
        text = path.read_text(encoding="utf-8")
        self.assertIn(old, text, f"{relative} does not contain {old!r}")
        path.write_text(text.replace(old, new, 1), encoding="utf-8")
        self.stage()

    def assert_red(self, *props: str) -> None:
        """The guard exits 1 and its `FAIL` lines are exactly `props`.

        Asserting only "contains" let a case claim a property was proven while
        three others went red beside it, so every twin names the *complete* set
        it must produce (reviewer finding 3): one mutation, one property -- or,
        where two properties genuinely share the mutation, both, written down.
        """
        result = self.run_guard()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(
            self.failed_properties(result), set(props), result.stdout + result.stderr
        )

    def failed_properties(self, result: subprocess.CompletedProcess[str]) -> set[str]:
        """The property names the guard wrote a `FAIL` line for.

        Matched against the known names, not split on `:`, because a name holds
        a colon itself and a message may hold another ("FAIL grammars: rev: rev
        'x' must be one 40-hex commit"). A `FAIL <name>:` line is the exact
        shape `Guard.fail` writes.
        """
        return {
            name for name in EXPECTED_PROPERTIES if f"FAIL {name}:" in result.stderr
        }

    def parsed_here(self) -> bool:
        """Whether this host has the pinned generator, so a parse case can run."""
        return PINNED_PARSE

    def test_the_repository_passes(self) -> None:
        result = self.run_guard()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("zed-check: OK", result.stdout)
        if self.parsed_here():
            # Strict mode was in force, so a green baseline means the 23-file
            # loop ran: no `note` may stand in for the parse.
            self.assertNotIn("note grammar: fixtures parse:", result.stdout)
        properties = sorted(
            line[len("ok   ") :]
            for line in result.stdout.splitlines()
            if line.startswith("ok   ")
        )
        self.assertEqual(len(properties), 59, "the guard checks exactly 59 properties")
        self.assertEqual(
            properties,
            sorted(EXPECTED_PROPERTIES),
            "every guard property, by name: a deleted or renamed check fails here",
        )

    def test_a_missing_pin_is_red_under_require_parse(self) -> None:
        # Reviewer finding 2: without the flag the loop's absence is a `note`,
        # with it the same tree is in red -- so a CI without the dev tool keeps
        # its green `make check`, and every caller that knows the pin is there
        # can insist the loop really ran.
        package = self.root / "editors" / "tree-sitter-blastfile" / "package.json"
        text = package.read_text(encoding="utf-8")
        pinned = re.search(r'"tree-sitter-cli": "([^"]+)"', text)
        self.assertIsNotNone(pinned, text)
        package.write_text(text.replace(pinned.group(1), "0.0.0", 1), encoding="utf-8")
        self.stage()
        result = self.invoke("--require-parse")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(
            self.failed_properties(result),
            {"grammar: fixtures parse"},
            result.stdout + result.stderr,
        )
        self.assertIn("--require-parse", result.stderr)

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

    def test_missing_sql_capture_is_red(self) -> None:
        # Two properties, because one deletion loses the capture in Zed *and*
        # leaves the grammar package's copy claiming it: the subset rule fires
        # too, which is the drift this pair of files invites.
        self.rewrite(HIGHLIGHTS, "(sql_keyword) @keyword\n", "")
        self.assert_red("highlights: sql captures", "highlights: copies agree")

    def test_a_grammar_copy_capture_zed_lacks_is_red(self) -> None:
        # The grammar package's copy travels beside `node-types.json`; a capture
        # it invents that Zed's copy does not carry is exactly the drift two
        # files produce (this one still promised `'…'` coloured a `query` after
        # the grammar had split `sql_string` out of `raw_string`).
        path = self.root / GRAMMAR_HIGHLIGHTS
        path.write_text(
            path.read_text(encoding="utf-8") + "\n(sql_number) @number\n",
            encoding="utf-8",
        )
        self.stage()
        self.assert_red("highlights: copies agree")

    def test_query_back_in_the_keyword_rule_is_red(self) -> None:
        # §9.2a decision 1: with `query` in the shared `keyword` rule the
        # one-token `raw_string` always wins and `sql_string` never fires, so the
        # guard refuses the grammar even though every node still exists.
        self.rewrite(
            GRAMMAR_JS, "'delete',\n      'body'", "'delete', 'query',\n      'body'"
        )
        self.assert_red("grammar: sql_string")

    def test_a_required_query_value_is_red(self) -> None:
        # MB.34.6's reviewer finding 1: `query` is a legal name, so dropping the
        # `optional` makes five `blastit fmt`-accepted files ERROR nodes. Only
        # the binary-dependent corpus would notice on a host with the pin, so the
        # guard pins the shape in `grammar.js` itself -- exactly one property.
        self.rewrite(
            GRAMMAR_JS,
            "seq('query', optional(choice($.sql_string, $.string)))",
            "seq('query', choice($.sql_string, $.string))",
        )
        self.assert_red("grammar: sql_string")

    def test_a_stale_node_types_json_is_red(self) -> None:
        # `node-types.json` is generator output: if it does not name the node, the
        # parser was not regenerated from the amended `grammar.js`.
        self.rewrite(
            "editors/tree-sitter-blastfile/src/node-types.json",
            '"type": "sql_keyword"',
            '"type": "sql_keyword_stale"',
        )
        self.assert_red("grammar: sql_string")

    def test_a_broken_blastfile_is_red(self) -> None:
        # The §9.2a "no false red" loop: a stray `}` is an ERROR node, which is
        # exactly what a `query '…'` that the manifest accepts must never be.
        if not self.parsed_here():
            self.skipTest("no pinned tree-sitter on this host, so the loop cannot run")
        broken = (
            self.root
            / "editors"
            / "tree-sitter-blastfile"
            / "test"
            / "fixtures"
            / "app.Blastfile"
        )
        broken.write_text(
            broken.read_text(encoding="utf-8") + "\n}\n", encoding="utf-8"
        )
        self.stage()
        self.assert_red("grammar: fixtures parse")

    def test_missing_indent_is_red(self) -> None:
        (self.root / INDENTS).write_text("; no indent capture\n", encoding="utf-8")
        self.stage()
        self.assert_red("indents: capture")

    def test_missing_item_is_red(self) -> None:
        (self.root / OUTLINE).write_text("(app_decl) @name\n", encoding="utf-8")
        self.stage()
        self.assert_red("outline: captures")

    def test_unexported_grammar_is_red(self) -> None:
        # Two properties: the export is gone, and under `--require-parse` the
        # loop cannot run either -- the grammar builds, the `.so` has no
        # `tree_sitter_blastfile`, and `parse` refuses it. That second red is the
        # point of finding 2: a silent skip would have hidden a grammar Zed
        # cannot load.
        self.rewrite(PARSER, "tree_sitter_blastfile", "tree_sitter_other")
        self.assert_red("grammar: export", "grammar: fixtures parse")

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
        # `blastit-lsp` is the portable binary and wins over `blastit`: a user
        # with both installed must get the better one.
        path = self.root / LIB
        text = path.read_text(encoding="utf-8")
        text = text.replace('which("blastit-lsp")', 'which("TMP")')
        text = text.replace('which("blastit")', 'which("blastit-lsp")')
        text = text.replace('which("TMP")', 'which("blastit")')
        path.write_text(text, encoding="utf-8")
        self.stage()
        self.assert_red("lib: which order")

    def test_a_dropped_path_step_is_red(self) -> None:
        # §8.3a's order with the `$PATH` step removed: a developer who owns a
        # binary would be sent to the network for one they already have.
        self.rewrite(
            LIB,
            "        if let Some(command) = from_path(worktree) {\n"
            "            return Ok(command);\n"
            "        }\n",
            "",
        )
        self.assert_red("lib: order")

    def test_settings_after_path_is_red(self) -> None:
        # The explicit path is the user's decision, so it must be read first: a
        # settings-writing developer whose `PATH` also holds a server would
        # silently get the wrong one.
        path = self.root / LIB
        text = path.read_text(encoding="utf-8")
        settings_step = (
            "        if let Some(command) = from_settings(worktree) {\n"
            "            return Ok(command);\n"
            "        }\n"
        )
        path_step = (
            "        if let Some(command) = from_path(worktree) {\n"
            "            return Ok(command);\n"
            "        }\n"
        )
        self.assertIn(settings_step + path_step, text)
        path.write_text(
            text.replace(settings_step + path_step, path_step + settings_step, 1),
            encoding="utf-8",
        )
        self.stage()
        self.assert_red("lib: order")

    def test_a_uv_step_is_red(self) -> None:
        # The owner's defect: `uv` in the editor path. One line brings it back.
        self.rewrite(
            LIB,
            "fn from_path(worktree: &zed::Worktree) -> Option<zed::Command> {\n",
            "fn from_path(worktree: &zed::Worktree) -> Option<zed::Command> {\n"
            '    let _uv = worktree.which("uv");\n',
        )
        self.assert_red("lib: no uv")

    def test_a_wrong_arguments_vector_is_red(self) -> None:
        # `blastit` on `PATH` is the production server and needs `lsp`; the
        # portable binary takes nothing.
        self.rewrite(LIB, 'vec!["lsp".to_string()]', "Vec::new()")
        self.assert_red("lib: arguments")

    def test_a_wide_env_allowlist_is_red(self) -> None:
        # `SYSTEMROOT` is not decoration: without it a Windows child cannot
        # resolve its own DLLs. Dropping a name from the list is the mutation.
        self.rewrite(
            LIB,
            'const ENV_ALLOWLIST: [&str; 2] = ["PATH", "SYSTEMROOT"];',
            'const ENV_ALLOWLIST: [&str; 1] = ["PATH"];',
        )
        self.assert_red("lib: env allowlist")

    def test_a_misspelt_asset_is_red(self) -> None:
        # One character in the asset name is a 404 on the user's machine, which
        # is the failure mode no local test can see.
        self.rewrite(
            LIB,
            "blastit-lsp-x86_64-apple-darwin.tar.gz",
            "blastit-lsp-x86_64-darwin.tar.gz",
        )
        self.assert_red("lib: asset map")

    def test_a_windows_gzip_asset_is_red(self) -> None:
        # `GzipTar` on Windows is a `.zip` the host refuses to extract; the
        # type and the extension are one decision.
        path = self.root / LIB
        text = path.read_text(encoding="utf-8")
        doctored, count = re.subn(
            r"(blastit-lsp-x86_64-pc-windows-msvc\.zip\",\s*"
            r"file_type: zed::DownloadedFileType::)Zip",
            r"\1GzipTar",
            text,
        )
        self.assertEqual(count, 1, doctored)
        path.write_text(doctored, encoding="utf-8")
        self.stage()
        self.assert_red("lib: zip windows")

    def test_a_cfg_gated_make_file_executable_is_red(self) -> None:
        # The call is unconditional because it is a no-op off Unix: a `cfg!`
        # beside it is the second code path §8.3a removes.
        self.rewrite(
            LIB,
            "    if let Err(error) = zed::make_file_executable(&command) {",
            "    let _ = cfg!(unix);\n"
            "    if let Err(error) = zed::make_file_executable(&command) {",
        )
        self.assert_red("lib: executable")

    def test_an_absolute_download_destination_is_red(self) -> None:
        # `download_file` takes a *directory* relative to the extension's work
        # directory; an absolute path writes outside it.
        self.rewrite(
            LIB,
            'const EXTRACT_DIR: &str = "blastit-lsp";',
            'const EXTRACT_DIR: &str = "/blastit-lsp";',
        )
        self.assert_red("lib: download destination")

    def test_no_cache_check_is_red(self) -> None:
        # Without the early return every editor start re-downloads the release,
        # and an offline machine gets no server at all.
        self.rewrite(LIB, CACHE_HIT, "if false {")
        self.assert_red("lib: cache")

    def test_a_release_lookup_before_the_cache_is_red(self) -> None:
        # The MAJOR this case exists for: `latest_github_release` in front of
        # the cache hit puts GitHub on the path of a binary that is already
        # unpacked, so an offline or rate-limited start fails with `Failed(…)`
        # and no server. The mutation is the real inversion — the cache block
        # moves below the release lookup.
        path = self.root / LIB
        text = path.read_text(encoding="utf-8")
        start = text.find(CACHE_HIT)
        self.assertNotEqual(start, -1, f"{LIB} does not contain {CACHE_HIT!r}")
        start -= len("    ")
        end = text.find("        });\n    }\n", start)
        self.assertNotEqual(end, -1, "the cache block is not the expected shape")
        block = text[start : end + len("        });\n    }\n")]
        without = text[:start] + text[start + len(block) :]
        release = without.find("RELEASE_OPTIONS) {")
        self.assertNotEqual(release, -1)
        after = without.find("\n    };\n", release)
        self.assertNotEqual(after, -1)
        at = after + len("\n    };\n")
        path.write_text(without[:at] + block + without[at:], encoding="utf-8")
        self.stage()
        self.assert_red("lib: cache")

    def test_a_missing_downloading_status_is_red(self) -> None:
        # The developer must see "downloading", not an editor that hangs.
        self.rewrite(
            LIB,
            "    zed::set_language_server_installation_status(\n"
            "        id,\n"
            "        &zed::LanguageServerInstallationStatus::Downloading,\n"
            "    );\n",
            "",
        )
        self.assert_red("lib: status")

    def test_a_foreign_argument_is_red(self) -> None:
        # `blastit-lsp` reads stdin only: an arm that grows an argument still
        # passes every other property, so this one must be the whitelist.
        self.rewrite(
            LIB,
            "        args: Vec::new(),",
            '        args: vec!["--stdio".to_string()],',
        )
        self.assert_red("lib: arguments")

    def test_a_binary_env_is_red(self) -> None:
        # §8.3a ignores the settings' env map: it is the one loader-hijack
        # vector (`LD_PRELOAD`, `DYLD_INSERT_LIBRARIES`) a settings file could
        # add, and the server reads no environment at all.
        self.rewrite(
            LIB,
            "    let args = binary.arguments.unwrap_or_default();",
            "    let args = binary.arguments.unwrap_or_default();\n"
            "    let _ = binary.env;",
        )
        self.assert_red("lib: env allowlist")

    def test_a_failure_without_the_two_halves_is_red(self) -> None:
        # A Mac on an unbuilt target must be told which machines have a server
        # and which setting bypasses the download.
        self.rewrite(LIB, '; {FAILURE_HINT}")', '; ")')
        self.assert_red("lib: failure")

    def test_a_cdn_host_is_red(self) -> None:
        # The download gate is checked before the redirect, so naming the CDN is
        # both useless and a wider permission.
        path = self.root / LIB
        path.write_text(
            path.read_text(encoding="utf-8")
            + "\n// release-assets.githubusercontent.com\n",
            encoding="utf-8",
        )
        self.stage()
        self.assert_red("lib: asset url")

    def test_a_declared_capability_is_red(self) -> None:
        # Two properties, because the regression arrives twice: the parsed
        # manifest grows a block (`extension: capabilities`) and the text grep
        # over `editors/zed/**` finds it (`lib: no capabilities`). The host
        # never consults the manifest for `download_file`, so a declared host
        # fails closed -- the empty manifest is the property.
        path = self.root / EXTENSION
        path.write_text(
            path.read_text(encoding="utf-8")
            + '\n[[capabilities]]\nkind = "download_file"\n'
            + 'host = "github.com"\npath = ["**"]\n',
            encoding="utf-8",
        )
        self.stage()
        self.assert_red("extension: capabilities", "lib: no capabilities")

    def test_a_wrong_platform_arm_is_red(self) -> None:
        # A Mac arm that names the wrong architecture downloads the wrong
        # binary, and every static check except the platform table still passes.
        self.rewrite(
            LIB,
            "(zed::Os::Mac, zed::Architecture::Aarch64)",
            "(zed::Os::Mac, zed::Architecture::X8664)",
        )
        self.assert_red("lib: platform")

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
