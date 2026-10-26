"""Reading blastit's event stream, and making its normalised SQL parseable (D3c).

No Django, no database, no session: `blastit.logs` is what the pytest plugin's
live server and `blastit-manifest fold` both read, and a mistake here would
quietly lose a denial or report a statement that no grant can match.
"""

import json

import pytest

from blastit import logs


def event(**fields: object) -> str:
    """One log line, as blastit writes it."""
    return json.dumps({"ts": "2026-10-25T14:02:11.384Z", "level": "info", **fields})


class TestParsing:
    """One JSON object per line, with everything else tolerated."""

    def test_events_are_found_one_per_line(self) -> None:
        text = "\n".join(
            [
                event(event="boot", component="init"),
                event(event="ready", component="init"),
            ]
        )
        assert [item["event"] for item in logs.parse_events(text.splitlines())] == [
            "boot",
            "ready",
        ]

    def test_a_blank_line_is_not_an_event(self) -> None:
        assert logs.parse_events(["", "\n", "   "]) == []

    def test_an_app_platform_prefix_is_stepped_over(self) -> None:
        # DO prefixes every forwarded line with the app and its instance (§8.1).
        line = '2026-10-25T14:02:11.384Z app[web.1]: {"event":"request","route":"notes"}'
        assert logs.parse_events([line]) == [{"event": "request", "route": "notes"}]

    def test_a_line_that_is_not_an_event_is_skipped(self) -> None:
        assert logs.parse_events(['14:02:11.384 INFO  request  GET /notes/ → notes']) == []
        assert logs.parse_events(["Traceback (most recent call last):", '{"not json": ']) == []

    def test_a_json_value_that_is_not_an_object_is_skipped(self) -> None:
        # An array, a string or a number is not an event, however valid.
        assert logs.parse_events(["[1, 2]", '"a string"', "42"]) == []

    def test_a_brace_inside_a_json_object_is_found_once(self) -> None:
        line = 'noise {"event":"app.output","line":"a { brace"} more noise'
        assert logs.parse_events([line]) == [
            {"event": "app.output", "line": "a { brace"}
        ]


class TestRequests:
    """The routing decisions, which are what a live server really served."""

    def test_routed_requests_are_deduplicated_in_log_order(self) -> None:
        events = logs.parse_events(
            [
                event(event="request", method="GET", path="/notes/", route="notes"),
                event(event="request", method="GET", path="/notes/", route="notes"),
                event(event="request", method="GET", path="/other/", route="other"),
            ]
        )
        assert logs.routed_requests(events) == [
            {"route": "notes", "method": "GET", "path": "/notes/", "count": 2},
            {"route": "other", "method": "GET", "path": "/other/", "count": 1},
        ]

    def test_an_unrouted_request_is_not_a_routed_one(self) -> None:
        events = logs.parse_events(
            [
                event(event="request", method="GET", path="/oops/", route=None),
                event(event="request", method="GET", path="/notes/", route="notes"),
            ]
        )
        assert logs.routed_requests(events) == [
            {"route": "notes", "method": "GET", "path": "/notes/", "count": 1}
        ]
        assert logs.unrouted_requests(events) == [
            {"method": "GET", "path": "/oops/", "count": 1}
        ]

    def test_a_request_event_without_a_method_is_a_get(self) -> None:
        events = logs.parse_events([event(event="request", path="/notes/", route="notes")])
        assert logs.routed_requests(events)[0]["method"] == "GET"

    def test_another_event_is_not_a_request(self) -> None:
        events = logs.parse_events([event(event="boot", component="init")])
        assert logs.routed_requests(events) == []
        assert logs.unrouted_requests(events) == []


