---
id: MB.23.9
title: "Prove T18's 200 half: a grant edit applies on save under the real PostgreSQL CI job"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.23.6]
paths: [crates/blastit/tests/critical_dev_reload.rs, .github/workflows/ci.yml]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T20:59"
updated: "2026-11-17T20:59"
---

# Goal

Prove design `T18`'s integration half: a `Blastfile` grant edit applies on save under CI's real PostgreSQL job, so a denied statement becomes `200` after exactly one `dev.reload`.

# Context

`MB.23.5.3` landed the dbproxy channel, the ack and the supervisor's document write, but its own follow-up records that the `200` half needs `BLASTIT_TEST_PG_URL` and no test reads that variable for `critical_dev_reload`. CI's `test` job already provides PostgreSQL 18 and the URL (`.github/workflows/ci.yml`), so the test can run there and be absent only when the variable is unset (never `#[ignore]`).

# Acceptance

- A two-route fixture with a postgres store and a grant: a query is denied (`403`/documented refusal), saving a `Blastfile` that adds the grant emits exactly one `dev.reload` and the next request is `200`.
- Removing the grant again denies, and the run keeps serving.
- The row is skipped only when `BLASTIT_TEST_PG_URL` is unset; with it set (CI) the row runs and fails loudly.
- `make check` (no URL) and the CI `test` job (URL) both green.

# Notes

Found by `docs/testing/AUDIT-MB.23.md` (MAJOR). Do not add a new dependency or a second transport; use the existing dbproxy connection path.
