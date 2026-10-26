"""`blastit-manifest fold`, and the manifest edit it writes (D3c).

The planning is pure — a manifest document, a list of denials, and a `covering`
function that is a stub here and the compiled extension in `test_the_extension_…`
— so what each test checks is one decision: which statements become grants,
which are skipped, and what the TOML ends up looking like.

The edit is checked the only way it can be: by parsing the result with `tomllib`
and by looking at the comments around it, because a fold that reorders a
manifest or drops a comment is worse than no fold at all.
"""

import io
import json
import sys
import tomllib
from pathlib import Path
from typing import Any

import pytest

from blastit import fold

MANIFEST = """\
# A manifest with the two kinds of group, and a route with a table grant.
version = 1
groups = ["session"]

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

# Session housekeeping, for every route.
[group.session.db]
queries = ["SELECT 'session'"]

[group.own.db]
queries = ["SELECT 'own'"]

[[route]]
name = "notes"
path = ['/notes/.*']
groups = ["own"]
[route.db]
queries = [
  "SELECT 'granted'",
]

[[route]]
name = "admin"
path = ['/admin/.*']
[route.db]
unrestricted = true

[[route]]
name = "catalog"
path = ['/catalog/']
[route.db]
queries = ["SELECT 'listed'"]
[route.db.tables]
products = { select = ["id"] }
"""


def denial(sql: str, route: str = "notes", proxy: str = "db", reason: str = "not granted") -> Any:
    """One `sql.denied` line's worth of a denial."""
    return fold._Denial(route=route, proxy=proxy, sql=sql, reason=reason)


def document(text: str = MANIFEST) -> dict[str, Any]:
    """The manifest as `fold` reads it."""
    return tomllib.loads(text)


def covering_for(table: str = "orders") -> Any:
    """A stand-in for `blastit._blastit.covering`, with one table grant."""

    def covering(statements: list[str], prefix: str) -> str:
        return json.dumps(
            {
                "tables": {table: {"select": ["id"], "insert": None, "update": None, "delete": False}},
                "functions": ["unaccent"],
                "toml": f"[{prefix}.tables]\n",
                "unanalysable": [],
            }
        )

    return covering


