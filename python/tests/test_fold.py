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
import subprocess
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


#: A route whose whole `db` grant is written as sections of it — no
#: `[route.db]` header at all. blastit boots this (the tables are the route's,
#: §6), so fold has to accept it too.
TABLES_ONLY = """\
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "catalog"
path = ['/catalog/.*']
[route.db.tables]
products = { select = ["id"] }
"""


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
            covering=lambda statements, prefix: json.dumps(
                {
                    "tables": {"products": {"select": ["id"]}},
                    "functions": [],
                    "toml": "",
                    "unanalysable": [],
                }
            ),
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
            "ghost: the manifest declares no route of that name — fold extends the routes "
            "a manifest has, it never creates one"
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

    def test_a_route_without_a_grant_on_that_proxy_is_refused(self) -> None:
        """A denial can only come from a proxy the route is granted (§2.2)."""
        addition = fold.plan(
            document(), [denial("SELECT 'new'", proxy="warehouse")], prefer_tables=False
        )[0]
        # `warehouse` is not declared by this manifest, so the plan refuses it…
        assert addition.is_empty()
        assert any("does not declare" in note for note in addition.notes)
        # …and a declared proxy the route has no grant on is refused too.
        manifest = MANIFEST.replace(
            'input = { url = "DATABASE_URL" }',
            'input = { url = "DATABASE_URL" }\n\n[proxy.warehouse]\n'
            'protocol = "postgresql"\ninput = { url = "WAREHOUSE_URL" }',
        )
        addition = fold.plan(
            document(manifest), [denial("SELECT 'new'", proxy="warehouse")], prefer_tables=False
        )[0]
        assert addition.is_empty()
        assert any("no grant on that proxy" in note for note in addition.notes)
        with pytest.raises(fold._FoldError):
            fold.apply_additions(manifest, [fold._Addition(
                route="notes", proxy="warehouse", queries=[("SELECT 'new'", "")]
            )])

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

    def test_a_grant_written_as_sections_alone_takes_a_queries_key(self) -> None:
        """`[route.db.tables]` is the route's `db` grant, so fold writes into it."""
        addition = fold._Addition(
            route="catalog", proxy="db", queries=[("SELECT 1", "not granted")]
        )
        edited = fold.apply_additions(TABLES_ONLY, [addition])
        parsed = tomllib.loads(edited)
        grant = parsed["route"][0]["db"]
        assert grant["queries"] == ["SELECT 1"]
        assert grant["tables"] == {"products": {"select": ["id"]}}, "the table grant is intact"
        # The key is written under a `[route.db]` header of its own, before the
        # section it would otherwise have landed inside.
        assert edited.index("[route.db]\nqueries = [") < edited.index("[route.db.tables]")
        assert edited.rstrip().endswith('products = { select = ["id"] }')

    def test_a_nested_table_section_is_a_grant_too(self) -> None:
        """`[route.db.tables.products]` and nothing else is still a grant."""
        text = TABLES_ONLY.replace(
            '[route.db.tables]\nproducts = { select = ["id"] }',
            '[route.db.tables.products]\nselect = ["id"]\n',
        )
        addition = fold._Addition(
            route="catalog", proxy="db", queries=[("SELECT 1", "not granted")]
        )
        edited = fold.apply_additions(text, [addition])
        parsed = tomllib.loads(edited)
        assert parsed["route"][0]["db"]["queries"] == ["SELECT 1"]
        assert parsed["route"][0]["db"]["tables"]["products"]["select"] == ["id"]
        # The super-table is written before the section it is the parent of.
        assert edited.index("[route.db]\n") < edited.index("[route.db.tables.products]")

    def test_a_tables_section_can_be_added_to_a_sections_only_grant(self) -> None:
        """The `--prefer-tables` edit lands in the table grant that is there."""
        addition = fold._Addition(
            route="catalog", proxy="db", tables={"orders": {"select": ["id"]}}
        )
        edited = fold.apply_additions(TABLES_ONLY, [addition])
        parsed = tomllib.loads(edited)
        assert parsed["route"][0]["db"]["tables"] == {
            "products": {"select": ["id"]},
            "orders": {"select": ["id"]},
        }

    def test_the_refusal_counts_the_sections_a_grant_can_be_written_as(self) -> None:
        """A route with no grant at all is told which sections would have been one."""
        manifest = MANIFEST.replace(
            'input = { url = "DATABASE_URL" }',
            'input = { url = "DATABASE_URL" }\n\n[proxy.warehouse]\n'
            'protocol = "postgresql"\ninput = { url = "WAREHOUSE_URL" }',
        )
        addition = fold._Addition(
            route="notes", proxy="warehouse", queries=[("SELECT 1", "not granted")]
        )
        with pytest.raises(fold._FoldError) as refused:
            fold.apply_additions(manifest, [addition])
        message = str(refused.value)
        assert "no `[route.warehouse]` section" in message, message
        assert "no `[route.warehouse.tables]` one" in message, message
        assert "and no group that grants it" in message, message

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

    def test_the_module_takes_folds_arguments_without_the_subcommand(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        """`python -m blastit.fold LOG --manifest …`: this module *is* fold."""
        manifest, log = self._files(tmp_path)
        assert fold._direct([str(log), "--manifest", str(manifest)]) == 0
        out = capsys.readouterr().out
        assert "[route.db]" in out
        assert '"DELETE FROM notes",  # the statement is not granted' in out

    def test_the_module_parses_its_own_arguments_for_real(self, tmp_path: Path) -> None:
        """`python -m blastit.fold app.log …` used to refuse `app.log` as an
        invalid subcommand: the module takes fold's arguments, and nothing else."""
        manifest, log = self._files(tmp_path)
        completed = subprocess.run(  # noqa: S603 - sys.executable, fixed argv
            [sys.executable, "-m", "blastit.fold", str(log), "--manifest", str(manifest)],
            capture_output=True,
            text=True,
            check=False,
        )
        assert completed.returncode == 0, completed.stderr
        assert "[route.db]" in completed.stdout
        assert '"DELETE FROM notes"' in completed.stdout

    def test_the_module_fails_the_way_the_console_script_does(self, tmp_path: Path) -> None:
        """Same exit status, same words, for a log that is not there."""
        manifest, _ = self._files(tmp_path)
        completed = subprocess.run(  # noqa: S603 - sys.executable, fixed argv
            [
                sys.executable,
                "-m",
                "blastit.fold",
                str(tmp_path / "nope.log"),
                "--manifest",
                str(manifest),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        assert completed.returncode == 1
        assert "blastit-manifest fold:" in completed.stderr
        assert "nope.log" in completed.stderr

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
        # A plain `SELECT` the analysis refuses (a system catalogue cannot be a
        # table grant) is kept as an exact query: the kind is what decides, not
        # the analysis.
        additions = fold.plan(
            document(),
            [denial('SELECT "pg_type"."oid" FROM "pg_type"')],
            prefer_tables=True,
        )
        assert [sql for sql, _ in additions[0].queries] == [
            'SELECT "pg_type"."oid" FROM "pg_type"'
        ]
        assert "catalogues" in additions[0].queries[0][1]
        assert additions[0].tables == {}

    def test_a_denial_the_log_normalised_is_analysed_all_the_same(self) -> None:
        pytest.importorskip("blastit._blastit", reason="the analysis is the extension")
        additions = fold.plan(
            document(),
            [denial('DELETE FROM products WHERE products.id IN ($?, ...)')],
            prefer_tables=True,
        )
        assert additions[0].queries == []
        assert additions[0].tables["products"]["delete"] is True


class TestDangerousStatements:
    """A grant is not an allowlist for DDL, `SET`, `COPY` or a transaction (D3c).

    The kind comes from PostgreSQL's own parser — never from a pattern over the
    text — so `SET ROLE` and a `SELECT` are told apart by the same authority that
    will run them.
    """

    def classify(self, verdict: str, label: str = "SET") -> Any:
        def classify(sql: str) -> str:
            return json.dumps({"class": verdict, "label": label, "plain": verdict != "other"})

        return classify

    @pytest.mark.parametrize(
        "sql",
        [
            "SET ROLE admin",
            "SET SESSION AUTHORIZATION 'admin'",
            "RESET ALL",
            "COPY notes FROM STDIN",
            "DO $$ BEGIN END $$",
            "CALL p()",
            "LOCK TABLE notes",
            "GRANT SELECT ON notes TO x",
            "REVOKE SELECT ON notes FROM x",
            "ALTER TABLE notes ADD COLUMN a int",
            "CREATE TABLE t (a int)",
            "DROP TABLE notes",
            "TRUNCATE notes",
            "VACUUM notes",
            "LISTEN c",
            "NOTIFY c",
            "BEGIN",
            "COMMIT",
        ],
    )
    def test_a_statement_of_another_kind_is_refused(self, sql: str) -> None:
        additions = fold.plan(document(), [denial(sql)], prefer_tables=False)
        assert additions[0].queries == [], sql
        assert additions[0].is_empty(), sql
        note = additions[0].notes[0]
        assert "refused" in note and sql in note, note
        assert "--allow-dangerous" in note, note

    def test_the_label_names_the_kind_the_parser_saw(self) -> None:
        additions = fold.plan(document(), [denial("SET ROLE admin")], prefer_tables=False)
        assert "SET is not a plain SELECT, INSERT, UPDATE or DELETE" in additions[0].notes[0]
        additions = fold.plan(document(), [denial("CREATE TABLE t (a int)")], prefer_tables=False)
        assert "CREATE TABLE is not a plain" in additions[0].notes[0]

    def test_a_plain_statement_of_every_kind_is_granted(self) -> None:
        for sql in [
            "SELECT 1",
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "INSERT INTO notes (body) VALUES ($?)",
            "UPDATE notes SET body = $? WHERE id = $?",
            "DELETE FROM notes WHERE id = $?",
        ]:
            additions = fold.plan(document(), [denial(sql)], prefer_tables=False)
            assert [item for item, _ in additions[0].queries] == [sql], sql

    def test_the_flag_grants_it_anyway_and_says_so(self) -> None:
        additions = fold.plan(
            document(),
            [denial("SET ROLE admin")],
            prefer_tables=False,
            allow_dangerous=True,
        )
        assert [sql for sql, _ in additions[0].queries] == ["SET ROLE admin"]
        assert any("--allow-dangerous" in note for note in additions[0].notes)

    def test_a_statement_nothing_can_parse_is_refused(self) -> None:
        """Text that is not SQL is not a grant: nothing would ever match it."""
        additions = fold.plan(
            document(),
            [denial("SELECT 'unterminated")],
            prefer_tables=False,
        )
        assert additions[0].is_empty()
        assert "unreadable" in additions[0].notes[0]

    def test_prefer_tables_refuses_them_too(self) -> None:
        additions = fold.plan(
            document(),
            [denial("SELECT 'x' FROM t"), denial("SET ROLE admin")],
            prefer_tables=True,
            covering=covering_for("t"),
        )
        assert [sql for sql, _ in additions[0].queries] == []
        assert additions[0].tables == {"t": {"select": ["id"]}}
        assert any("SET ROLE admin" in note for note in additions[0].notes)

    def test_the_classifier_is_what_decides(self) -> None:
        """Nothing about the text decides it: the injected verdict is obeyed (D3c)."""
        additions = fold.plan(
            document(),
            [denial("SELECT 1")],
            prefer_tables=False,
            classify=self.classify("other", "SET"),
        )
        assert additions[0].is_empty()
        assert "refused" in additions[0].notes[0]


class TestLogFieldsAreNotManifestText:
    """Whatever a log line holds, it reaches the file as data or not at all (D3c)."""

    def test_a_reason_becomes_one_bounded_comment(self) -> None:
        hostile = 'nope]\n[route.evil]\nqueries = ["DROP TABLE users"]\n# \x07\x01'
        rendered = fold.render(
            fold.plan(document(), [denial("SELECT 1", reason=hostile)], prefer_tables=False)
        )
        assert "\x07" not in rendered and "\x01" not in rendered
        assert len(rendered.splitlines()) == 5, (
            "the reason is a comment on the statement's own line, and adds none"
        )
        parsed = tomllib.loads(rendered)
        assert parsed["route"]["db"]["queries"] == ["SELECT 1"], (
            "the reason is a comment, and only a comment"
        )
        assert "evil" not in parsed["route"], "no table was opened by a log line"
        assert [line for line in rendered.splitlines() if line.startswith("[")] == ["[route.db]"]

    def test_a_long_reason_is_cut(self) -> None:
        rendered = fold.render(
            fold.plan(document(), [denial("SELECT 1", reason="x" * 5000)], prefer_tables=False)
        )
        comment = next(line for line in rendered.splitlines() if "#" in line)
        assert len(comment) <= fold.COMMENT_LIMIT + 40, len(comment)

    def test_a_statement_is_an_escaped_toml_string(self) -> None:
        sql = "SELECT 'a\"b', 'c\\d', 'e\nf' FROM t"
        rendered = fold.render(
            fold.plan(document(), [denial(sql)], prefer_tables=False)
        )
        parsed = tomllib.loads(rendered)
        assert parsed["route"]["db"]["queries"] == [sql]

    def test_a_table_name_needing_quotes_is_quoted(self) -> None:
        addition = fold._Addition(
            route="notes",
            proxy="db",
            tables={"weird name": {"select": ["id"]}},
        )
        rendered = fold.render([addition])
        assert '"weird name" = { select = ["id"] }' in rendered
        assert tomllib.loads(rendered)["route"]["db"]["tables"] == {
            "weird name": {"select": ["id"]}
        }

    def test_a_proxy_that_is_not_a_plain_toml_key_is_refused(self) -> None:
        additions = fold.plan(
            document(), [denial("SELECT 1", proxy='db"\ninjected')], prefer_tables=False
        )
        assert additions[0].is_empty()
        assert "not a plain TOML key" in additions[0].notes[0]

    def test_a_proxy_the_manifest_does_not_declare_is_refused(self) -> None:
        additions = fold.plan(
            document(), [denial("SELECT 1", proxy="elsewhere")], prefer_tables=False
        )
        assert additions[0].is_empty()
        assert "does not declare" in additions[0].notes[0]

    def test_a_route_the_manifest_does_not_declare_is_refused(self) -> None:
        additions = fold.plan(
            document(), [denial("SELECT 1", route="ghost]\n[route.evil")], prefer_tables=False
        )
        assert additions[0].is_empty()
        assert "no route of that name" in additions[0].notes[0]

    def test_the_notes_are_diagnostics_not_toml(self) -> None:
        additions = fold.plan(document(), [denial("SET ROLE admin")], prefer_tables=False)
        rendered = fold.render(additions)
        assert rendered == "", "a refusal prints no TOML"
        assert additions[0].notes, "and the reason is still said"


class TestTheScan:
    """Headers and keys, told apart from the same text inside a string or array."""

    def test_a_header_inside_a_multi_line_string_is_not_a_header(self) -> None:
        text = (
            'version = 1\n\n[[route]]\nname = "one"\npath = ["/one/"]\n'
            '[route.db]\nqueries = [\n  """\n[[route]]\n[route.db]\n""",\n]\n\n'
            '[[route]]\nname = "two"\npath = ["/two/"]\n[route.db]\nqueries = []\n'
        )
        scan = fold._Scan(text)
        headers = [line.header for line in scan.headers()]
        assert headers == [("route",), ("route", "db"), ("route",), ("route", "db")]
        routes = [
            fold._route_name(scan, line)
            for line in scan.headers(("route",))
            if line.array_of_tables
        ]
        assert routes == ["one", "two"]

    def test_a_key_inside_a_multi_line_array_is_not_a_key(self) -> None:
        text = 'version = 1\n\n[[route]]\nname = "one"\npath = ["/one/"]\n'
        text += '[route.db]\nqueries = [\n  "SELECT 1",\n  "tables = 2",\n]\n'
        scan = fold._Scan(text)
        keys = [line.key for line in scan.lines if line.key is not None]
        assert keys == [("version",), ("name",), ("path",), ("queries",)]

    def test_a_key_inside_a_multi_line_string_is_not_a_key(self) -> None:
        text = 'version = 1\n\n[[route]]\nname = "one"\npath = ["/one/"]\n'
        text += '[route.db]\nqueries = [\n  """\nname = "evil"\n""",\n]\n'
        scan = fold._Scan(text)
        names = [line.key for line in scan.lines if line.key == ("name",)]
        assert len(names) == 1

    def test_a_bracket_inside_a_string_does_not_open_an_array(self) -> None:
        text = 'version = 1\n\n[[route]]\nname = "one"\npath = ["/one/"]\n'
        text += '[route.db]\nqueries = ["SELECT \'[\' , \']\'"]\n[[route]]\nname = "two"\n'
        scan = fold._Scan(text)
        assert [line.header for line in scan.headers(("route",))] == [("route",), ("route",)]
        assert scan.lines[-1].depth == 0

    def test_find_unquoted_steps_over_quoted_regions(self) -> None:
        """The bracket a one-line array closes on is found outside its strings."""
        for sample in [
            'a = ["]"]',
            'a = ["x]y"]',
            r'a = ["x\"] y"]',
            "a = [']' ]",
        ]:
            found = fold._find_unquoted(sample, "]", 0)
            assert sample[found:] == "]", f"{sample!r}: {found}"
        assert fold._find_unquoted('a = ["x"] # ]', "]", 9) == -1, "a comment is not a bracket"
        assert fold._find_unquoted("nothing here", "]", 0) == -1
        assert fold._find_unquoted("a = ']' ", "]", 0) == -1, "a literal is not a bracket"

    def test_a_quoted_region_ends_where_toml_says_it_does(self) -> None:
        for text, expected_rest in [
            ('"a\\"b" rest', " rest"),
            ("'a''b' rest", " rest"),
            ('"a]b" rest', " rest"),
            ("'a]b' rest", " rest"),
        ]:
            assert text[fold._quoted_end(text, 0, text[0]) :] == expected_rest, text


class TestTheEditAgainstHostileShapes:
    """The edit against the manifest shapes a line regex would get wrong (D3c)."""

    ONE_LINE = """\
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "notes"
path = ['/notes/']
[route.db]
queries = ["SELECT 'a]b'", 'SELECT "c]d"."e" FROM "c]d"']  # two statements, one line
"""

    def addition(self, sql: str = "SELECT 1") -> Any:
        return fold.plan(document(self.ONE_LINE), [denial(sql)], prefer_tables=False)[0]

    def test_a_bracket_in_a_literal_does_not_end_the_array(self) -> None:
        edited = fold.apply_additions(self.ONE_LINE, [self.addition()])
        parsed = tomllib.loads(edited)
        assert parsed["route"][0]["db"]["queries"] == [
            "SELECT 'a]b'",
            'SELECT "c]d"."e" FROM "c]d"',
            "SELECT 1",
        ]
        assert "# two statements, one line" in edited

    def test_a_multi_line_string_does_not_hide_the_route_it_is_in(self) -> None:
        text = self.ONE_LINE.replace(
            """queries = ["SELECT 'a]b'", 'SELECT "c]d"."e" FROM "c]d"']  # two statements, one line""",
            'queries = [\n  """\n[[route]]\nname = "not a route"\n""",\n]',
        )
        text += '\n[[route]]\nname = "later"\npath = ["/later/"]\n[route.db]\nqueries = []\n'
        addition = fold.plan(
            document(text), [denial("SELECT 1", route="later")], prefer_tables=False
        )[0]
        edited = fold.apply_additions(text, [addition])
        parsed = tomllib.loads(edited)
        assert [route["name"] for route in parsed["route"]] == ["notes", "later"]
        assert parsed["route"][1]["db"]["queries"] == ["SELECT 1"]
        assert '"""' in edited, "the multi-line string is still there"

    def test_an_inline_grant_is_refused_not_duplicated(self) -> None:
        text = self.ONE_LINE.replace(
            "[route.db]\nqueries = [\"SELECT 'a]b'\", 'SELECT \"c]d\".\"e\" FROM \"c]d\"']  # two statements, one line",
            'db = { queries = ["SELECT 1"] }',
        )
        with pytest.raises(fold._FoldError) as refused:
            fold.apply_additions(text, [self.addition()])
        assert "inline table" in str(refused.value)
        assert "line" in str(refused.value)

    def test_an_inline_tables_grant_is_refused_not_duplicated(self) -> None:
        text = self.ONE_LINE.replace(
            "queries = [\"SELECT 'a]b'\", 'SELECT \"c]d\".\"e\" FROM \"c]d\"']  # two statements, one line",
            'tables = { products = { select = ["id"] } }',
        )
        addition = fold.plan(
            document(text),
            [denial("INSERT INTO products (id) VALUES ($1)")],
            prefer_tables=True,
            covering=lambda statements, prefix: json.dumps(
                {
                    "tables": {"products": {"insert": ["id"]}},
                    "functions": [],
                    "toml": "",
                    "unanalysable": [],
                }
            ),
        )[0]
        assert addition.tables, "the plan has something to write"
        with pytest.raises(fold._FoldError) as refused:
            fold.apply_additions(text, [addition])
        assert "inline table" in str(refused.value)


class TestTheEditIsProved:
    """Nothing is written until the result is the manifest plus exactly the plan (D3c).

    The plan is a document a log wrote; the file is one a manifest author owns.
    Between the two, the edit has to be *proved*: re-parsed, compared key by key,
    and audited by blastit itself.
    """

    MANIFEST = """\
# A manifest with one route and one grant.
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "notes"
path = ['/notes/']
[route.db]
queries = ["SELECT 'granted'"]
"""

    LOG = "\n".join(
        [
            '{"event":"request","method":"GET","path":"/notes/","route":"notes"}',
            '{"event":"sql.denied","route":"notes","proxy":"db","sql":"SELECT \'new\'",'
            '"reason":"not granted"}',
        ]
    )

    @pytest.fixture
    def manifest(self, tmp_path: Path) -> Path:
        path = tmp_path / "blastit.toml"
        path.write_text(self.MANIFEST, encoding="utf-8")
        return path

    def planned(self, tmp_path: Path, manifest: Path) -> tuple[Any, ...]:
        """The old text, the document, the events, the denials and the plan."""
        events = fold.read_events([str(tmp_path / "app.log")])
        denials = fold.denials_of(events)
        additions = fold.plan(
            tomllib.loads(manifest.read_text(encoding="utf-8")),
            denials,
            prefer_tables=False,
        )
        return events, denials, additions

    @pytest.fixture(autouse=True)
    def log(self, tmp_path: Path) -> None:
        (tmp_path / "app.log").write_text(self.LOG + "\n", encoding="utf-8")

    def test_an_edit_that_is_exactly_the_plan_is_accepted(self, tmp_path: Path, manifest: Path) -> None:
        old = manifest.read_text(encoding="utf-8")
        events, denials, additions = self.planned(tmp_path, manifest)
        edited = fold.apply_additions(old, additions)
        fold._verify_edit(
            manifest, old, edited, tomllib.loads(old), additions, events, denials
        )
        parsed = tomllib.loads(edited)
        assert parsed["route"][0]["db"]["queries"] == ["SELECT 'granted'", "SELECT 'new'"]

    def test_a_route_that_appeared_is_refused(self, tmp_path: Path, manifest: Path) -> None:
        old = manifest.read_text(encoding="utf-8")
        events, denials, additions = self.planned(tmp_path, manifest)
        edited = old + '\n[[route]]\nname = "extra"\npath = ["/extra/"]\n'
        with pytest.raises(fold._FoldError) as refused:
            fold._verify_edit(
                manifest, old, edited, tomllib.loads(old), additions, events, denials
            )
        assert "routes" in str(refused.value)

    def test_a_grant_that_changed_something_else_is_refused(
        self, tmp_path: Path, manifest: Path
    ) -> None:
        old = manifest.read_text(encoding="utf-8")
        events, denials, additions = self.planned(tmp_path, manifest)
        edited = fold.apply_additions(old, additions).replace(
            '  "SELECT \'new\'",',
            '  "SELECT \'new\'",\n  "SELECT \'unplanned\'",',
        )
        with pytest.raises(fold._FoldError) as refused:
            fold._verify_edit(
                manifest, old, edited, tomllib.loads(old), additions, events, denials
            )
        assert "not the grant the plan asked for" in str(refused.value)

    def test_a_proxy_that_changed_is_refused(self, tmp_path: Path, manifest: Path) -> None:
        old = manifest.read_text(encoding="utf-8")
        events, denials, additions = self.planned(tmp_path, manifest)
        edited = fold.apply_additions(old, additions).replace("proxy.db", "proxy.other")
        with pytest.raises(fold._FoldError) as refused:
            fold._verify_edit(
                manifest, old, edited, tomllib.loads(old), additions, events, denials
            )
        assert "proxy declarations" in str(refused.value)

    def test_a_statement_the_audit_still_refuses_is_refused(
        self, tmp_path: Path, manifest: Path
    ) -> None:
        """The last word is blastit's own audit, run on the whole document."""
        old = manifest.read_text(encoding="utf-8")
        events, denials, additions = self.planned(tmp_path, manifest)
        # A plan that claims to grant a statement the edit does not write: the
        # audit is what catches it.
        additions[0].statements.append("SELECT 'new'")
        additions[0].queries = []
        edited = fold.apply_additions(old, additions)
        with pytest.raises(fold._FoldError) as refused:
            fold._verify_edit(
                manifest, old, edited, tomllib.loads(old), additions, events, denials
            )
        assert "still refuses" in str(refused.value)

    def test_a_refused_edit_writes_nothing(
        self, tmp_path: Path, manifest: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """The file is left alone: a fold that cannot prove its edit does not guess."""
        old = manifest.read_text(encoding="utf-8")
        events, denials, additions = self.planned(tmp_path, manifest)
        monkeypatch.setattr(
            fold, "apply_additions", lambda text, wanted: text + '\n[[route]]\nname = "x"\n'
        )
        with pytest.raises(fold._FoldError):
            fold._write(manifest, old, tomllib.loads(old), events, denials, additions)
        assert manifest.read_text(encoding="utf-8") == old


class TestTheCliDiagnostics:
    """What a session prints where: TOML on stdout, notes on stderr (D3c)."""

    def test_a_refusal_is_a_diagnostic_not_a_comment(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(TestTheEditIsProved.MANIFEST, encoding="utf-8")
        log = tmp_path / "app.log"
        log.write_text(
            '{"event":"sql.denied","route":"notes","proxy":"db","sql":"SET ROLE admin",'
            '"reason":"not granted"}\n',
            encoding="utf-8",
        )
        assert fold.main(["fold", str(log), "--manifest", str(manifest)]) == 0
        captured = capsys.readouterr()
        assert "SET ROLE admin" in captured.err
        assert captured.err.startswith("note: "), captured.err
        assert not captured.err.startswith("# "), "a note is not a TOML comment"
        assert captured.out == "", "a refusal has no TOML to print"

    def test_the_flag_grants_it_and_the_file_parses(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(TestTheEditIsProved.MANIFEST, encoding="utf-8")
        log = tmp_path / "app.log"
        log.write_text(
            '{"event":"request","method":"GET","path":"/notes/","route":"notes"}\n'
            '{"event":"sql.denied","route":"notes","proxy":"db","sql":"SET ROLE admin",'
            '"reason":"not granted"}\n',
            encoding="utf-8",
        )
        assert (
            fold.main(["fold", str(log), "--manifest", str(manifest), "--allow-dangerous"])
            == 0
        )
        captured = capsys.readouterr()
        assert "SET ROLE admin" in captured.err, "it says what it did"
        parsed = tomllib.loads(captured.out)
        assert parsed["route"]["db"]["queries"] == ["SET ROLE admin"]

    def test_write_refuses_a_manifest_it_cannot_edit_safely(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(
            TestTheEditIsProved.MANIFEST.replace(
                '[route.db]\nqueries = ["SELECT \'granted\'"]',
                'db = { queries = ["SELECT \'granted\'"] }',
            ),
            encoding="utf-8",
        )
        old = manifest.read_text(encoding="utf-8")
        log = tmp_path / "app.log"
        log.write_text(
            '{"event":"request","method":"GET","path":"/notes/","route":"notes"}\n'
            '{"event":"sql.denied","route":"notes","proxy":"db","sql":"SELECT \'new\'",'
            '"reason":"not granted"}\n',
            encoding="utf-8",
        )
        assert fold.main(["fold", str(log), "--manifest", str(manifest), "--write"]) == 1
        assert "inline table" in capsys.readouterr().err
        assert manifest.read_text(encoding="utf-8") == old, "nothing was written"

    def test_prefer_tables_write_end_to_end(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(TestTheEditIsProved.MANIFEST, encoding="utf-8")
        log = tmp_path / "app.log"
        log.write_text(
            '{"event":"request","method":"GET","path":"/notes/","route":"notes"}\n'
            '{"event":"sql.denied","route":"notes","proxy":"db",'
            '"sql":"INSERT INTO orders (id, total) VALUES ($1, $2)","reason":"not granted"}\n',
            encoding="utf-8",
        )
        assert (
            fold.main(
                ["fold", str(log), "--manifest", str(manifest), "--prefer-tables", "--write"]
            )
            == 0
        )
        parsed = tomllib.loads(manifest.read_text(encoding="utf-8"))
        tables = parsed["route"][0]["db"]["tables"]
        assert tables["orders"] == {"insert": ["id", "total"]}
        assert parsed["route"][0]["db"]["queries"] == ["SELECT 'granted'"]


class TestAFunctionGrantSurvives:
    """A route's own `functions` are kept when the plan adds one (D3c)."""

    MANIFEST = """\
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "search"
path = ['/search/']
[route.db]
functions = ["unaccent"]
queries = ["SELECT 'listed'"]
"""

    LOG = "\n".join(
        [
            '{"event":"request","method":"GET","path":"/search/","route":"search"}',
            '{"event":"sql.denied","route":"search","proxy":"db",'
            '"sql":"SELECT unaccent($1), st_distance($1, $2)",'
            '"reason":"the function st_distance is not granted"}',
        ]
    )

    def test_the_union_is_written_and_the_edit_is_accepted(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(self.MANIFEST, encoding="utf-8")
        log = tmp_path / "app.log"
        log.write_text(self.LOG + "\n", encoding="utf-8")
        # `st_distance` is outside blastit's safe set, so the statement is
        # refused unless the operator asks for it (D3c).
        assert (
            fold.main(
                [
                    "fold",
                    str(log),
                    "--manifest",
                    str(manifest),
                    "--prefer-tables",
                    "--allow-dangerous",
                    "--write",
                ]
            )
            == 0
        ), capsys.readouterr().err
        parsed = tomllib.loads(manifest.read_text(encoding="utf-8"))
        assert parsed["route"][0]["db"]["functions"] == ["unaccent", "st_distance"], (
            "the route's own function stays where it is, and the plan's is appended"
        )
        assert parsed["route"][0]["db"]["queries"] == ["SELECT 'listed'"]


class TestLockGrants:
    """A row lock never gets `"*"`, and a lock with no column to name is a query (D3c)."""

    MANIFEST = """\
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "users"
path = ['/users/.*']
[route.db]
queries = ["SELECT 'listed'"]
"""

    def log(self, sql: str) -> str:
        return "\n".join(
            [
                '{"event":"request","method":"GET","path":"/users/","route":"users"}',
                '{"event":"sql.denied","route":"users","proxy":"db","sql":%s,'
                '"reason":"not granted"}' % json.dumps(sql),
            ]
        )

    def plan(self, sql: str, **kwargs: Any) -> Any:
        manifest = tomllib.loads(self.MANIFEST)
        return fold.plan(
            manifest,
            [fold._Denial(route="users", proxy="db", sql=sql, reason="not granted")],
            prefer_tables=True,
            **kwargs,
        )[0]

    def test_a_lock_the_statement_reads_a_column_of_names_it(self) -> None:
        addition = self.plan(
            'SELECT "auth_user"."id" FROM "auth_user" WHERE "auth_user"."id" = $? FOR UPDATE'
        )
        assert addition.tables["auth_user"]["update"] == ["id"]
        assert addition.tables["auth_user"]["update"] != "*", "never a star"

    def test_a_lock_that_reads_every_column_names_one_of_them(self) -> None:
        # `SELECT *` names no column, so there is nothing to put in `update`, and
        # `"*"` would grant the UPDATE privilege on every column of the table.
        addition = self.plan('SELECT * FROM "auth_user" FOR UPDATE')
        assert addition.queries == [
            ('SELECT * FROM "auth_user" FOR UPDATE', addition.queries[0][1])
        ]
        assert "primary key" in addition.queries[0][1], addition.queries[0][1]
        assert not addition.tables, "the table grant cannot cover it"

    def test_a_lock_that_reads_no_column_is_a_query_with_a_note(self) -> None:
        addition = self.plan('SELECT 1 FROM "auth_user" FOR UPDATE')
        assert [sql for sql, _ in addition.queries] == ['SELECT 1 FROM "auth_user" FOR UPDATE']
        assert "primary key" in addition.queries[0][1]
        assert not addition.tables

    def test_a_statement_the_existing_table_grant_cannot_cover_is_not_dropped(self) -> None:
        """An empty `update` is not "already covered": it grants no column."""
        manifest = self.MANIFEST.replace(
            'queries = ["SELECT \'listed\'"]',
            '[route.db.tables]\nauth_user = { select = ["id"] }',
        )
        addition = fold.plan(
            tomllib.loads(manifest),
            [
                fold._Denial(
                    route="users",
                    proxy="db",
                    sql='SELECT 1 FROM "auth_user" FOR UPDATE',
                    reason="not granted",
                )
            ],
            prefer_tables=True,
        )[0]
        assert [sql for sql, _ in addition.queries] == ['SELECT 1 FROM "auth_user" FOR UPDATE']

    def test_never_writes_a_star_for_a_lock_in_the_written_file(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(self.MANIFEST, encoding="utf-8")
        log = tmp_path / "app.log"
        log.write_text(
            self.log('SELECT * FROM "auth_user" FOR UPDATE') + "\n", encoding="utf-8"
        )
        assert (
            fold.main(
                ["fold", str(log), "--manifest", str(manifest), "--prefer-tables", "--write"]
            )
            == 0
        ), capsys.readouterr().err
        written = manifest.read_text(encoding="utf-8")
        assert "update = \"*\"" not in written, written
        parsed = tomllib.loads(written)
        assert parsed["route"][0]["db"]["queries"] == [
            "SELECT 'listed'",
            'SELECT * FROM "auth_user" FOR UPDATE',
        ]


class TestFunctionGrants:
    """An exact `queries` entry is trusted: whatever it calls runs (D3c)."""

    @pytest.mark.parametrize(
        "sql, function",
        [
            ("SELECT set_config('TimeZone', $?, false)", "set_config"),
            ("SELECT pg_read_file($?)", "pg_read_file"),
            ("SELECT lo_import($?)", "lo_import"),
            ("SELECT pg_terminate_backend($?)", "pg_terminate_backend"),
            ("SELECT unaccent($?)", "unaccent"),
            ("SELECT 'x'::regclass", "regclass"),
        ],
    )
    def test_a_function_outside_the_safe_set_is_refused(
        self, sql: str, function: str
    ) -> None:
        additions = fold.plan(document(), [denial(sql)], prefer_tables=False)
        assert additions[0].queries == [], sql
        note = additions[0].notes[0]
        assert "refused" in note and function in note, note
        assert "--allow-dangerous" in note, note

    def test_the_flag_grants_it_and_names_what_it_calls(self) -> None:
        additions = fold.plan(
            document(),
            [denial("SELECT set_config('TimeZone', $?, false)")],
            prefer_tables=False,
            allow_dangerous=True,
        )
        assert [sql for sql, _ in additions[0].queries] == [
            "SELECT set_config('TimeZone', $?, false)"
        ]
        assert any("set_config" in note for note in additions[0].notes)

    def test_a_safe_function_is_not_dangerous(self) -> None:
        additions = fold.plan(document(), [denial("SELECT lower($?)")], prefer_tables=False)
        assert [sql for sql, _ in additions[0].queries] == ["SELECT lower($?)"]
        assert additions[0].notes == []

    def test_a_multi_statement_text_is_refused_even_with_the_flag(self) -> None:
        additions = fold.plan(
            document(),
            [denial("SELECT 1; DELETE FROM notes")],
            prefer_tables=False,
            allow_dangerous=True,
        )
        assert additions[0].is_empty(), "a grant is one statement"
        assert "2 statements" in additions[0].notes[0]
        assert "split" in additions[0].notes[0]

    def test_prefer_tables_refuses_it_too(self) -> None:
        additions = fold.plan(
            document(),
            [denial("SELECT set_config('TimeZone', $?, false)")],
            prefer_tables=True,
            covering=covering_for("t"),
        )
        assert additions[0].queries == []
        assert additions[0].tables == {}, "no table grant either"
        assert any("set_config" in note for note in additions[0].notes)

    def test_the_analysis_is_what_decides(self) -> None:
        """Not the text: an injected analysis says what the statement calls."""
        additions = fold.plan(
            document(),
            [denial("SELECT 1")],
            prefer_tables=False,
            describe=lambda sql: json.dumps(
                {"kind": "select", "tables": {}, "functions": ["pg_read_file"]}
            ),
        )
        assert additions[0].queries == []
        assert "pg_read_file" in additions[0].notes[0]

    def test_a_broken_installation_is_not_a_verdict(self) -> None:
        """Only the parser's refusal is caught; anything else is the tool's problem."""
        def broken(sql: str) -> str:
            raise RuntimeError("no extension here")

        with pytest.raises(RuntimeError):
            fold.plan(document(), [denial("SELECT 1")], prefer_tables=False, classify=broken)


class TestDiagnosticsAndEdits:
    """The small things that make the tool usable, and the shapes it accepts (D3c)."""

    MANIFEST = """\
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[group.session.db]
queries = ["SELECT 'session'"]

[[route]]
name = "grouped"
path = ['/grouped/']
groups = ["session"]

[[route]]
name = "notes"
path = ['/notes/']
[route.db]
functions = [
  "unaccent",  # reads a rule file
]
queries = ["SELECT 'listed'"]
[route.db.tables]
products = { select = ["id"] }
[route.db.tables.orders]
select = ["id"]
"""

    def write(self, tmp_path: Path, sql: str, route: str, *args: str) -> tuple[int, str, str]:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(self.MANIFEST, encoding="utf-8")
        path = "/grouped/" if route == "grouped" else "/notes/"
        log = tmp_path / "app.log"
        log.write_text(
            "\n".join(
                [
                    '{"event":"request","method":"GET","path":"%s","route":"%s"}'
                    % (path, route),
                    '{"event":"sql.denied","route":"%s","proxy":"db","sql":%s,'
                    '"reason":"not granted"}' % (route, json.dumps(sql)),
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        code = fold.main(
            ["fold", str(log), "--manifest", str(manifest), *args]
        )
        return code, manifest.read_text(encoding="utf-8"), ""

    def test_a_group_only_route_says_which_group(self, tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
        code, written, _ = self.write(
            tmp_path, "SELECT 'new'", "grouped", "--write"
        )
        assert code == 1
        error = capsys.readouterr().err
        assert "'session'" in error and "group" in error, error
        assert "[route.db]" in error, "and says what to write by hand"
        assert written == self.MANIFEST, "nothing was written"

    def test_an_inline_tables_grant_is_left_alone_when_nothing_is_written_into_it(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(
            """version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "notes"
path = ['/notes/']
[route.db]
queries = ["SELECT 'listed'"]
tables = { products = { select = ["id"] } }
""",
            encoding="utf-8",
        )
        log = tmp_path / "app.log"
        log.write_text(
            "\n".join(
                [
                    json.dumps(
                        {
                            "event": "request",
                            "method": "GET",
                            "path": "/notes/",
                            "route": "notes",
                        }
                    ),
                    json.dumps(
                        {
                            "event": "sql.denied",
                            "route": "notes",
                            "proxy": "db",
                            "sql": 'SELECT "notes"."body" FROM "notes"',
                            "reason": "not granted",
                        }
                    ),
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert (
            fold.main(["fold", str(log), "--manifest", str(manifest), "--write"]) == 0
        ), capsys.readouterr().err
        parsed = tomllib.loads(manifest.read_text(encoding="utf-8"))
        assert parsed["route"][0]["db"]["queries"] == [
            "SELECT 'listed'",
            'SELECT "notes"."body" FROM "notes"',
        ]
        assert parsed["route"][0]["db"]["tables"] == {"products": {"select": ["id"]}}, (
            "the inline tables grant is left exactly as it was"
        )

    def test_a_table_written_as_its_own_header_is_updated_in_place(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        code, written, _ = self.write(
            tmp_path,
            'INSERT INTO "orders" ("id", "total") VALUES ($1, $2)',
            "notes",
            "--prefer-tables",
            "--write",
        )
        assert code == 0, capsys.readouterr().err
        assert "[route.db.tables.orders]" in written, "the header stays where it is"
        parsed = tomllib.loads(written)
        assert parsed["route"][1]["db"]["tables"]["orders"] == {
            "select": ["id"],
            "insert": ["id", "total"],
        }
        assert parsed["route"][1]["db"]["tables"]["products"] == {"select": ["id"]}

    def test_functions_are_appended_to_the_array_the_manifest_has(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        code, written, _ = self.write(
            tmp_path,
            "SELECT st_distance($1, $2)",
            "notes",
            "--prefer-tables",
            "--allow-dangerous",
            "--write",
        )
        assert code == 0, capsys.readouterr().err
        assert "# reads a rule file" in written, "the comment on the entry stays"
        parsed = tomllib.loads(written)
        assert parsed["route"][1]["db"]["functions"] == ["unaccent", "st_distance"]

    def test_the_summary_counts_the_functions(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        code, _, _ = self.write(
            tmp_path,
            "SELECT st_distance($1, $2)",
            "notes",
            "--prefer-tables",
            "--allow-dangerous",
            "--write",
        )
        assert code == 0
        assert "2 function(s)" in capsys.readouterr().err

    def test_a_control_character_is_escaped(self, tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
        sql = "SELECT 'a\x7fb', '\x01' FROM t"
        code, written, _ = self.write(tmp_path, sql, "notes", "--write")
        assert code == 0, capsys.readouterr().err
        assert "\x7f" not in written and "\x01" not in written, "no raw control character"
        assert "\\u007F" in written, written
        parsed = tomllib.loads(written)
        assert parsed["route"][1]["db"]["queries"] == ["SELECT 'listed'", sql]


class TestTheAuditProofIsNotSkipped:
    """A route the edited manifest does not route the log's requests to says so (D3c)."""

    MANIFEST = """\
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

# A catch-all *before* the route the log names: the audit routes the request
# here, so nothing of the log is checked against the later route.
[[route]]
name = "catch-all"
path = ['.*']
[route.db]
queries = ["SELECT 'catch-all'"]

[[route]]
name = "notes"
path = ['/notes/']
[route.db]
queries = ["SELECT 'listed'"]
"""

    def test_the_note_says_the_audit_routed_it_elsewhere(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        manifest = tmp_path / "blastit.toml"
        manifest.write_text(self.MANIFEST, encoding="utf-8")
        log = tmp_path / "app.log"
        log.write_text(
            "\n".join(
                [
                    json.dumps(
                        {
                            "event": "request",
                            "method": "GET",
                            "path": "/notes/",
                            "route": "notes",
                        }
                    ),
                    json.dumps(
                        {
                            "event": "sql.denied",
                            "route": "notes",
                            "proxy": "db",
                            "sql": "SELECT 'new'",
                            "reason": "not granted",
                        }
                    ),
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert (
            fold.main(["fold", str(log), "--manifest", str(manifest), "--write"]) == 0
        ), capsys.readouterr().err
        error = capsys.readouterr().err
        assert "routes none of the log's requests here" in error, error
        parsed = tomllib.loads(manifest.read_text(encoding="utf-8"))
        assert parsed["route"][1]["db"]["queries"] == ["SELECT 'listed'", "SELECT 'new'"]
