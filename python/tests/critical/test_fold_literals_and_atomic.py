"""Protected critical tier: `fold` refuses inlined values and writes atomically.

`blastit-manifest fold` turns `sql.denied` lines into manifest grants (D3c). Two
properties of that are security boundaries and are pinned here (R25, R29):

* a statement that still holds a value — an inline quoted literal, or the `…`
  dbproxy redacts an inlined value to — is **refused**. A grant is committed,
  so writing it would commit whatever the application put in the query; the
  spec's two protocol setup statements are the only literals allowed. The
  refusal is not reachable with `--allow-dangerous`: that flag grants
  dangerous *kinds*, never a leaked value.
* `--write` goes through `.{name}.{pid}.tmp` + `fsync` + `os.replace`, and an
  error leaves neither a half-written manifest nor a stray `.tmp` next to it.

The allowed path is tested too: a parameterised statement is written, the
result parses, and the command exits `0` even when other statements in the same
log were refused (an unreadable input is the only `1`). The standard suite
(`python/tests/test_fold.py`) covers the planning in depth; this file is the
wall.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import stat
import tempfile
import tomllib
import unittest
from pathlib import Path
from typing import Any
from unittest import mock

from blastit import fold

MANIFEST = """\
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "notes"
path = ['/notes/']
[route.db]
queries = ["SELECT 1"]
"""


def classify_plain(_sql: str) -> str:
    """The parser's verdict for a plain, single `SELECT`."""
    return json.dumps(
        {"class": "select", "label": "SELECT", "plain": True, "statements": 1}
    )


def describe_nothing(_sql: str) -> str:
    """The analysis for a statement that calls no function."""
    return json.dumps({"functions": []})


def denial(sql: str) -> fold._Denial:
    """One `sql.denied` line's worth of a denial on the manifest's route."""
    return fold._Denial(route="notes", proxy="db", sql=sql, reason="not granted")


def planned(sql: str, *, allow_dangerous: bool = False) -> fold._Addition:
    """Plan one denial against [`MANIFEST`], with the parser and walk stubbed."""
    return fold.plan(
        tomllib.loads(MANIFEST),
        [denial(sql)],
        prefer_tables=False,
        allow_dangerous=allow_dangerous,
        classify=classify_plain,
        describe=describe_nothing,
    )[0]