class TestPlanning:
    """Which denials become which grants."""

    def test_a_denial_becomes_an_exact_query(self) -> None:
        additions = fold.plan(
            document(), [denial("DELETE FROM notes WHERE id = $?")], prefer_tables=False
        )
        assert len(additions) == 1
        assert additions[0].route == "notes"
        assert additions[0].queries == [
            ("DELETE FROM notes WHERE id = $?", "not granted")
        ]
        assert additions[0].tables == {}
        assert additions[0].functions == []

    def test_a_statement_the_route_already_grants_is_not_added(self) -> None:
        additions = fold.plan(document(), [denial("SELECT 'granted';")], prefer_tables=False)
        assert additions[0].queries == []
        assert any("already granted by the route's own grant" in note for note in additions[0].notes)

    def test_a_statement_a_group_grants_is_not_added(self) -> None:
        additions = fold.plan(document(), [denial("SELECT 'own'")], prefer_tables=False)
        assert additions[0].queries == []
        assert any("the group 'own'" in note for note in additions[0].notes)

    def test_a_statement_a_global_group_grants_is_not_added(self) -> None:
        additions = fold.plan(document(), [denial("SELECT 'session'")], prefer_tables=False)
        assert additions[0].queries == []
        assert any("the global group 'session'" in note for note in additions[0].notes)

    def test_an_unrestricted_route_is_never_given_a_grant(self) -> None:
        additions = fold.plan(
            document(), [denial("SELECT 'anything'", route="admin")], prefer_tables=False
        )
        assert additions[0].queries == []
        assert any("unrestricted" in note for note in additions[0].notes)

    def test_an_undeclared_route_is_reported_not_granted(self) -> None:
        additions = fold.plan(
            document(), [denial("SELECT 1", route="ghost")], prefer_tables=False
        )
        assert additions[0].queries == []
        assert any("no route of that name" in note for note in additions[0].notes)

    def test_the_same_statement_twice_is_one_grant(self) -> None:
        additions = fold.plan(
            document(),
            [denial("SELECT 1"), denial("SELECT 1")],
            prefer_tables=False,
        )
        assert additions[0].queries == [("SELECT 1", "not granted")]

    def test_manifest_order_decides_the_order_of_the_blocks(self) -> None:
        additions = fold.plan(
            document(),
            [denial("SELECT 2", route="catalog"), denial("SELECT 1", route="notes")],
            prefer_tables=False,
        )
        assert [addition.route for addition in additions] == ["notes", "catalog"]

    def test_prefer_tables_turns_statements_into_a_table_grant(self) -> None:
        additions = fold.plan(
            document(),
            [denial("DELETE FROM orders WHERE id = $?")],
            prefer_tables=True,
            covering=covering_for(),
        )
        assert additions[0].queries == []
        assert additions[0].tables == {"orders": {"select": ["id"]}}
        assert additions[0].functions == ["unaccent"]

    def test_prefer_tables_keeps_what_a_table_grant_cannot_describe(self) -> None:
        def covering(statements: list[str], prefix: str) -> str:
            return json.dumps(
                {
                    "tables": {},
                    "functions": [],
                    "toml": "",
                    "unanalysable": [
                        {"sql": statement, "reason": "system catalogues are not grantable"}
                        for statement in statements
                    ],
                }
            )

        additions = fold.plan(
            document(),
            [denial("SELECT * FROM pg_type")],
            prefer_tables=True,
            covering=covering,
        )
        assert additions[0].queries == [
            ("SELECT * FROM pg_type", "system catalogues are not grantable")
        ]
        assert additions[0].tables == {}

    def test_prefer_tables_merges_into_the_routes_existing_table_grant(self) -> None:
        additions = fold.plan(
            document(),
            [denial("INSERT INTO products (id, name) VALUES ($1, $2)", route="catalog")],
            prefer_tables=True,
            covering=lambda statements, prefix: json.dumps(
                {
                    "tables": {"products": {"insert": ["id", "name"]}},
                    "functions": [],
                    "toml": "",
                    "unanalysable": [],
                }
            ),
        )
        # The route's own `select = ["id"]` is kept, and the new INSERT is added
        # to it: one table grant, not two.
        assert additions[0].tables == {
            "products": {"select": ["id"], "insert": ["id", "name"]}
        }

    def test_prefer_tables_skips_what_the_existing_table_grant_covers(self) -> None:
        additions = fold.plan(
            document(),
            [denial('SELECT "products"."id" FROM "products"', route="catalog")],
            prefer_tables=True,
            covering=covering_for("products"),
        )
        assert additions[0].queries == []
        assert additions[0].tables == {}
        assert any("already covered" in note for note in additions[0].notes)

    def test_the_covering_grant_is_taken_over_all_the_statements_at_once(self) -> None:
        seen: list[list[str]] = []

        def covering(statements: list[str], prefix: str) -> str:
            seen.append(statements)
            return json.dumps(
                {"tables": {}, "functions": [], "toml": "", "unanalysable": []}
            )

        fold.plan(
            document(),
            [denial("SELECT 1"), denial("SELECT 2")],
            prefer_tables=True,
            covering=covering,
        )
        assert seen == [["SELECT 1", "SELECT 2"]], "one call, not one per statement"


class TestRender:
    """What a reader (or an agent) pastes into the manifest."""

    def test_a_block_is_a_route_grant_with_the_reasons_as_comments(self) -> None:
        additions = fold.plan(
            document(), [denial("SELECT 1", reason="the statement is not granted")], prefer_tables=False
        )
        rendered = fold.render(additions)
        assert "[route.db]" in rendered
        assert "queries = [" in rendered
        assert '"SELECT 1",  # the statement is not granted' in rendered

    def test_a_prefer_tables_block_names_the_tables_section(self) -> None:
        additions = fold.plan(
            document(), [denial("SELECT 1")], prefer_tables=True, covering=covering_for()
        )
        rendered = fold.render(additions)
        assert "[route.db.tables]" in rendered
        assert 'orders = { select = ["id"] }' in rendered
        assert "functions = [\"unaccent\"]" in rendered

    def test_nothing_to_add_is_no_output(self) -> None:
        additions = fold.plan(document(), [denial("SELECT 'granted'")], prefer_tables=False)
        assert fold.render(additions) == ""
        assert additions[0].notes, "the reason is still said, on stderr"

    def test_a_note_alone_prints_no_toml(self) -> None:
        additions = fold.plan(document(), [denial("SELECT 1", route="ghost")], prefer_tables=False)
        assert fold.render(additions) == ""
        assert additions[0].notes == [
            "ghost: the manifest declares no route of that name — add it, then fold again"
        ]


