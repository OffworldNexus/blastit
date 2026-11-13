---
id: MA.2.2.5
title: Protect dbproxy socket identity (uid mismatch) in the critical suite
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T14:56"
updated: "2026-11-12T15:08"
---

# Goal

Pin the socket-derived identity check (`blastit_dbproxy::state::credentials_match`) in the
protected critical suite, not only in in-module unit tests.

# Context

- Audit `docs/testing/AUDIT-MA.2.2.md` (mutant `db.credentials_any_uid`, mutation
  `root_allowed && credentials.uid == spec_uid` -> `root_allowed`). The mutation survived
  `critical_learn_and_redaction` and was killed only by three unprotected unit tests
  (`state::tests::only_the_listeners_own_uid_is_accepted`,
  `state::tests::a_root_ci_container_may_opt_into_a_root_listener`,
  `hooks::tests::a_stranger_is_refused_and_logged`).
- `security-invariants` §3: identity comes from which per-route listener accepted the
  connection plus `SCM_CREDENTIALS`; a broken uid check lets a route use another route's
  broader grants. That boundary belongs in the protected tier.
- The harness already exists in `critical_learn_and_redaction.rs` (`start`, `RawClient`,
  `listener_spec`, `current_uid`, `MockPgServer`); it is unprivileged and needs no root.

# Acceptance

```console
$ cargo test -p blastit-dbproxy --test critical_learn_and_redaction --locked
```

- New test: a listener that declares `current_uid() + 1` refuses a client running as
  `current_uid()` with an `ErrorResponse` SQLSTATE `28000` (or the documented credentials
  SQLSTATE), the mock upstream receives no startup packet and no statement, and no
  `sql.denied`/observation line is produced.
- The accepted twin stays: a listener declaring `current_uid()` serves the client.
- No `#[ignore]`; the file is protected and this node owns its path.

# Notes

- Do not change production code. If the harness cannot express "another uid" without root,
  assert the refusal with the listener uid set to a different, non-zero value.