class TestDenials:
    """The statements a route was refused, which is what a manifest is missing."""

    def test_a_denial_keeps_its_route_proxy_statement_and_reason(self) -> None:
        line = event(
            event="sql.denied",
            route="notes",
            proxy="db",
            sql="SELECT 1",
            reason="the statement is not granted",
            sqlstate="42501",
        )
        assert logs.denials(logs.parse_events([line])) == [
            {
                "route": "notes",
                "proxy": "db",
                "sql": "SELECT 1",
                "reason": "the statement is not granted",
                "count": 1,
            }
        ]

    def test_the_same_statement_denied_twice_is_one_entry(self) -> None:
        line = event(event="sql.denied", route="notes", proxy="db", sql="SELECT 1")
        denials = logs.denials(logs.parse_events([line, line]))
        assert len(denials) == 1
        assert denials[0]["count"] == 2

    def test_the_same_statement_on_two_routes_is_two_entries(self) -> None:
        first = event(event="sql.denied", route="notes", proxy="db", sql="SELECT 1")
        second = event(event="sql.denied", route="admin", proxy="db", sql="SELECT 1")
        assert len(logs.denials(logs.parse_events([first, second]))) == 2

    @pytest.mark.parametrize(
        "fields",
        [
            {"route": "notes", "sql": ""},
            {"route": "", "sql": "SELECT 1"},
            {"route": "notes"},
            {"sql": "SELECT 1"},
        ],
    )
    def test_a_denial_without_a_route_or_a_statement_is_skipped(
        self, fields: dict[str, str]
    ) -> None:
        line = event(event="sql.denied", **fields)
        assert logs.denials(logs.parse_events([line])) == []

    def test_a_missing_proxy_is_empty_not_a_guess(self) -> None:
        line = event(event="sql.denied", route="notes", sql="SELECT 1")
        assert logs.denials(logs.parse_events([line]))[0]["proxy"] == ""


class TestParseableSql:
    """The normalised text, as the parser PostgreSQL's own analysis needs (§6)."""

    def test_a_placeholder_becomes_one(self) -> None:
        assert logs.parseable_sql("SELECT 1 WHERE a = $?") == "SELECT 1 WHERE a = $1"

    def test_a_collapsed_in_list_is_a_list_again(self) -> None:
        assert (
            logs.parseable_sql('SELECT "t"."id" FROM "t" WHERE "t"."id" IN ($?, ...)')
            == 'SELECT "t"."id" FROM "t" WHERE "t"."id" IN ($1)'
        )

    def test_a_collapsed_values_run_is_two_identical_rows(self) -> None:
        # One row is a statement of its own (`bulk_create` of one object), so a
        # run the normaliser cut short has to be written as a run again.
        assert (
            logs.parseable_sql("INSERT INTO t (a) VALUES ($1), ... RETURNING t.a")
            == "INSERT INTO t (a) VALUES ($1), ($1) RETURNING t.a"
        )

    def test_a_two_row_run_keeps_its_row(self) -> None:
        assert (
            logs.parseable_sql("INSERT INTO t (a, b) VALUES ($1, $2), ...")
            == "INSERT INTO t (a, b) VALUES ($1, $2), ($1, $2)"
        )

    def test_quoted_text_is_left_alone(self) -> None:
        # A literal is data: `$?` in it is not a placeholder, and `, ...` is not
        # a collapsed list.
        sql = "SELECT '$?', ', ...' , $?"
        assert logs.parseable_sql(sql) == "SELECT '$?', ', ...' , $1"

    def test_a_doubled_quote_does_not_end_the_literal(self) -> None:
        assert logs.parseable_sql("SELECT 'it''s $?' || $?") == "SELECT 'it''s $?' || $1"

    def test_a_quoted_identifier_is_left_alone(self) -> None:
        assert logs.parseable_sql('SELECT "a$?" FROM t WHERE b = $?') == (
            'SELECT "a$?" FROM t WHERE b = $1'
        )

    def test_a_dollar_quoted_string_is_left_alone(self) -> None:
        assert logs.parseable_sql("SELECT $tag$a $? b$tag$, $?") == (
            "SELECT $tag$a $? b$tag$, $1"
        )

    def test_nothing_to_rewrite_is_left_alone(self) -> None:
        assert logs.parseable_sql("SELECT 1") == "SELECT 1"
        assert logs.parseable_sql("") == ""

    @pytest.mark.parametrize(
        "sql",
        [
            "SELECT 1 WHERE a = $?",
            'SELECT "t"."id" FROM "t" WHERE "t"."id" IN ($?, ...)',
            "INSERT INTO t (a) VALUES ($1), ... RETURNING t.a",
            "SELECT '$?' , $?",
            "UPDATE t SET a = $? WHERE b = $? AND c IN ($?, ...)",
        ],
    )
    def test_normalising_it_gives_the_text_back(self, sql: str) -> None:
        """What a grant holds is unchanged: only the parser sees the difference."""
        extension = pytest.importorskip(
            "blastit._blastit", reason="the normaliser is the compiled extension"
        )
        assert extension.normalize(logs.parseable_sql(sql)) == extension.normalize(sql)