class TestEdit:
    """The `--write` edit: a line-level merge, checked by parsing the result."""

    def _addition(self, **kwargs: Any) -> Any:
        return fold.plan(document(), [denial(**kwargs)], prefer_tables=False)[0]

    def test_a_statement_is_appended_inside_the_existing_array(self) -> None:
        edited = fold.apply_additions(MANIFEST, [self._addition(sql="SELECT 1")])
        assert '"SELECT \'granted\'",' in edited, "the old statement is still there"
        assert '"SELECT 1",  # not granted' in edited
        assert "queries = [\n" in edited
        assert edited.count("[[route]]") == 3, "the file still has its routes"
        parsed = tomllib.loads(edited)
        queries = parsed["route"][0]["db"]["queries"]
        assert queries == ["SELECT 'granted'", "SELECT 1"]

    def test_comments_and_order_survive(self) -> None:
        edited = fold.apply_additions(MANIFEST, [self._addition(sql="SELECT 1")])
        assert "# A manifest with the two kinds of group" in edited
        assert "# Session housekeeping, for every route." in edited
        assert edited.index("[group.own.db]") < edited.index("[[route]]")

    def test_a_route_without_a_grant_gets_one(self) -> None:
        addition = fold.plan(document(), [denial("SELECT 'new'")], prefer_tables=False)[0]
        addition.proxy = "warehouse"
        edited = fold.apply_additions(MANIFEST, [addition])
        parsed = tomllib.loads(edited)
        notes = [route for route in parsed["route"] if route["name"] == "notes"][0]
        assert notes["warehouse"]["queries"] == ["SELECT 'new'"]
        assert notes["db"]["queries"] == ["SELECT 'granted'"]
        assert "tables" not in notes["db"], "nothing invented for a route that had none"
        catalog = [route for route in parsed["route"] if route["name"] == "catalog"][0]
        assert catalog["db"]["tables"] == {"products": {"select": ["id"]}}
        assert catalog["db"]["queries"] == ["SELECT 'listed'"]
        assert "# A manifest with the two kinds of group" in edited

    def test_a_one_line_array_becomes_a_multi_line_one(self) -> None:
        text = MANIFEST.replace(
            'queries = [\n  "SELECT \'granted\'",\n]', 'queries = ["SELECT \'granted\'"]  # one line'
        )
        edited = fold.apply_additions(text, [self._addition(sql="SELECT 1")])
        parsed = tomllib.loads(edited)
        assert parsed["route"][0]["db"]["queries"] == ["SELECT 'granted'", "SELECT 1"]
        assert "# one line" in edited

    def test_an_existing_table_entry_is_updated_in_place(self) -> None:
        addition = fold.plan(
            document(),
            [denial("INSERT INTO products (id, name) VALUES ($1, $2)", route="catalog")],
            prefer_tables=True,
            covering=lambda statements, prefix: json.dumps(
                {
                    "tables": {"products": {"insert": ["name"]}},
                    "functions": [],
                    "toml": "",
                    "unanalysable": [],
                }
            ),
        )[0]
        edited = fold.apply_additions(MANIFEST, [addition])
        assert edited.index("products =") > edited.index("[route.db.tables]")
        parsed = tomllib.loads(edited)
        catalog = [route for route in parsed["route"] if route["name"] == "catalog"][0]
        assert catalog["db"]["tables"] == {"products": {"select": ["id"], "insert": ["name"]}}
        assert "products = { select = [\"id\"], insert = [\"name\"] }" in edited

    def test_a_new_tables_section_is_written_under_the_grant(self) -> None:
        addition = fold.plan(
            document(), [denial("SELECT 1")], prefer_tables=True, covering=covering_for()
        )[0]
        edited = fold.apply_additions(MANIFEST, [addition])
        parsed = tomllib.loads(edited)
        assert parsed["route"][0]["db"]["tables"] == {"orders": {"select": ["id"]}}
        assert 'orders = { select = ["id"] }' in edited

    def test_a_route_the_manifest_does_not_declare_cannot_be_written(self) -> None:
        addition = fold._Addition(
            route="ghost", proxy="db", queries=[("SELECT 1", "not granted")]
        )
        with pytest.raises(fold._FoldError):
            fold.apply_additions(MANIFEST, [addition])

    def test_a_queries_key_that_is_not_an_array_is_refused(self) -> None:
        text = MANIFEST.replace(
            'queries = [\n  "SELECT \'granted\'",\n]', 'queries = "SELECT \'granted\'"'
        )
        with pytest.raises(fold._FoldError):
            fold.apply_additions(text, [self._addition(sql="SELECT 1")])


