"""The plugin's fixture and terminal strings, proved without a PostgreSQL (D3b).

`python/tests/test_pytest_plugin.py` needs a real server: its sessions open a
database, and CI sets `DATABASE_URL`. These tests need neither Django nor a
database, so they run everywhere and keep the CI path honest — the fixture the
plugin suite reads must parse through the one reader, the writer the report-repair
loop uses must lower back to the model it was given, the terminal strings the
skipped suite pins must be what `_summary`/`_finish` really print, and the
fixture's `LIMIT 1` must be the one the pinned `unused` list names. If any of
those breaks, the plugin suite's 39 live cases would fail for a reason nobody
could see on a machine without PostgreSQL.
"""

import json
from pathlib import Path
from types import SimpleNamespace

import pytest
from blastit import _blastit
from blastit import pytest_plugin as plugin

HERE = Path(__file__).parent
PROJECT = HERE / "project"

#: The views the fixture declares, in routing order.
VIEWS = [
    "notes",
    "note-create",
    "note-delete",
    "note-atomic",
    "note-in",
    "note-multi",
    "note-bulk",
    "note-types",
    "note-stream",
    "note-atomic-live",
    "note-cache",
    "note-queue",
    "orders",
]


def model(source: str) -> dict:
    """The one reader's lowered model, as `blastit check` would read it."""
    return json.loads(_blastit.blastfile_model(source))


def fixture() -> str:
    """The fixture's `Blastfile`, as the plugin suite hands it over."""
    return (PROJECT / "Blastfile").read_text(encoding="utf-8")


def test_the_fixture_is_a_blastfile_the_one_reader_accepts() -> None:
    parsed = model(fixture())
    assert [store["name"] for store in parsed["stores"]] == ["db"]
    assert [view["name"] for view in parsed["views"]] == VIEWS
    assert [path.name for path in PROJECT.glob("Blastfile*")] == ["Blastfile"], (
        "the fixture is one Blastfile, and nothing else sits beside it"
    )


def test_the_repair_writer_round_trips_the_fixture() -> None:
    """`dumps_blastfile` writes a document that lowers back to the same model."""
    import test_pytest_plugin as harness

    parsed = model(fixture())
    again = model(harness.dumps_blastfile(parsed))
    assert again == parsed


def test_a_repaired_model_carries_every_fix_the_report_names() -> None:
    """The repair's directives are `Blastfile`: `transactions`, `egress any`,
    `trait connection` + `use connection`, and a `query "…"` with a literal."""
    import test_pytest_plugin as harness

    delete_sql = 'DELETE FROM "notes_note" WHERE "notes_note"."id" = $?'
    report = {
        "routes": [
            {
                "name": "note-delete",
                "proxies": [
                    {
                        "name": "db",
                        "missing": [{"sql": delete_sql}],
                        "transaction_control": {"needed": False},
                    }
                ],
                "egress": {"needed": False},
            },
            {
                "name": "note-atomic",
                "proxies": [
                    {
                        "name": "db",
                        "missing": [],
                        "transaction_control": {"needed": True},
                    }
                ],
                "egress": {"needed": False},
            },
            {
                "name": "note-queue",
                "proxies": [],
                "egress": {"needed": True},
            },
        ],
        "connection_setup": {
            "statements": [
                {"sql": "SET TIME ZONE 'UTC'", "ungranted_in": ["notes"]},
                {"sql": harness.APP_SETUP_SQL, "ungranted_in": ["notes"]},
            ]
        },
        "unrouted": [{"path_info": "/oops/"}],
    }
    written = harness.repaired_manifest(fixture(), SimpleNamespace(report=report))

    again = model(written)
    repaired = {view["name"]: view for view in again["views"]}
    assert repaired["note-delete"]["grants"]["db"]["queries"][-1] == delete_sql
    assert repaired["note-atomic"]["grants"]["db"]["transactions"] is True
    assert repaired["note-queue"]["egress"] is True
    assert again["traits"]["connection"]["db"]["queries"] == [
        "SET TIME ZONE 'UTC'",
    ], "the grantable setup statement is in the trait; the inlined one is not"
    assert all("connection" in view["use"] for view in again["views"]), (
        "every view uses the trait the repair wrote, the one it created included"
    )
    assert again["views"][-1]["name"] == "fallback"
    assert again["views"][-1]["use"] == ["connection"], (
        "the view the repair creates opens the store, so it uses the trait too: "
        "without it the next report names the setup statement ungranted there"
    )
    assert again["views"][-1]["patterns"] == ["/oops/"]
    assert "query \"SET TIME ZONE 'UTC'\"" in written, (
        'a `\'`-bearing statement is written as a `"…"` string'
    )


def test_the_fixture_grants_the_stale_limit_one_the_suite_pins() -> None:
    """The pinned `unused` list is the fixture's own grant, `LIMIT 1` included.

    `test_pytest_plugin.py` pins `note-delete`'s stale grant by its exact text;
    normalisation keeps the constant in `LIMIT 1`, so a fixture that said
    `LIMIT 21` would report a different `unused` list in CI while every local run
    skipped past it.
    """
    import test_pytest_plugin as harness

    views = {view["name"]: view for view in model(fixture())["views"]}
    stale = views["note-delete"]["grants"]["db"]["queries"][0]
    assert stale == harness.STALE_GRANT, "the pinned unused list is this grant"
    assert _blastit.normalize(stale) == harness.STALE_GRANT
    grants = "\n".join(
        query
        for view in views.values()
        for query in (view.get("grants", {}).get("db") or {}).get("queries", [])
    )
    assert grants.count("LIMIT 1") == 2, (
        "note-delete's stale grant and orders' grant are the two LIMITs"
    )
    assert "LIMIT 21" not in grants


