---
id: MB.3.4
title: "Add the pure blastit-fastcgi crate: record framing, params codec, lockdown matrix"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.3]
paths:
  - Cargo.lock
  - "crates/blastit-fastcgi/Cargo.toml"
  - "crates/blastit-fastcgi/src/lib.rs"
  - "crates/blastit-fastcgi/src/record.rs"
  - "crates/blastit-fastcgi/src/param.rs"
  - "crates/blastit-fastcgi/src/lockdown.rs"
  - "crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T01:09"
---

# Goal

Add the new **pure** crate `crates/blastit-fastcgi`: FastCGI record framing, PARAMS
name-value encoding/decoding, and the param canonicalisation + allowlist + refusal
matrix the caller applies *before a byte leaves the process*. No I/O, no sockets, no
`unsafe`, no new external dependency — the session and the pump are `MB.3.6`.

# Context

- Grammar, record types, the complete param set and every refusal row are pinned by
  `docs/design/php-fastcgi.md` (accepted). Implement that document; do not invent a
  key, a status, or a row.
- Files (this is the whole crate):
  - `Cargo.toml`: `version.workspace = true`, `edition.workspace = true`,
    `rust-version.workspace = true`, `license.workspace = true`, `publish.workspace =
    true`, `lints.workspace = true`. **No dependencies** unless the design proves one
    is unavoidable; if so, stop and log a follow-up instead of editing the workspace
    manifest.
  - `src/lib.rs`: crate docs, module declarations, the `Error` enum (no `thiserror`;
    hand-written `Display`), size constants (`FCGI_VERSION_1`, `FCGI_MAX_CONTENT`,
    the param byte budget).
  - `src/record.rs`: encode/decode of the version-1 header and the record types the
    design lists (`BEGIN_REQUEST`, `PARAMS`, `STDIN`, `STDOUT`, `STDERR`,
    `END_REQUEST`, `ABORT_REQUEST`), including padding to 8 bytes, `u16`
    `contentLength` refusal above the cap, and a decoder that is total (a truncated or
    over-long record is an error, never a panic or an over-read).
  - `src/param.rs`: name-value **encoding** (1-byte length below 128, 4-byte with the
    high bit otherwise) and **decoding** of a stream, with the terminator.
  - `src/lockdown.rs`: the request's param map -> the exact bytes sent. Pins keys with
    their declared values, derives `PATH_INFO`/`SCRIPT_NAME`/`DOCUMENT_ROOT`,
    case-folds names, enforces the count and byte budgets, drops everything the
    allowlist does not name, and returns a typed refusal carrying the design's status
    for each row. `HTTP_PROXY` cannot come from a `Proxy:` header: state the rule in
    a doc comment and enforce it in code.
  - `tests/critical_fastcgi_lockdown.rs`: **new protected suite**, one test per row of
    the design's refusal matrix (table-driven, one `#[test]` iterating rows or one test
    per row), plus the positive case: the documented `public/index.php` request
    serialises to the exact expected param stream, and the record framing round-trips
    every type.
- Unit tests for the codec live in-module (`#[cfg(test)]`), not in a second test file:
  the file list above is the budget.
- `Cargo.lock` is yours because a new workspace member adds a lock entry. Nothing else
  in the release tooling does: root `Cargo.toml` needs no edit (`members = ["crates/*"]`).

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked
$ CARGO_NET_OFFLINE=true cargo clippy -p blastit-fastcgi --all-targets --locked -- -D warnings
$ make rs-lint
$ make critical-guard BASE=develop NODE=MB.3.4
$ grep -q 'forbid(unsafe_code)' crates/blastit-fastcgi/src/lib.rs && echo "crate is safe-only"
$ make check
```

# Notes

- **Protected tests: add, never weaken.** `critical_fastcgi_lockdown.rs` is a new file
  (the guard allows additions). If an existing `critical_*.rs` in another crate would
  have to change, stop and log a follow-up — it is not in `paths`.
- The refusal matrix is the contract `MB.3.6` and `MB.3.7` rely on: a row that is not
  in the test table is not implemented. Keep every row binary (input bytes in, typed
  refusal or exact param stream out).
- Budget: the whole crate under ~600 changed lines *including* tests. If the design's
  matrix makes that impossible, land the codec and every refusal row first, then log
  the overage in `RESULT.md` — never drop a row to fit.
- Do not touch `crates/blastit-manifest/**`, `crates/blastit-scheduler/**`,
  `crates/blastit/**`, or any other workspace crate. Do not connect a socket here, not
  even in a test: the design puts I/O in `MB.3.6`.
- Escalate through `plan.py --agent implementer human add "…" --node MB.3.4` if the
  design contradicts itself; do not pick a side silently.