class TestLiteralRefusal(unittest.TestCase):
    """A value in the statement is never written to the manifest (R25)."""

    def test_a_quoted_literal_is_refused(self) -> None:
        addition = planned("SELECT id FROM notes WHERE body = 'secret'")
        self.assertTrue(addition.is_empty(), addition)
        self.assertIn("inlines a quoted literal", addition.notes[0])

    def test_all_eight_postgresql_literal_forms_are_refused_and_redacted(self) -> None:
        for literal in [
            "'hunter2'",
            r"E'abc\'hunter2'",
            r"E'abc\\\'hunter2'",
            "B'1'",
            "X'ff'",
            "N'hunter2'",
            r"U&'\0041hunter2'",
            "$$hunter2$$",
            "$t$hunter2$t$",
            "$hunter2$s3cr3t$hunter2$",
        ]:
            sql = f"SELECT id FROM notes WHERE body = {literal}"
            addition = planned(sql)
            self.assertTrue(addition.is_empty(), sql)
            note = addition.notes[0]
            self.assertIn("inlines a quoted literal", note, sql)
            self.assertIn("bind the value as a parameter", note, sql)
            self.assertIn("SELECT id FROM notes WHERE body = …", note, sql)
            self.assertNotIn("hunter2", note, sql)
            self.assertNotIn("s3cr3t", note, sql)
            self.assertNotIn("0041", note, sql)
            redacted = fold._redact_literal(sql)
            self.assertEqual(redacted, "SELECT id FROM notes WHERE body = …", sql)
            self.assertEqual(fold._redact_literal(redacted), redacted, sql)

    def test_a_redacted_value_is_refused(self) -> None:
        addition = planned("SELECT id FROM notes WHERE body = …")
        self.assertTrue(addition.is_empty(), addition)
        self.assertIn("holds `…`", addition.notes[0])
        self.assertIn("parameter", addition.notes[0])

    def test_the_dangerous_flag_does_not_grant_a_value(self) -> None:
        for sql in [
            "SELECT id FROM notes WHERE body = 'secret'",
            r"SELECT id FROM notes WHERE body = E'abc\'hunter2'",
            r"SELECT id FROM notes WHERE body = U&'\0041hunter2'",
            "SELECT id FROM notes WHERE body = $t$hunter2$t$",
            "SELECT …",
        ]:
            addition = planned(sql, allow_dangerous=True)
            self.assertTrue(addition.is_empty(), sql)
            self.assertIn("refused", addition.notes[0], sql)
            self.assertNotIn("hunter2", addition.notes[0], sql)
            self.assertNotIn("secret", addition.notes[0], sql)

    def test_a_dollar_quoted_literal_is_refused(self) -> None:
        for sql in [
            "SELECT id FROM notes WHERE body = $tag$secret$tag$",
            "SELECT id FROM notes WHERE body = $é$secret$é$",
        ]:
            addition = planned(sql)
            self.assertTrue(addition.is_empty(), addition)
            self.assertIn("inlines a quoted literal", addition.notes[0])
            self.assertNotIn("secret", addition.notes[0])

    def test_a_literal_in_a_comment_or_an_identifier_is_not_one(self) -> None:
        for sql in [
            "SELECT id FROM notes -- 'this is a comment'",
            "SELECT id FROM notes /* outer /* inner */ 'still a comment' */",
            'SELECT "it\'s" FROM notes',
            'SELECT U&"quoted_ident" FROM notes',
            "SELECT a$b$c FROM notes",
        ]:
            addition = planned(sql)
            self.assertEqual([item for item, _ in addition.queries], [sql], sql)
            self.assertEqual(fold._redact_literal(sql), sql, sql)
        # Negative twin: in SQL identifiers `""` escapes `"`, never `\"`, so a
        # trailing backslash in `"col\"` must not hide a following `'hunter2'`.
        hostile = "SELECT \"col\\\", 'hunter2' FROM notes"
        refused = planned(hostile)
        self.assertTrue(refused.is_empty(), refused)
        self.assertNotIn("hunter2", refused.notes[0])
        self.assertEqual(fold._redact_literal(hostile), 'SELECT "col\\", … FROM notes')
        self.assertIn(repr('SELECT "col\\", … FROM notes'), refused.notes[0])

    def test_the_two_protocol_setup_statements_keep_their_literals(self) -> None:
        for sql in [
            "SET TIME ZONE 'UTC'",
            "SELECT set_config('TimeZone', $?, false)",
        ]:
            addition = planned(sql, allow_dangerous=True)
            self.assertEqual([item for item, _ in addition.queries], [sql], sql)
            self.assertEqual(fold._redact_literal(sql), sql, sql)

    def test_a_parameterised_statement_is_the_allowed_path(self) -> None:
        sql = "SELECT id FROM notes WHERE body = $?"
        addition = planned(sql)
        self.assertEqual([item for item, _ in addition.queries], [sql])
        self.assertEqual(addition.notes, [], "nothing to explain")

    def test_the_refusal_note_never_echoes_the_value(self) -> None:
        for sql in [
            "SELECT id FROM notes WHERE body = 'hunter2'",
            r"SELECT id FROM notes WHERE body = E'abc\'hunter2'",
            r"SELECT id FROM notes WHERE body = U&'\0041hunter2'",
            "SELECT id FROM notes WHERE body = $t$hunter2$t$",
        ]:
            addition = planned(sql)
            self.assertTrue(addition.is_empty(), addition)
            self.assertNotIn("hunter2", addition.notes[0], "the value is never printed")
            self.assertNotIn("0041", addition.notes[0], "unicode escapes are scrubbed")
            self.assertIn("SELECT id FROM notes WHERE body = …", addition.notes[0])
            self.assertIn("bind the value as a parameter", addition.notes[0])

    def test_an_unparseable_statement_never_echoes_the_offending_token(self) -> None:
        def fail_parse(_sql: str) -> str:
            msg = 'Invalid statement: unterminated quoted string at or near "\'hunter2"'
            raise ValueError(msg)

        for dangerous in (False, True):
            addition = fold.plan(
                tomllib.loads(MANIFEST),
                [denial("SELECT 'hunter2")],
                prefer_tables=False,
                allow_dangerous=dangerous,
                classify=fail_parse,
                describe=describe_nothing,
            )[0]
            self.assertTrue(addition.is_empty(), addition)
            self.assertIn("unreadable", addition.notes[0])
            self.assertNotIn("hunter2", addition.notes[0])

    def test_a_multi_statement_text_is_refused(self) -> None:
        addition = fold.plan(
            tomllib.loads(MANIFEST),
            [denial("SELECT 1; DELETE FROM notes")],
            prefer_tables=False,
            classify=lambda _sql: json.dumps(
                {
                    "class": "select",
                    "label": "2 statements",
                    "plain": False,
                    "statements": 2,
                    "reason": "the text holds more than one statement",
                }
            ),
            describe=describe_nothing,
        )[0]
        self.assertTrue(addition.is_empty(), "a grant is one statement")
        self.assertIn("2 statements", addition.notes[0])
        self.assertIn("a grant is one statement", addition.notes[0])
        self.assertIn("split the text", addition.notes[0])

    def test_an_unsafe_function_is_refused_without_the_flag(self) -> None:
        sql = "SELECT unaccent($1), st_distance($1, $2)"
        addition = fold.plan(
            tomllib.loads(MANIFEST),
            [denial(sql)],
            prefer_tables=False,
            classify=classify_plain,
            describe=lambda _sql: json.dumps(
                {"functions": ["unaccent", "st_distance"]}
            ),
        )[0]
        self.assertTrue(addition.is_empty(), addition)
        self.assertIn("unaccent, st_distance", addition.notes[0])
        self.assertIn("outside blastit's safe set", addition.notes[0])
        self.assertIn("--allow-dangerous", addition.notes[0])