def test_the_summary_hints_name_directives_the_reader_accepts() -> None:
    """The actionable lines `_summary` prints, rendered from the real producer."""
    report = {
        "totals": {
            "requests": 1,
            "statements": 1,
            "routes_exercised": 1,
            "routes": 1,
            "clean": False,
            "unused_global": 0,
            "unrouted": 0,
            "egress": 1,
            "unrecognised": 0,
            "connection_setup": 1,
            "connection_setup_ungranted": 1,
            "plugin_warnings": 0,
        },
        "routes": [
            {
                "name": "atomic",
                "proxies": [
                    {
                        "name": "db",
                        "missing": [],
                        "unused": [],
                        "table_grants": {"tables": {}, "functions": []},
                    }
                ],
                "needs_transaction_control": True,
                "egress": {"observed": [], "needed": False},
            },
            {
                "name": "queue",
                "proxies": [
                    {
                        "name": "db",
                        "missing": [{"sql": "DELETE FROM notes_note"}],
                        "unused": [],
                        "table_grants": {
                            "tables": {
                                "notes_note": {
                                    "select": ["id"],
                                    "insert": None,
                                    "update": None,
                                    "delete": True,
                                }
                            },
                            "functions": [],
                        },
                    }
                ],
                "needs_transaction_control": False,
                "egress": {"observed": [{"target": "127.0.0.1:9"}], "needed": True},
            },
        ],
        "unrouted": [],
        "unexercised_routes": [],
        "connection_setup": {"ungranted": 1},
        "unrecognised": {"count": 0},
    }
    text = "\n".join(
        line for line, _ in plugin._summary(report, "blastit-report.json", True)
    )
    assert "needs transactions (add `transactions` to its postgres block)" in text
    assert "but has no egress any" in text
    assert "put them in a trait attached to every view" in text
    for retired in (
        "transaction_control = true",
        "egress = true",
        "group attached to every route",
        "[route.db.tables]",
    ):
        assert retired not in text, retired
    assert "global group" not in text, "a Blastfile has no global groups"
    assert "select notes_note(id)\ndelete notes_note" in text


def test_the_strict_line_names_the_directives_the_file_spells(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The exact strict sentence the live suite pins, rendered by `_finish`."""
    report = {
        "totals": {
            "missing": 0,
            "unrouted": 0,
            "connection_setup_ungranted": 1,
            "egress": 0,
            "unrecognised": 0,
            "plugin_warnings": 0,
            "clean": False,
        }
    }
    printed: list[tuple[str, bool]] = []
    monkeypatch.setattr(plugin, "_merged_observations", lambda state: {})
    monkeypatch.setattr(plugin, "_audit", lambda state, document: report)
    monkeypatch.setattr(plugin, "_summary", lambda *args, **kwargs: [])
    monkeypatch.setattr(plugin, "_write_json", lambda path, document: None)
    monkeypatch.setattr(
        plugin, "_print_lines", lambda config, lines: printed.extend(lines)
    )
    state = SimpleNamespace(
        installed=True,
        deferred=None,
        unmapped=set(),
        warnings=[],
        problems=[],
        strict=True,
        prefer_tables=False,
        report_path=Path("blastit-report.json"),
    )
    session = SimpleNamespace(config=object(), exitstatus=0)

    plugin._finish(session, state)

    text = "\n".join(line for line, _ in printed)
    assert (
        "blastit: --blastit-strict: 0 statement execution(s) missing, 0 unrouted "
        "request(s), 1 connection-setup statement(s) not granted in every route, "
        "0 route(s) opening connections without egress any, 0 unreadable "
        "statement(s) and 0 warning(s)" in text
    ), text
    assert "egress = true" not in text
    assert session.exitstatus == pytest.ExitCode.TESTS_FAILED


def test_the_fixture_renders_the_pinned_unused_and_totals_through_the_audit() -> None:
    """The two request shapes `test_pytest_plugin.py` pins, through the real audit.

    The audit is pure: no PostgreSQL is involved. Sending it the request the
    suite makes for each of the two views whose grants are partly stale renders
    the `unused` list and `totals["unused"]` the live suite asserts — so a
    fixture that said `LIMIT 21` (a different grant text, hence a different
    `unused` line) fails here instead of in CI behind a skip.
    """
    import test_pytest_plugin as harness

    views = {view["name"]: view for view in model(fixture())["views"]}
    insert = views["note-create"]["grants"]["db"]["queries"][0]
    observations = {
        "version": 1,
        "requests": [
            {
                "method": "POST",
                "path_info": "/notes/4/delete/",
                "statements": [
                    {
                        "alias": "default",
                        "proxy": "db",
                        "sql": 'DELETE FROM "notes_note" WHERE "notes_note"."id" = $?',
                    }
                ],
            },
            {
                "method": "POST",
                "path_info": "/notes/create/",
                "statements": [{"alias": "default", "proxy": "db", "sql": insert}],
            },
        ],
    }
    report = json.loads(_blastit.audit(fixture(), json.dumps(observations)))
    routes = {route["name"]: route for route in report["routes"]}
    assert routes["note-delete"]["proxies"][0]["unused"] == [harness.STALE_GRANT]
    assert routes["note-create"]["proxies"][0]["unused"] == [
        views["note-create"]["grants"]["db"]["queries"][1]
    ], "the body SELECT the INSERT grant does not cover"
    assert report["totals"]["unused"] == 2
    assert report["totals"]["missing"] == 1, "the DELETE the fixture does not grant"
