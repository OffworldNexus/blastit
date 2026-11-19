---
id: MB.40.3.3
title: "Delete tests/common/mock_pg.rs: real TCP PostgreSQL-upstream harness for both critical suites"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.40.3.2]
paths:
  - "crates/blastit/tests/**"
  - "crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs"
  - "crates/blastit-dbproxy/tests/common/**"
  - Cargo.toml
  - Cargo.lock
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T01:45"
updated: "2026-11-19T01:45"
---

# Goal
Delete the `mock_pg` test fixture (282 LOC) and its inline duplicate, replacing every call site with one named real-upstream harness that is a real `TcpListener` speaking the PostgreSQL v3 wire subset, leaving zero `MockPgServer`/`mock_pg` mentions in code or comments.

# Exact Targets (`file:line` pre-diagnosed — quoted from MB.40.3/RESULT.md §Follow-ups 4)
- `crates/blastit/tests/common/mock_pg.rs` — 282 LOC, the shared fixture; `crates/blastit/tests/common/mod.rs` is its only `mod` declaration.
- **Protected** `crates/blastit/tests/critical_dev_and_fail_closed.rs` — the sole consumer, "~40 call sites" of `MockPgServer`/`mock_pg`.
- **Protected** `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs` — "a second inline copy" of the same fixture.
- RESULT.md: "Those suites drive a real `blastit dev` child whose dbproxy dials a TCP upstream, so the fixture is genuinely an upstream PostgreSQL, not a stand-in for blastit code. Deleting it means authorizing edits to both protected files and replacing the fixture with either a real ephemeral PostgreSQL or a named, non-`Mock*` upstream fixture shared by the two suites."
- Chosen shape: one harness in `crates/blastit/tests/common/pg_upstream.rs` (`PgUpstream`), reached from both suites — a real `std::net::TcpListener` bound to `127.0.0.1:0`, real PostgreSQL v3 framing (startup + optional `AuthenticationOk`/`ParameterStatus`/`BackendKeyData`/`ReadyForQuery`, then per-statement `RowDescription`/`DataRow`/`CommandComplete`), recording the received statements and answering whatever the existing assertions already expect. `blastit-dbproxy`'s copy is removed rather than duplicated: it either calls the same source through a dev-dependency path already used by that suite or gets the identical harness in `crates/blastit-dbproxy/tests/common/`, with no two divergent copies left behind.
- Zero fakes of blastit code: dbproxy, policy and framing are the real crates; only the external PostgreSQL peer is simulated, and it is named after what it is.

# Authorization (protected critical tests)
Edits to `crates/blastit/tests/critical_dev_and_fail_closed.rs` and `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs` are authorized as an assertion-preserving fixture swap only: type/constructor/call-site renames from the `mock_pg` helper to `PgUpstream`, keeping every existing assertion, test name and count; this is never a weakening, an added `#[ignore]`, or a deletion of a test.

# Acceptance
- `make fast-check`
- `cargo test -p blastit --locked --test critical_dev_and_fail_closed`
- `cargo test -p blastit-dbproxy --locked --test critical_learn_and_redaction`
- `uv run --no-project tools/critical_guard.py --base e6b1291 --node MB.40.3.3`
- Evidence in RESULT.md: the two suite tails show the **same test count** as on `e6b1291`, and `grep -rni "mock_pg\|MockPg" .` (excluding `plan/`) returns nothing.

# Notes
Single Cargo writer: serial with the sibling slices. No new production dependency; test-only deps go in `[dev-dependencies]` and therefore into `Cargo.toml` + `Cargo.lock` in this same commit. Bind ephemeral ports only — never a fixed 5432.
