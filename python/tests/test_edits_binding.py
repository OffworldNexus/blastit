"""The `blastit._blastit.blastfile_apply_edits` binding (§14.5's one editor).

`blastit-manifest fold` (and anything else that plans a `Blastfile` edit in
Python) calls this binding instead of carrying a writer: the Rust editor parses,
splices, re-parses, re-lowers and compares the result with the model the edits
intended, so a returned string is already proved. These three cases pin the
contract at the PyO3 boundary — the one place a Rust test cannot reach, because
`crates/blastit-py` is a `cdylib` (see `crates/blastit-py/tests/`).

It runs wherever the package and its extension are importable: the installed
wheel in CI, this repository's staged package in `make test-critical`, or
`python/blastit/_blastit*.so` after a build. Nothing here needs a database.
"""

from __future__ import annotations

import json

import pytest
from blastit import _blastit

MANIFEST = """\
postgres $DATABASE_URL

view notes "/notes/" {
    postgres {
        query 'SELECT "notes"."id" FROM "notes"'
    }
}
"""


def edits(**grant: object) -> str:
    """A `BlastfileEdits` document with one grant and no new view."""
    document = {
        "views": [],
        "grants": [
            {
                "view": "notes",
                "store": None,
                "queries": [],
                "tables": {},
                "functions": [],
                **grant,
            }
        ],
    }
    return json.dumps(document)


def test_a_table_and_function_scope_is_written_into_the_store_block() -> None:
    """The allowed path: the scope lands as §13.1 spells it, and nothing else moves."""
    source = MANIFEST
    json_edits = edits(
        tables={
            "shop_product": {
                "select": ["id", "name"],
                "insert": None,
                "update": None,
                "delete": False,
            }
        },
        functions=["st_distance"],
    )
    edited = _blastit.blastfile_apply_edits(source, json_edits)

    assert "        functions st_distance\n" in edited, edited
    assert "        select shop_product(id, name)\n" in edited, edited
    assert '        query \'SELECT "notes"."id" FROM "notes"\'\n' in edited, edited
    # The rest of the document is the author's, byte for byte.
    assert edited.startswith('postgres $DATABASE_URL\n\nview notes "/notes/" {\n'), (
        edited
    )

    # Idempotent: the same proposal over the edited file adds nothing.
    assert _blastit.blastfile_apply_edits(edited, json_edits) == edited


def test_the_edited_file_parses_and_validates() -> None:
    """A returned string boots: `check_audit` parses, lowers and validates it."""
    json_edits = edits(
        queries=['SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $?']
    )
    edited = _blastit.blastfile_apply_edits(MANIFEST, json_edits)
    report = json.loads(_blastit.check_audit(edited, "", ""))
    assert report["schema_version"] == 1, report
    assert report["routes"]["total"] == 1, report


def test_a_document_that_is_not_edits_is_refused() -> None:
    """The denied path: bad JSON names the shape it wanted, never a statement."""
    with pytest.raises(ValueError, match="not a BlastfileEdits document"):
        _blastit.blastfile_apply_edits(MANIFEST, '{"views": "no"}')


def test_a_literal_is_refused_rather_than_written() -> None:
    """§14.5: a statement that inlines a value is never written as a grant."""
    json_edits = edits(queries=["SELECT 1 WHERE 'x' = 'y'"])
    with pytest.raises(ValueError, match="inline literal"):
        _blastit.blastfile_apply_edits(MANIFEST, json_edits)
