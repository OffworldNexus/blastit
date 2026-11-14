---
id: MB.3.4.6
title: "Pin the last two unprotected branches in the fastcgi protected suites: head_end at MAX_HEAD_BYTES and a digit-leading header name"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths:
  - "crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs"
  - "crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T13:15"
updated: "2026-11-14T13:56"
---

# Goal

Two documented behaviours in `crates/blastit-fastcgi` still have no oracle after
`MB.3.4.3`-`MB.3.4.5`. Add one assertion each to the **existing** protected suites.
Test-only: no `src/` change.

# Context

- **`cgi::head_end` at the 64 KiB cap** (`src/cgi.rs:118-126`). The head ends at the
  first `\r\n\r\n`, and an end offset **equal** to `MAX_HEAD_BYTES` is accepted by
  `(end <= MAX_HEAD_BYTES).then_some(end)`. `critical_fastcgi_codec.rs::head_total_cap_is_exactly_64_kib`
  already builds a buffer of exactly `MAX_HEAD_BYTES` that ends in `\r\n\r\n`, but it
  passes that buffer straight to `parse_head`; nothing calls `head_end` on it.
  `cargo-mutants` 27.1.0 does not generate a `<=` -> `<` mutant here (the gate audit's
  `mutants.out` contains only `replace <= with >`, which is caught), so the boundary is
  unguarded. A `<=` -> `<` regression makes a legal, exactly-64 KiB head read as
  incomplete (the scheduler never sees a terminator and faults a legal response).
- **A digit-leading request header name** (`src/lockdown.rs:412-422`). Section 4.6.2
  says a folded name that is empty, **starts with a digit/`_`**, or contains any other
  byte is dropped. `critical_fastcgi_lockdown.rs::unrepresentable_header_name_is_dropped`
  covers `X.Foo`, `-x` (folds to `_X`) and non-ASCII, but no digit-leading name - and
  `1x` is a legal RFC 7230 token from the HTTP front end. A regression that accepted
  `A-Z0-9` as the first byte would emit `HTTP_1X` with every existing test still green.

# Acceptance

- `cargo test -p blastit-fastcgi --test critical_fastcgi_codec --locked` and
  `cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked` pass.
- In `critical_fastcgi_codec.rs::head_total_cap_is_exactly_64_kib` (or a sibling test in
  that file): `assert_eq!(head_end(&at_cap), Some(at_cap.len()))` on the existing at-cap
  buffer, and `head_end(&over_cap) == None`. Keep the existing `parse_head` assertions.
- In `critical_fastcgi_lockdown.rs::unrepresentable_header_name_is_dropped`: add
  `(b"1x", b"4")` and assert `HTTP_1X` is in `dropped()` and adds no pair (the fixed
  count stays 16). Optionally also add the builder-API-only empty name `(b"", b"4")`
  (`HTTP_` dropped), which pins the `None` arm of `is_representable_header`.
- `make check` passes; `cargo-mutants -p blastit-fastcgi` still reports the same three
  tolerated survivors (this node adds no source, so no new mutant is expected).
- Do not weaken or delete an existing assertion. Both files are already in this node's
  `paths`, so the protected-file modification is authorized.

# Notes

- Both behaviours are **already correct**; this node is oracle coverage only. Do not
  change `src/`.
- Do not add a getter-only assertion: `Params::is_empty -> true` remains a tolerated,
  deliberately-untested survivor (no production caller; the `false` direction is
  already caught by the `-> false` mutant).