class TestFoldCli(unittest.TestCase):
    """Exit codes and the file on disk: `0` when it read the input, `1` when it did not."""

    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.manifest = self.root / "blastit.toml"
        self.manifest.write_text(MANIFEST, encoding="utf-8")
        self.log = self.root / "app.log"

    def run_fold(self, *args: str) -> tuple[int, str, str]:
        """Run the console script's spelling, returning code + stdout + stderr."""
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = fold.main(["fold", *args, "--manifest", str(self.manifest)])
        return code, out.getvalue(), err.getvalue()

    def write_log(self, *statements: str) -> None:
        lines = [
            '{"event":"request","method":"GET","path":"/notes/","route":"notes"}',
            *(
                json.dumps(
                    {
                        "event": "sql.denied",
                        "route": "notes",
                        "proxy": "db",
                        "sql": statement,
                        "reason": "not granted",
                    }
                )
                for statement in statements
            ),
        ]
        self.log.write_text("\n".join(lines) + "\n", encoding="utf-8")

    def test_the_happy_path_writes_a_parsable_diff_and_no_temp_file(self) -> None:
        self.write_log("SELECT id FROM notes WHERE id = $?")
        code, _, err = self.run_fold(str(self.log), "--write")
        self.assertEqual(code, 0, err)
        written = self.manifest.read_text(encoding="utf-8")
        self.assertIn('"SELECT id FROM notes WHERE id = $?"', written)
        parsed = tomllib.loads(written)
        self.assertEqual(
            parsed["route"][0]["db"]["queries"],
            ["SELECT 1", "SELECT id FROM notes WHERE id = $?"],
        )
        self.assertEqual(list(self.root.glob("*.tmp")), [], "no temp file survives")
        self.assertEqual(
            list(self.root.glob(".*.tmp")), [], "no hidden temp file either"
        )

    def test_readable_input_with_refusals_exits_zero(self) -> None:
        self.write_log(
            "SELECT id FROM notes WHERE body = 'secret'",
            "SELECT id FROM notes WHERE body = …",
        )
        code, _, err = self.run_fold(str(self.log), "--write")
        self.assertEqual(code, 0, err)
        self.assertIn("inlines a quoted literal", err)
        self.assertIn("holds `…`", err)
        self.assertEqual(
            self.manifest.read_text(encoding="utf-8"), MANIFEST, "nothing was written"
        )

    def test_the_refusal_note_on_stderr_never_echoes_the_value(self) -> None:
        self.write_log(
            "SELECT id FROM notes WHERE body = 'hunter2'",
            r"SELECT id FROM notes WHERE body = E'abc\'hunter2'",
            r"SELECT id FROM notes WHERE body = E'abc\'hunter2' AND id = $?",
            r"SELECT id FROM notes WHERE body = U&'\0041hunter2'",
            "SELECT id FROM notes WHERE body = $t$hunter2$t$",
            "SELECT id FROM notes WHERE body = B'1'",
            "SELECT id FROM notes WHERE body = X'ff'",
            "SELECT id FROM notes WHERE body = N'hunter2'",
            "SELECT id FROM notes WHERE body = $$hunter2$$",
        )
        code, out, err = self.run_fold(str(self.log))
        self.assertEqual(code, 0, err)
        self.assertEqual(
            out, "", "no TOML diff is emitted when every denial is refused"
        )
        self.assertNotIn("hunter2", err, "a value from a log never reaches stderr")
        self.assertNotIn("0041", err, "unicode escape payload never reaches stderr")
        self.assertNotIn("unreadable", err, "every statement parsed as SQL")

        code, out, err = self.run_fold(str(self.log), "--write")
        self.assertEqual(code, 0, err)
        self.assertEqual(out, "")
        self.assertIn("inlines a quoted literal", err)
        self.assertNotIn("unreadable", err, "E-string + $? placeholder parsed cleanly")
        self.assertIn(
            "SELECT id FROM notes WHERE body = …",
            err,
            "the literal token and prefix are redacted to a single ellipsis",
        )
        self.assertIn(
            "SELECT id FROM notes WHERE body = … AND id = $?",
            err,
            "trailing placeholders after a backslash-escaped E-string stay intact",
        )
        self.assertIn("bind the value as a parameter", err)
        self.assertNotIn("hunter2", err, "a value from a log never reaches stderr")
        self.assertNotIn("0041", err, "unicode escape payload never reaches stderr")
        self.assertEqual(
            self.manifest.read_text(encoding="utf-8"), MANIFEST, "nothing was written"
        )

    def test_write_preserves_a_0600_manifest_mode(self) -> None:
        os.chmod(self.manifest, 0o600)
        self.write_log("SELECT id FROM notes WHERE id = $?")
        code, _, err = self.run_fold(str(self.log), "--write")
        self.assertEqual(code, 0, err)
        self.assertEqual(
            stat.S_IMODE(self.manifest.stat().st_mode),
            0o600,
            "--write must never widen a restricted manifest's permissions",
        )
        parsed = tomllib.loads(self.manifest.read_text(encoding="utf-8"))
        self.assertEqual(
            parsed["route"][0]["db"]["queries"],
            ["SELECT 1", "SELECT id FROM notes WHERE id = $?"],
        )

    def test_an_unreadable_log_exits_one(self) -> None:
        code, _, err = self.run_fold(str(self.root / "missing.log"), "--write")
        self.assertEqual(code, 1)
        self.assertIn("missing.log", err)
        self.assertEqual(
            self.manifest.read_text(encoding="utf-8"), MANIFEST, "nothing was written"
        )

    def test_an_unreadable_manifest_exits_one(self) -> None:
        self.write_log("SELECT id FROM notes WHERE id = $?")
        self.manifest.unlink()
        code, _, err = self.run_fold(str(self.log), "--write")
        self.assertEqual(code, 1)
        self.assertIn("blastit.toml", err)

    def test_a_failed_replace_leaves_the_manifest_and_no_temp_file(self) -> None:
        self.write_log("SELECT id FROM notes WHERE id = $?")
        with mock.patch.object(
            fold.os, "replace", side_effect=OSError("disk full")
        ) as replaced:
            code, _, err = self.run_fold(str(self.log), "--write")
        self.assertEqual(code, 1, err)
        self.assertIn("could not write the edit", err)
        self.assertEqual(
            self.manifest.read_text(encoding="utf-8"), MANIFEST, "nothing was written"
        )
        source = replaced.call_args.args[0]
        (
            self.assertEqual(source.name, f".{self.manifest.name}.{os.getpid()}.tmp"),
            "the documented temp-file name",
        )
        self.assertFalse(source.exists(), "the temp file was cleaned up")
        self.assertEqual(list(self.root.glob("*tmp*")), [])


