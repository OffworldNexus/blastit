---
id: MA.2.2.2
title: "Leaf 5: surgical comment-preserving manifest editor and atomic write"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MA.2.2.1]
paths:
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/edit.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T13:12"
---

# Goal

Implement `crates/blastit-manifest/src/edit.rs`: a pure-Rust, comment- and formatting-preserving surgical editor (`apply_edits`) plus an atomic writer (`write_atomic`), proven by a protected critical suite that includes the catch-all insertion cases and temp-file cleanup.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §4.2 B (`ProposedRoute`, `ProposedGrant`, `ManifestEdits`, `apply_edits`, `write_atomic`), §7.2 row `R29 (edit)`, §8 Leaf 5; `ADR-0003`; requirements `R28` (insertion point), `R29`.
- **Must run after `MA.2.2.1`** because both leaves touch `crates/blastit-manifest/src/lib.rs`.
- `apply_edits` parses first, locates the insertion index immediately above the first `is_unrestricted_catch_all()` route (both `path = ".*"` and `path = ['.*']`) or at EOF, edits line-by-line, then re-parses and proves every untouched field is unchanged.
- `write_atomic` writes `.{filename}.{pid}.tmp` in the target directory, `fsync`s the file, `rename`s, `fsync`s the directory, and leaves no temp file on success or failure.
- Load skills `rust-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_manifest_edit --locked
$ cargo test -p blastit-manifest --locked
$ make check
```

- `crates/blastit-manifest/tests/critical_manifest_edit.rs` is **new**: comments/ordering/quoting survive an edit; a route is inserted before a `path = ".*"` catch-all and before a `path = ['.*']` variant; repeated edits are idempotent; `write_atomic` leaves no `.tmp` file and the file parses after the write; a simulated failure cleans the temp file.
- Round-tripping `apply_edits` output through `Manifest::parse_and_validate` succeeds for every `ManifestEdits` fixture (new route names/paths and exact normalized `queries`, including a query with `$?` placeholders).

# Notes

- ~340 changed lines. `apply_edits` must not depend on `toml_edit` spans for the reserved-key guard (that remains `parse.rs`'s job from `MA.2.2.1`).
- No Python `fold.py` changes here (Leaf `MA.2.3.1`); no user-doc edits (`MA.3`).
