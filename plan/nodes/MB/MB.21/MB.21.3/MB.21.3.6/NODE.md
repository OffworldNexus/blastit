---
id: MB.21.3.6
title: "Parse blastfile doc fences in docs_fences, reporting both openings until the strict flip"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.21.3.5]
paths: ["crates/blastit-manifest/tests/docs_fences.rs", "crates/blastit-manifest/src/blastfile/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Teach `crates/blastit-manifest/tests/docs_fences.rs` to read ```` ```blastfile ```` fences: every
complete `Blastfile` a page shows must parse and validate, and every documented file must declare how
many complete documents it carries.

# Context

- Today the suite finds ```` ```toml title="blastit.toml" ```` fences in nine named files and parses
  each; a second opening (` ```toml `) is a fragment, except the one static block it parses under a
  minimal header. Its header comment explains the standard-tier choice (a protected copy would turn
  every documentation edit into a `critical_guard` conflict) — keep that reasoning.
- This leaf lands the **reader**, not the cutover: recognize ```` ```blastfile title="Blastfile" ````
  as a complete document, parse it with the new parser, and validate it on `UidLayout::DEFAULT_BASE`.
  While `MB.21.5.1`/`MB.21.5.2` have not migrated the pages yet, a ```` ```toml title="blastit.toml" ````
  fence must still pass — but the suite must **report the count of each opening**, so the migration
  child can flip the assertion with a one-line change instead of rewriting the suite.
- Add the negative self-tests the suite lacks today: an unclosed fence, a `blastfile` fence that does
  not parse (assert the `line:col` reaches the failure message), and a file whose count drifted.
  Use fixture strings, not the repository's own pages, so the self-tests stay green during migration.
- Keep the file count rule: a new documented file with a complete document must be added to the list,
  and a file that drops one must lower the count. `MB.21.5.2` removes the TOML opening entirely.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked --test docs_fences
$ make check
$ grep -n 'blastfile' crates/blastit-manifest/tests/docs_fences.rs
```

1. A `blastfile` fence that parses is accepted and its model validated; one that does not parse fails
   with the document's `file:line:col` in the message (self-test on an inline fixture).
2. A missing, extra or unclosed fence is refused (self-test), and the per-file count rule still holds.
3. The suite prints/reports both openings while both exist; with today's pages untouched it is green.
4. `make check` exit 0.
5. Files: `crates/blastit-manifest/tests/docs_fences.rs` and at most one small fixture addition
   (≤ 3 files, ≤ 350 changed lines).

# Notes

- Do not touch `docs/**` here — the pages migrate in `MB.21.5.1`, and this suite must be green at every
  commit in between.
- If the parser needs a helper to collect fences from a string, put it beside the parser and unit-test
  it there; the test file stays about documents.
