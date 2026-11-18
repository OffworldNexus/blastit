---
id: MB.26.46
title: "PgGrant's derived Debug prints query text: redact queries like QuerySite already redacts raw"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-manifest/src/model.rs", "crates/blastit-manifest/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

Stop `PgGrant`'s derived `Debug` from printing statement text.

`crates/blastit-manifest/src/model.rs:277` derives `Debug` for `PgGrant`, so any `{:?}` of a grant
prints `queries` verbatim — the statement text a developer wrote, which can hold a literal
(`'a@b.com'`, an e-mail, a token). `QuerySite` already has a hand-written `Debug` that redacts
`raw`; the grant's `Vec<String>` was never redacted, so the redaction added in `MB.34.4` is
bypassed by one level of nesting.

1. Hand-write `Debug for PgGrant` (or wrap `queries` in a type whose `Debug` redacts) so the
   statement text is printed through `redact_literals` (or as a count), while every other field
   keeps its current representation.
2. Test it: a grant holding `SELECT … WHERE email = 'a@b.com'` must produce a `{:?}` that contains
   neither the literal nor the raw statement, and a `tracing`/`eprintln` path that formats a grant
   must be covered by the same assertion. The test must go red if the derive comes back.
3. Audit the sibling types in the same module for the same hole (any `#[derive(Debug)]` that owns a
   `queries`/`sql`/`body` field) and fix or justify each in `RESULT.md`.

# Context

- Found by the `MB.34.4` reviewer as a MINOR and explicitly pre-existing: it is not a regression,
  which is exactly why it needs its own node rather than a line in a follow-up list.
- `lessons`: never log SQL/parameter values (skill `security-invariants` §5). A `Debug` impl is a
  log path the moment someone writes `tracing::debug!(?grant)`.
- `crates/blastit-manifest/src/model.rs` is not protected; no critical suite needs to change, but
  adding a case to an unprotected suite is not enough if the leak is only reachable via
  `Display`-adjacent paths — assert on the actual formatting output.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-manifest --locked
$ make check
```

and in `RESULT.md`: the red output of the new test with the derive restored, the `{:?}` of a
literal-bearing grant, and the audit table of the sibling types.

# Notes

- If a `Debug` on a neighbouring type prints a secret for a different reason, fix it here and say
  so; a follow-up node for one line is not worth a second gate.

# Provenance

Re-filed from `MB.38` at the `v0.2.0` gate: defence-in-depth `Debug` redaction on `PgGrant::queries` (matching `QuerySite`'s custom `Debug`).
