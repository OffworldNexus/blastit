---
id: MB.3.5
title: "Parse and validate [route.fastcgi] blocks in blastit-manifest"
kind: epic
status: ready
priority: 2
depends_on: [MB.3.4]
paths:
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/probe.rs"
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - "crates/blastit-manifest/tests/critical_fastcgi_manifest.rs"
  - crates/blastit/src/probe.rs
  - crates/blastit/src/lib.rs
  - crates/blastit/src/supervise.rs
  - docs/reference/manifest.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T10:51"
---

# Goal

Teach `blastit-manifest` the `[route.fastcgi]` grammar exactly as the design pins it, add
the pure `SocketProbe` seam and its two implementations, and document the block — split
into two sequential leaves that each stay inside the file/line budget.

| Plan id | Design leaf | Lands |
|---|---|---|
| `MB.3.5.1` | `MB.3.5` part 1 | `FastCgiPolicy`, `Executor` (`ALL_KINDS`, `kind()`), crate-root re-exports + `lib.rs` doctest, `Route::fastcgi()`, `route_lines()`, both predicates, the error variants, `docs_fences.rs` `7 → 8` |
| `MB.3.5.2` | `MB.3.5` part 2 | `blastit-manifest/src/probe.rs` (`SocketProbe`, `SocketProbeError`, `ProbeUid`, `check_fastcgi_entries`, `check_fastcgi_sockets`), `crates/blastit/src/{probe,lib,supervise}.rs`, protected `critical_fastcgi_manifest.rs`, all three `docs/reference/manifest.md` edits |

Plan ids are numeric because `tools/plan.py` only accepts numeric segments.

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.1 (grammar, defaults, boot-fatal list,
  dead-declaration rule, the three `manifest.md` edits), §4.2 "one predicate, one
  spelling", §4.2.1 (`Executor`/`FastCgiPolicy` derives), §8's two rows and §8.2's
  `MB.3.5` row.
- **`render_pool_conf` does not live here.** It is `crates/blastit/src/supervise.rs`,
  owned by `MB.3.6.3.2`; this epic only defines the policy type it renders.
- `docs/reference/manifest.md` gets **all three** §4.1 edits, not only `:142-145`.
- `Executor` and `FastCgiPolicy` are re-exported at the crate root
  (`blastit_manifest::Executor`) by `.1`, with a `lib.rs` doctest proving it —
  `MB.9.1.1`'s `RouteSpec`/`BuildCtx` name them by that path.
- `.2` also owns `crates/blastit/src/{probe,lib,supervise}.rs`: `pub mod probe;` +
  `pub mod supervise;` in `lib.rs`, and `Parts`/`Supervisor`/`new` narrowed to
  `pub(crate)` — which is why `blastit-manifest` stays `rustix`/`libc`-free.
- Neither leaf edits a `Cargo.toml` or lists `Cargo.lock`.
- Protected suites: `critical_static_manifest.rs` and `critical_static_cli.rs` are **not**
  in `paths` and stay green unmodified; `critical_fastcgi_manifest.rs` is a new file.

# Acceptance

- [ ] Both children `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ cargo test -p blastit-manifest --locked
$ cargo test -p blastit-manifest --test critical_fastcgi_manifest --test docs_fences --locked
$ cargo test -p blastit --locked
$ ! grep -qE 'rustix|libc' crates/blastit-manifest/Cargo.toml && echo "manifest stays syscall-free"
$ make docs-check
$ make check
```
- [ ] Every boot-fatal rule in §4.1 is a named test, and the page's PHP manifest parses,
  boots and keeps `[route.static]` working beside it.

# Notes

- Additive only: every existing `version = 1` manifest still parses unchanged.
- `docs/reference/manifest.md` also carries `MB.6`'s static wording: add the
  `[route.fastcgi]` entry and leave every static sentence exactly as it is.
- Existence and ancestor safety are two explicit calls (`check_fastcgi_entries`,
  `check_fastcgi_sockets`), never `validate`, so `docs_fences.rs` needs no socket.
