---
id: MA.2.2.8
title: "Complete R8 reserved-key and $1 setup-SQL coverage in critical_manifest_011"
kind: task
status: done
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-manifest/tests/critical_manifest_011.rs"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T14:56"
updated: "2026-11-12T15:08"
---

# Goal

Close the two manifest-schema coverage holes found by mutation: the R8 reserved-key message
inside `[group.*]` / `[route.<proxy>]` bodies is unasserted, and the `$1` spelling of the
fixed setup statement is exempt only in an unprotected `blastit-py` fixture.

# Context

- Audit `docs/testing/AUDIT-MA.2.2.md`, mutants `mf.group_arm_guard_off` and
  `mf.validate_no_normalize`.
- `parse.rs::check_top_level_keys` guards `[proxy.*]`, `[group.*.*]` and `[[route]]` grants;
  `critical_manifest_011` only asserts the message for `[proxy.db]` and `[route.ingress]`.
  With the group arm's `check_grant` removed, `[group.session.db] version = 1` is still
  refused (as `UnknownKey { path: "group.session.db.version" }`) but no test notices the
  documented R8 message is gone.
- `validate.rs` normalises a grant before the `KNOWN_STATIC_SETUP_SQL` exemption, so the
  documented wire spelling `SELECT set_config('TimeZone', $1, false)` validates; the only
  oracle is `blastit_py::audit::tests` (unprotected, another crate). Removing `normalize`
  survived `critical_manifest_011`, `critical_manifest_edit` and the whole manifest crate.

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_manifest_011 --locked
```

- New negative twins: `version = 1` / `app = "a:b"` / `preload = ["m"]` inside
  `[group.<g>.<proxy>]` and inside `[route.<proxy>]` fail with exactly
  `NotTopLevel { key }` and the documented message (not `UnknownKey`).
- New positive twin: a `queries` entry spelled `SELECT set_config('TimeZone', $1, false)`
  parses, validates and keeps its `$1` text in the model (normalisation is for comparison,
  not rewriting), i.e. the exemption holds for both spellings.
- Existing reserved-key, literal and `KNOWN_STATIC_SETUP_SQL` tests stay unchanged.

# Notes

- Test-only node. Do not move `tests/docs_fences.rs` into the protected tier.
