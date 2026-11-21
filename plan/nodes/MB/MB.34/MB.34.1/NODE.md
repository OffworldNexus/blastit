---
id: MB.34.1
title: "Document SQL template validation, the exact squiggle, the Parameterize Quick Fix and $?/... hover on the reference, local, CI and troubleshooting pages"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: [MB.31]
paths: [docs/reference/blastfile.md, docs/local.md, docs/ci.md, docs/troubleshooting.md]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-18T05:02"
---

# Goal

Write the words a developer reads **before** the validator exists, so the behaviour is decided
in prose and the implementation only has to make the page true.

One page each, and no new fences (the 33-block inventory pinned by `MB.32.1` must not move):

1. **`docs/reference/blastfile.md`** — the `query` row and its bullets state the new contract:
   - an exact `query` must be **one** PostgreSQL statement Blastit can parse; a typo is a
     `Blastfile:line:col` diagnostic at the offending token, not a runtime `sql.denied`;
   - `$?` is one bind placeholder per value, `IN ($?, ...)` is a placeholder list of any
     length, and `(row), ...` repeats identical `VALUES` rows — say what each expands to on the
     wire (`$1, $2, …`, `IN ($1, $2, …)`, one row) in one sentence each;
   - an empty entry, a second `;`-separated statement and a stray `...` outside those two
     positions are refused, with the sentence the CLI prints;
   - the inline-literal refusal now says *where* the literal is, and `KNOWN_STATIC_SETUP_SQL`
     (`SET TIME ZONE 'UTC'`, `set_config(…)`) remains the one exemption.
2. **`docs/local.md` `## Editors`** — the squiggle lands on the SQL token; the Quick Fix
   `"Parameterize SQL for Blastfile ($?)"` rewrites a pasted statement (quoted literals or
   `$1…$n`) into `$?`/`IN ($?, ...)`; hover over `$?`/`...` explains the binding. Keep step 2's
   one-sentence shape and do not add a fence.
3. **`docs/ci.md`** — `blastit check` catches the same thing in CI: one row in the findings table
   or the failure prose naming an unparseable `query`, and `--json` keeps carrying the finding
   it already carries (no new `fix.action`).
4. **`docs/troubleshooting.md`** — one row: *"a `query` squiggle I cannot find"* → the statement
   is validated after `$?` expansion, so count `IN ($?, ...)` as one value and copy the SQL into
   `psql -c` to see PostgreSQL's own error.

Then have a **fresh-context** `critic` review the diff through the `dx` and `ai-ergonomics`
lenses (the same goldfish protocol as `MB.22.1`/`MB.31.1`); record `VERDICT: PASS`, the lens, and
each finding's resolution in `RESULT.md`. A finding that changes a sentence is fixed here, not
deferred to the designer.

# Context

- **This is the DX-first gate.** `MB.34.2` (design) reads these sentences as its input, and
  `MB.34.3`–`MB.34.6` implement them. If a sentence here cannot be implemented, the epic
  returns to this leaf — do not paper over it in code.
- **The authority for the grammar already exists.** `crates/blastit-sqlpolicy/src/lib.rs:1-30`
  and `docs/reference/blastfile.md:258-270` define normalisation: comments become one space,
  `$1` and `$?` are the same, `IN` lists collapse, `VALUES` rows collapse. This leaf documents
  what the *editor* adds (a parse), not a new SQL dialect.
- **No new fences.** `python/tests/test_docs_assets.py` (after `MB.32.1`) pins the exact set of
  `text` blocks and `crates/blastit-manifest/tests/docs_fences.rs` pins the console commands;
  a new block is a different node's change. Use inline code for the `psql` advice.
- **Tone.** `make doc-tone` refuses apology language; no "sorry", no "unfortunately", no
  kernel/WASM lecture. `docs/local.md` step 2 is one sentence today — keep it one or two.
- **Do not touch** `docs/design/**`, `docs/decisions/**`, the nav, or any `crates/**` file.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ make doc-tone docs-check
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
$ git diff --stat -- docs
```

plus, in `RESULT.md`: the critic report (lens, verdict, findings), the list of pages touched,
and the `git diff` of the four sentences that define the contract (`$?`, `IN ($?, ...)`,
`(row), ...`, the single-statement rule).

The leaf is done when a reader can answer, from these pages alone: *which* part of a `query` is
validated, *where* the squiggle appears, *what* `$?`/`...` expand to, and *what* the Quick Fix
will rewrite.

# Notes

- If the reference page needs a table row rather than prose, add a row — do not add a fence.
- The inline-literal exemption must read as one sentence with an example (`SET TIME ZONE 'UTC'`),
  pointing at `docs/reference/blastfile.md#traits` for `set_config`.
- A `text`-block count change is a red `test_docs_assets.py`; that is the check working, not an
  obstacle to route around.
