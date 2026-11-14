---
id: MB.3.4.2
title: "Param builder, allowlist wall and protected lockdown matrix - design MB.3.4b"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.3.4.1]
paths:
  - "crates/blastit-fastcgi/src/lib.rs"
  - "crates/blastit-fastcgi/src/lockdown.rs"
  - "crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-14T03:37"
updated: "2026-11-14T12:31"
---

# Goal

Add the param builder, the allowlist wall and `FastCgiPaths` to `crates/blastit-fastcgi`,
with a **new protected suite** that table-drives every row of the design's lockdown /
refusal matrix (design leaf `MB.3.4b`). This is the leaf that makes "a param the harness did
not compute cannot reach the wire" executable.

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.6 (param contract, canonicalisation and
  budgets), §4.7 (the binary refusal matrix) and §8's `MB.3.4.2` row; §8.2's `MB.3.4.2` row
  is the brief re-sync.
- Files (the whole leaf, 3): `crates/blastit-fastcgi/src/lib.rs`,
  `crates/blastit-fastcgi/src/lockdown.rs`,
  `crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs`.
- What lands:
  - `src/lib.rs` (created by `MB.3.4.1`) gains `pub mod lockdown;` — this leaf owns that
    file for that one line.
  - `src/lockdown.rs`: the request's param map → the exact bytes sent. Pins keys with their
    declared values, derives `PATH_INFO`/`SCRIPT_NAME`/`DOCUMENT_ROOT`, case-folds names,
    enforces count and byte budgets, drops everything the allowlist does not name, and
    returns a typed `LockdownError` (`UnknownParam`, `DuplicateParam`, `ParamsTooLarge`,
    `MalformedPath`, …) — **assert `LockdownError::*`, never `Refusal`**; the
    `Refusal`/status mapping is `MB.3.6.2.1`'s.
    `FastCgiPaths { script_filename, document_root, script_name }` is **defined here**;
    `SCRIPT_FILENAME` is pinned to its absolute `script_filename`, never a request byte.
    `HTTP_PROXY` cannot come from a `Proxy:` header (`httpoxy`); `PHP_VALUE`,
    `PHP_ADMIN_VALUE`, `PHP_ADMIN_*`, `auto_prepend_file`, `allow_url_include`,
    `open_basedir`, `disable_functions`, `REDIRECT_STATUS`/`DOCUMENT_ROOT` spoofing,
    duplicate names, NUL/over-long names and unknown names each refuse with the design's
    typed error. A header value `> 8192` bytes is **dropped** (`Param { Dropped }`), not
    refused; colliding `HTTP_*` headers keep the **first** (`200`, `Param { Dropped }`).
    `CVE-2012-1823` `-d …` is impossible by construction: assert a query string never
    becomes a param or an argument, and that the `CVE-2024-4577` best-fit/encoding probe
    never reaches a param byte.
  - `tests/critical_fastcgi_lockdown.rs`: **new protected suite**, one test per refusal row
    (table-driven, by the §4.7 named function), plus the positive case — the documented
    `public/index.php` request serialises to the exact expected param stream, `HTTP_*`
    derivation, the 128/65 535 boundaries, and row 11 (`REDIRECT_STATUS` absent from the
    stream).
  - **Row 18 is not a client-status row**: an unknown bare (non-`HTTP_`) param name has no
    HTTP client path; it is the internal builder guard `LockdownError::UnknownParam` →
    `500`, mapped by `MB.3.6.2.1`.
- **No `Cargo.toml`, no `Cargo.lock`:** the crate manifest is `MB.3.4.1`'s and the
  `[dev-dependencies] blastit-backend` edge arrives in `MB.3.6.1.2`. Keep `--locked` in
  every acceptance command.

# Acceptance

```console
$ cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked
$ cargo test -p blastit-fastcgi --locked
$ grep -q 'pub mod lockdown;' crates/blastit-fastcgi/src/lib.rs && echo "lockdown module is wired"
$ grep -c 'LockdownError' crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs
$ ! grep -n 'Refusal' crates/blastit-fastcgi/src/lockdown.rs && echo "status mapping is not this crate's"
$ make check
```

# Notes

- **Protected tests: add, never weaken.** `critical_fastcgi_lockdown.rs` is a new file (the
  guard allows additions freely). No existing `critical_*.rs` is in `paths`.
- Keep every row binary: input bytes in → typed `LockdownError` or exact param stream out.
- Budget ≤ 550 changed lines including tests. If the matrix makes that impossible, land
  every refusal row first and record the overage in `RESULT.md` — never drop a row.
- Do not connect a socket, here or in a test: I/O is `MB.3.6.2.1`'s.