class TestCli:
    """The command itself: reading a log, printing, and `--write`."""

    LOG = "\n".join(
        [
            '2026-10-25T14:02:11.384Z app[web.1]: {"event":"request","method":"POST",'
            '"path":"/notes/","route":"notes"}',
            "a line that is not an event",
            '{"event":"sql.denied","route":"notes","proxy":"db","sql":"DELETE FROM notes",'
            '"reason":"the statement is not granted","sqlstate":"42501"}',
        ]
    )

    def _files(self, tmp_path: Path) -> tuple[Path, Path]:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(MANIFEST, encoding="utf-8")
        log = tmp_path / "app.log"
        log.write_text(self.LOG + "\n", encoding="utf-8")
        return manifest, log

    def test_the_toml_to_add_is_printed(self, tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
        manifest, log = self._files(tmp_path)
        assert fold.main(["fold", str(log), "--manifest", str(manifest)]) == 0
        out = capsys.readouterr().out
        assert "[route.db]" in out
        assert '"DELETE FROM notes",  # the statement is not granted' in out
        assert manifest.read_text(encoding="utf-8") == MANIFEST, "nothing was written"

    def test_write_merges_it_into_the_manifest(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest, log = self._files(tmp_path)
        assert fold.main(["fold", str(log), "--manifest", str(manifest), "--write"]) == 0
        written = manifest.read_text(encoding="utf-8")
        assert '"DELETE FROM notes"' in written
        # The manifest is still the manifest: the routes, the comments, the
        # table grant and the groups are all where they were.
        parsed = tomllib.loads(written)
        assert [route["name"] for route in parsed["route"]] == ["notes", "admin", "catalog"]
        assert parsed["route"][0]["db"]["queries"] == ["SELECT 'granted'", "DELETE FROM notes"]
        assert parsed["groups"] == ["session"]
        assert "# A manifest with the two kinds of group" in written

    def test_nothing_to_add_writes_nothing(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(MANIFEST, encoding="utf-8")
        log = tmp_path / "app.log"
        log.write_text(
            '{"event":"sql.denied","route":"notes","proxy":"db","sql":"SELECT \'granted\'"}'
            + "\n",
            encoding="utf-8",
        )
        assert fold.main(["fold", str(log), "--manifest", str(manifest), "--write"]) == 0
        assert manifest.read_text(encoding="utf-8") == MANIFEST
        assert "nothing to add" in capsys.readouterr().err

    def test_standard_input_is_a_log(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(MANIFEST, encoding="utf-8")
        monkeypatch.setattr(sys, "stdin", io.StringIO(self.LOG))
        assert fold.main(["fold", "-", "--manifest", str(manifest)]) == 0

    def test_a_log_that_is_not_there_is_an_error(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(MANIFEST, encoding="utf-8")
        assert fold.main(["fold", str(tmp_path / "nope.log"), "--manifest", str(manifest)]) == 1
        assert "nope.log" in capsys.readouterr().err

    def test_a_manifest_that_is_not_there_is_an_error(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        log = tmp_path / "app.log"
        log.write_text(self.LOG + "\n", encoding="utf-8")
        assert fold.main(["fold", str(log), "--manifest", str(tmp_path / "nope.toml")]) == 1
        assert "nope.toml" in capsys.readouterr().err


class TestWithTheExtension:
    """The same decisions, with blastit's own analysis instead of a stub."""

    def test_prefer_tables_reports_a_statement_no_table_grant_describes(self) -> None:
        pytest.importorskip("blastit._blastit", reason="the analysis is the extension")
        additions = fold.plan(
            document(),
            [denial("SET TIME ZONE 'UTC'"), denial('SELECT "products"."id" FROM "products"')],
            prefer_tables=True,
        )
        assert additions[0].queries == [
            ("SET TIME ZONE 'UTC'", additions[0].queries[0][1])
        ]
        assert "only SELECT" in additions[0].queries[0][1]
        assert additions[0].tables == {"products": {"select": ["id"]}}, (
            "the covering grant of the analysable statement, cleaned of the "
            "operations it does not grant"
        )

    def test_a_denial_the_log_normalised_is_analysed_all_the_same(self) -> None:
        pytest.importorskip("blastit._blastit", reason="the analysis is the extension")
        additions = fold.plan(
            document(),
            [denial('DELETE FROM products WHERE products.id IN ($?, ...)')],
            prefer_tables=True,
        )
        assert additions[0].queries == []
        assert additions[0].tables["products"]["delete"] is True