class TestTempFileName(unittest.TestCase):
    """The temp file is a sibling named `.{name}.{pid}.tmp`, never elsewhere (R29)."""

    def test_the_write_goes_through_the_documented_sibling(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "blastit.toml"
            path.write_text("version = 1\n", encoding="utf-8")
            seen: list[Any] = []
            real_replace = os.replace

            def recording_replace(source: Any, target: Any) -> None:
                seen.append((Path(source), Path(target)))
                real_replace(source, target)

            with mock.patch.object(fold.os, "replace", side_effect=recording_replace):
                fold._write_atomic(path, "version = 2\n")
            self.assertEqual(len(seen), 1)
            source, target = seen[0]
            self.assertEqual(target, path)
            self.assertEqual(source.name, f".blastit.toml.{os.getpid()}.tmp")
            self.assertEqual(source.parent, path.parent)
            self.assertFalse(source.exists())
            self.assertEqual(path.read_text(encoding="utf-8"), "version = 2\n")

    def test_a_planted_temp_symlink_is_never_written_through(self) -> None:
        """Anyone who can write the directory can predict the temp name (R29)."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "blastit.toml"
            path.write_text("version = 1\n", encoding="utf-8")
            victim = root / "victim.txt"
            victim.write_text("the author's own file\n", encoding="utf-8")
            planted = root / f".blastit.toml.{os.getpid()}.tmp"
            planted.symlink_to(victim)

            fold._write_atomic(path, "version = 2\n")

            self.assertEqual(
                victim.read_text(encoding="utf-8"),
                "the author's own file\n",
                "a planted symlink is never written through",
            )
            self.assertEqual(path.read_text(encoding="utf-8"), "version = 2\n")
            self.assertFalse(planted.exists(), "the temp file is gone, not a symlink")
            self.assertFalse(planted.is_symlink(), "the planted link was not followed")
            self.assertEqual(
                sorted(entry.name for entry in root.iterdir()),
                ["blastit.toml", "victim.txt"],
                "no .tmp is left beside the manifest",
            )

    def test_a_0600_manifest_keeps_its_mode_before_and_after_replace(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "blastit.toml"
            path.write_text("version = 1\n", encoding="utf-8")
            os.chmod(path, 0o600)
            temp_modes: list[int] = []
            real_replace = os.replace

            def recording_replace(source: Any, target: Any) -> None:
                temp_modes.append(stat.S_IMODE(Path(source).stat().st_mode))
                real_replace(source, target)

            with mock.patch.object(fold.os, "replace", side_effect=recording_replace):
                fold._write_atomic(path, "version = 2\n")
            self.assertEqual(temp_modes, [0o600], "temp file is 0600 before replace")
            self.assertEqual(
                stat.S_IMODE(path.stat().st_mode),
                0o600,
                "replaced manifest stays 0600",
            )
            self.assertEqual(path.read_text(encoding="utf-8"), "version = 2\n")


if __name__ == "__main__":
    unittest.main()
