---
id: MB.3.4
title: "Add the pure blastit-fastcgi crate: records, params codec, CGI head parser, lockdown matrix (split: MB.3.4.1 codec, MB.3.4.2 lockdown)"
kind: epic
status: done
priority: 2
depends_on: [MB.3.3]
paths:
  - Cargo.lock
  - "crates/blastit-fastcgi/Cargo.toml"
  - "crates/blastit-fastcgi/src/lib.rs"
  - "crates/blastit-fastcgi/src/record.rs"
  - "crates/blastit-fastcgi/src/param.rs"
  - "crates/blastit-fastcgi/src/cgi.rs"
  - "crates/blastit-fastcgi/src/lockdown.rs"
  - "crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T14:22"
---

# Goal

Add the new **pure** crate `crates/blastit-fastcgi` — FastCGI record framing, the PARAMS
name-value codec, the CGI response-head parser, and the param canonicalisation +
allowlist + refusal matrix the caller applies *before a byte leaves the process*. No I/O,
no sockets, no `unsafe`, no external dependency.

Split into two sequential leaves so each stays inside the file/line budget:

| Plan id | Design leaf | Lands |
|---|---|---|
| `MB.3.4.1` | `MB.3.4a` | manifest + `Cargo.lock`, `src/{lib,record,param,cgi}.rs`, zero deps |
| `MB.3.4.2` | `MB.3.4b` | `src/lockdown.rs` + protected `tests/critical_fastcgi_lockdown.rs` |

Plan ids are numeric because `tools/plan.py` only accepts numeric segments; the design's
lettering is carried in each child's title and body.

# Context

- Grammar, record types, the complete param set and every refusal row are pinned by
  `docs/design/php-fastcgi.md` (accepted). Implement that document; do not invent a key,
  a status or a row.
- `MB.3.4.1` creates the workspace member and is the **first** `Cargo.lock` owner of the
  release chain (`MB.3.4.1` → `MB.9.1` → `MB.9.2.1` → `MB.3.6.1` → `MB.3.8`); it runs its
  acceptance **without** `--locked` once to write the lock. `MB.3.4.2` keeps `--locked`
  and touches no `Cargo.toml`/`Cargo.lock`.
- `crates/blastit-fastcgi` must stay dependency-free in production; the
  `[dev-dependencies] blastit-backend` edge (for the conformance driver) is added later,
  in `MB.3.6.1`.
- Do not touch `crates/blastit-manifest/**`, `crates/blastit-scheduler/**`,
  `crates/blastit/**`, or any protected `critical_*.rs` outside this crate — in particular
  `crates/blastit-manifest/tests/critical_static_manifest.rs` stays uneditable.

# Acceptance

- [ ] Both children `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked
$ test -z "$(cargo tree -p blastit-fastcgi -e normal --prefix none | tail -n +2)" && echo "zero dependencies"
$ ! grep -rn 'tokio\|UnixStream\|TcpStream\|std::fs\|Command::new' crates/blastit-fastcgi/src && echo "pure codec, no I/O"
$ make check
```
- [ ] No proof is taken on trust: every refusal row is a test case, and the documented
  `public/index.php` request serialises to the exact expected param stream.

# Notes

- The refusal matrix is the contract `MB.3.6.2` and `MB.3.7` rely on: a row that is not
  in the test table is not implemented.
- I/O lives in `MB.3.6.2`; do not connect a socket in this epic, not even in a test.
