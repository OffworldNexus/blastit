---
id: MB.21.3.2
title: "LL(1) Blastfile parser producing the existing Manifest, with the v1 model-equality oracle"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.3.1]
paths:
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_parser.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Add the LL(1) parser: Blastfile text → `Manifest`, with the same validation the TOML path has today,
so a document that parses produces exactly the model the rest of Blastit already consumes.

# Context

- Design: `docs/design/blastfile-dsl.md` §2 (first/follow, the one-token choices between the view
  forms, store block) and §8/§10. This leaf also owns `Manifest::from_blastfile_str` and the
  `parse_and_validate` twin.
- Sections to parse: `app`, `entrypoint python` (with optional `preload` block), `entrypoint php`,
  the three store forms (`postgres $URL`, `postgres <name> $URL`, `postgres [name] { host $DB_HOST; … }`),
  `trait <name> { … }`, `view` in both headers, and `static "<prefix>" { … }`.
- Refusals at `line:col`: unknown keyword at top level, unknown directive inside a block, a directive
  in the wrong section, a duplicate `app`/`entrypoint`/`view`/`trait`/store name, a string literal
  where `$IDENT` is required, `unrestricted` or `egress any` inside a `trait`, a `body` on a static-only
  mount, a `use` of an unknown trait, more than one `entrypoint`, `query` outside a grant block.
- **The equality oracle is the deliverable.** For each row of `MB.21`'s capability map, one TOML twin
  and one Blastfile twin in the protected suite, asserting `toml_manifest == blastfile_manifest`
  (routes, grants, `TableGrants`, ingress, static, php, proxies' raw `input`/`output`). This is what
  proves nothing was dropped — including `preload`, the split `host/port/user/password/database/sslmode`
  inputs, `ca_cert`, and `immutable`.
- Keep `Manifest::from_toml_str` working and untouched; `MB.21.4` removes it. Do not rename accessors.
- Tests: `crates/blastit-manifest/tests/critical_blastfile_parser.rs` (protected) + unit tests beside
  the parser for the refusal matrix.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked --test critical_blastfile_parser
$ cargo test -p blastit-manifest --locked
$ make rs-fmt
$ make rs-lint
$ make check
```

1. Every capability-map row has a passing twin pair; a reviewer can read the test and see Blastfile and
   TOML producing the same model.
2. Every refusal in the list above is a test asserting message **and** `line:col`, and no refusal
   silently defaults to a permissive value.
3. Parsing a 256 KiB `Blastfile` of many views completes without unbounded recursion (nesting is two
   block levels by grammar; assert a hostile `{{{{…` input fails cleanly instead of overflowing).
4. `make check` exit 0; the TOML path is byte-for-byte unchanged (existing suites green).
5. Files: `src/blastfile/{parser,lower}.rs` (or the design's names), `src/model.rs`'s constructor entry
   point, and the one new protected suite (≤ 8 files, ≤ 600 changed lines).

# Notes

- Reuse `validate.rs` rather than re-implementing it: a Blastfile must fail on exactly the same
  cross-document rules (`MB.3.21`'s lesson — a rebuild that forgets `static_policy`/`php` compares
  against a different route than the author wrote).
- `query 'SELECT … WHERE x = $?'` keeps the existing normalized spelling; the parser stores the raw
  string and the analyser does the `$?` work it already does.
