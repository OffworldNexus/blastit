---
id: MB.21.4.5.1
title: "Retire the v1 parser suite and translate the 0.1.1 schema suite to Blastfile, re-homing every refusal"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-manifest/tests/parse.rs"
  - "crates/blastit-manifest/tests/critical_manifest_011.rs"
  - "crates/blastit-manifest/tests/blastfile_errors.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T16:57"
updated: "2026-11-16T16:57"
---

# Goal

The manifest crate's two v1-shaped suites stop testing a format that no longer exists, without losing a
single refusal a developer can still trigger.

- `crates/blastit-manifest/tests/parse.rs` is deleted with its subject: a suite whose every fixture is a
  retired document now tests nothing a `Blastfile` can reach.
- `crates/blastit-manifest/tests/critical_manifest_011.rs` keeps **every** assertion, re-spelled on
  `Blastfile`: the documented spelling that must parse and validate, and the mistake that must be refused
  with the message the documentation promises.
- Every refusal the deleted suite covered is either proved by a suite that survives (named in `RESULT.md`)
  or recorded as a spelling the new language does not have, one line each.
- `tests/blastfile_errors.rs` is the named home for a parser-level refusal that has no other home; add a
  case there rather than inventing a suite.

# Context

- Design §15.2 (the erasure) and §20 (R8 parity); `ADR-0012`. Every product entry point already reads
  `Blastfile` (`MB.21.4.1`–`MB.21.4.4`); these are the last two suites inside the crate that build a
  manifest from a retired document.
- Measured at planning: `tests/parse.rs` 1112 lines, 53 retired-document fixtures, 24 reader calls;
  `tests/critical_manifest_011.rs` 747 lines, 29 fixtures, 2 reader calls. The diff is dominated by one
  file deletion; the authored part is the schema suite's translation. Say so in `RESULT.md` next to
  `git diff --stat`.
- Translate the subject, keep the assertion. `critical_manifest_011.rs`'s families are about **policy**,
  not syntax: an inlined literal, a redacted statement, a grant on an unknown store, a `queries` entry
  that omits its parameter, a key a table silently swallows. Each has a `Blastfile` spelling and a
  `Blastfile` refusal; where the old spelling cannot express the mistake at all (a top-level key after a
  table), the refusal is now an ordinary `<file>:<line>:<col>` syntax error and the assertion moves to
  that half — message and position included.
- The suite is protected. Every assertion stays; only fixture text and the reader call change. Its first
  doc line names the retired file today and must not: `MB.21.5.2`'s guard scans `crates/**/*.rs` with no
  allowlist.
- `tests/parse.rs` imports `common::{SECTION_5_1, SECTION_6, …}`. `MB.21.4.5.3` rewrites
  `tests/common/mod.rs` and depends on this leaf, so do not touch that file here.
- Cross-leaf contract: `ManifestError::MissingVersion` / `UnsupportedVersion` are deleted by
  `MB.21.4.5.6`. No test this leaf keeps may construct them; the surviving constructions are in
  `tests/validate.rs` and `tests/mutation_gaps.rs` (`MB.21.4.5.3`).

# Acceptance

```console
$ test ! -e crates/blastit-manifest/tests/parse.rs
$ grep -rnE 'from_toml_str|toml::' crates/blastit-manifest/tests/critical_manifest_011.rs crates/blastit-manifest/tests/blastfile_errors.rs
$ grep -rnE 'blastit[.]toml' crates/blastit-manifest/tests/critical_manifest_011.rs crates/blastit-manifest/tests/blastfile_errors.rs
$ cargo test -p blastit-manifest --test critical_manifest_011 --test blastfile_errors
$ cargo test -p blastit-manifest
$ make critical-guard BASE=<sha before this node> NODE=MB.21.4.5.1
```

1. The first command exits `0`; both greps print nothing.
2. Both targeted suites pass and `critical_manifest_011` asserts at least as many cases as its 29
   fixtures did — paste the case count before and after.
3. `cargo test -p blastit-manifest` exits `0` (the reader still exists, so every other suite stays green;
   paste any suite you had to leave red and why — an empty list is the expected answer).
4. `critical-guard` is clean: no assertion was deleted, skipped or weakened to make the translation fit.
5. `RESULT.md` carries the refusal table: one row per `#[test]` deleted from `parse.rs` → the suite that
   now proves it, or `no successor: <the spelling the language does not have>` plus one line of why. A
   `no successor` row without that line is a failure.

# Notes

- Delete, do not port: `parse.rs`'s private helpers (`manifest_with`, `inline`) have no successor unless a
  surviving suite needs them.
- No new file, and do not rename `critical_manifest_011.rs` (its name is not the retired file's name, and
  a rename churns every `paths` entry in the plan).
- Do not touch `crates/blastit-manifest/src/**`: the reader, its entry points and the model fields only it
  filled are `MB.21.4.5.6`'s.
