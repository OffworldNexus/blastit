---
id: MB.3.4.1
title: "Create the pure blastit-fastcgi crate: manifest, records, name-value codec, CGI head parser - design MB.3.4a"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.3.3]
paths:
  - Cargo.lock
  - "crates/blastit-fastcgi/Cargo.toml"
  - "crates/blastit-fastcgi/src/lib.rs"
  - "crates/blastit-fastcgi/src/record.rs"
  - "crates/blastit-fastcgi/src/param.rs"
  - "crates/blastit-fastcgi/src/cgi.rs"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-14T03:37"
updated: "2026-11-14T11:55"
---

# Goal

Create the **pure** crate `crates/blastit-fastcgi` (design leaf `MB.3.4a`): the workspace
member, record framing, the PARAMS name-value codec and the CGI response-head parser. No
I/O, no sockets, no `unsafe`, **zero production dependencies** and no `[dev-dependencies]`
yet (the `blastit-backend` dev-dependency arrives in `MB.3.6.1.2`).

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.5.2 and §5 pin the record grammar, the
  caps and the CGI-head rules; §8's `MB.3.4.1` row is this leaf; §8.2's `MB.3.4.1` row is
  the brief re-sync. Do not invent a record type, a constant or a refusal.
- Files (the whole leaf, 6): `Cargo.lock`, `crates/blastit-fastcgi/Cargo.toml`,
  `crates/blastit-fastcgi/src/{lib,record,param,cgi}.rs`.
- What lands:
  - `Cargo.toml`: `version.workspace`/`edition.workspace`/`rust-version.workspace`/
    `license.workspace`/`publish.workspace`/`lints.workspace`, **no dependencies**.
  - `src/lib.rs`: `#![forbid(unsafe_code)]`, crate docs, module declarations, the
    hand-written `Error` enum and `Display` (no `thiserror`), `FCGI_VERSION_1`,
    `FCGI_MAX_CONTENT`, the param byte budget.
  - `src/record.rs`: version-1 header encode/decode for `BEGIN_REQUEST`, `PARAMS`, `STDIN`,
    `STDOUT`, `STDERR`, `END_REQUEST`, `ABORT_REQUEST`; 8-byte padding; `contentLength`
    above the cap is an error; the decoder is total (a truncated or over-long record is an
    error, never a panic or an over-read).
  - `src/param.rs`: name-value **encoding** (1-byte length below 128, 4-byte with the high
    bit otherwise) and **decoding** of a stream, including the empty-name terminator.
  - `src/cgi.rs`: the CGI response-head parser — `Status:` accepts **`200..=599` only**
    (`1xx`/out-of-range → typed fault, mapped to `502` by the caller); header values refuse
    `0x00..=0x08`, `0x0a..=0x1f` and `0x7f`; header-count/byte caps; **the proxy-control
    header denial** — `cgi.rs` refuses exactly `X-Accel-Redirect`, `X-Sendfile`,
    `X-Lighttpd-Send-File`, `X-Rewrite-URL` (`Fault`) and **drops** every other `X-Accel-*`
    name (`X-Accel-Buffering` → `200`, header absent); the `Content-Type` default rule lives
    here (hop-by-hop stripping stays in the scheduler). A non-token header name
    (`X-Accel-Redirect\x00: /x`, ` X-Accel-Redirect: /x`) is a `Fault`.
- Unit tests live in-module (`#[cfg(test)]`), not in a second file: the file list is the
  budget.
- **`Cargo.lock` is yours and this is the first link of the release chain**
  (`MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` → `MB.3.6.1.2` → `MB.3.8`). A new workspace member
  adds a lock entry, so run the first build **without `--locked`** to write `Cargo.lock`,
  then keep the updated lock in the change. Root `Cargo.toml` needs no edit
  (`members = ["crates/*"]`).

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi          # no --locked: this leaf adds a workspace member and writes Cargo.lock
$ cargo test -p blastit-fastcgi --locked
$ test -z "$(cargo tree -p blastit-fastcgi -e normal --prefix none | tail -n +2)" && echo "zero dependencies"
$ grep -q 'forbid(unsafe_code)' crates/blastit-fastcgi/src/lib.rs && echo "crate is safe-only"
$ ! grep -rn 'tokio\|UnixStream\|TcpStream\|std::fs\|Command::new' crates/blastit-fastcgi/src && echo "pure codec, no I/O"
$ git diff --stat Cargo.lock && echo "lock updated in this leaf"
$ make check
```

# Notes

- `src/lockdown.rs` and `tests/critical_fastcgi_lockdown.rs` are **`MB.3.4.2`'s**: do not
  pre-create them, even empty, or the next leaf has to edit your files.
- Do not touch `crates/blastit-manifest/**`, `crates/blastit-scheduler/**`,
  `crates/blastit/**`, or any protected `critical_*.rs`. No socket in this leaf, not even in
  a test.
- Budget: ≤ 550 changed lines including in-module tests. Land every record type first;
  never drop a cap to fit.
