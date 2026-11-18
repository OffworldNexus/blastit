---
id: MB.3.5
title: "Parse and validate [route.php] (entry + workers) in blastit-manifest"
kind: epic
status: superseded
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
  - "crates/blastit-manifest/tests/critical_php_manifest.rs"
  - crates/blastit/src/probe.rs
  - crates/blastit/src/lib.rs
  - crates/blastit/src/supervise.rs
  - docs/reference/manifest.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-18T20:36"
---

# Goal

Teach `blastit-manifest` the `[route.php]` grammar exactly as `ADR-0010` §2 pins it (`entry`
required, optional downward `workers`), add the pure `EntryProbe` seam and its two
implementations, and document the block — split into sequential leaves that each stay
inside the file/line budget.

| Plan id | Design leaf | Lands |
|---|---|---|
| `MB.3.5.1` | `MB.3.5` part 1 (`done`; its external-PHP policy type and accessor went with `MB.3.17`) | `Executor` (`ALL_KINDS`, `kind()`), crate-root re-exports + `lib.rs` doctest, `route_lines()`, both predicates, the error variants, `docs_fences.rs` `7 → 8` |
| `MB.3.5.2` | `MB.3.5` part 2 | `blastit-manifest/src/probe.rs` (`EntryProbe`, `EntryProbeError`, `ProbeUid`, `check_php_entries`), `crates/blastit/src/{probe,lib,supervise}.rs`, protected `critical_php_manifest.rs`, the `docs/reference/manifest.md` entry |
| `MB.3.5.4` | landed out of order (`done`) | `[route.php]` / `PhpPolicy` / `Executor::Php`; the external-PHP key's narrowing was removed in `MB.3.17` |

Plan ids are numeric because `tools/plan.py` only accepts numeric segments.

# Context

- Design of record: `ADR-0010` §2 (the `[route.php]` grammar: `entry` required, an optional
  downward `workers` cap, `listen`/`socket`/`port`/`env` rejected as unknown route keys), the
  boot-fatal list and the dead-declaration rule restated in this brief, and `ADR-0008`'s
  surviving `Upstream` contract. The external-PHP grammar this epic first described is
  history (removed in `MB.3.17`; the historical briefs keep the record).
- **No pool config lives here.** There is no `render_pool_conf` and no external PHP process
  to configure: `[route.php]` runs embedded `libphp` in a view worker (ADR-0010 §1), and the
  pool machinery the epic once described was removed in `MB.3.17`.
- `docs/reference/manifest.md` gets **all three** §4.1 edits, not only `:142-145`.
- `Executor` and `PhpPolicy` are re-exported at the crate root
  (`blastit_manifest::Executor`) by `.1`, with a `lib.rs` doctest proving it —
  `MB.9.1.1`'s `RouteSpec`/`BuildCtx` name them by that path.
- `.2` also owns `crates/blastit/src/{probe,lib,supervise}.rs`: `pub mod probe;` +
  `pub mod supervise;` in `lib.rs` — which is why `blastit-manifest` stays
  `rustix`/`libc`-free.
- Neither leaf edits a `Cargo.toml` or lists `Cargo.lock`.
- Protected suites: `critical_static_manifest.rs` and `critical_static_cli.rs` are **not**
  in `paths` and stay green unmodified; `critical_php_manifest.rs` gains this epic's cases.

# Acceptance

- [ ] Both children `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ cargo test -p blastit-manifest --locked
$ cargo test -p blastit-manifest --test critical_php_manifest --test docs_fences --locked
$ cargo test -p blastit --locked
$ ! grep -qE 'rustix|libc' crates/blastit-manifest/Cargo.toml && echo "manifest stays syscall-free"
$ make docs-check
$ make check
```
- [ ] Every boot-fatal rule in `ADR-0010` §2 is a named test, and the page's PHP manifest parses,
  boots and keeps `[route.static]` working beside it.

# Notes

- Additive only: every existing `version = 1` manifest still parses unchanged.
- `docs/reference/manifest.md` also carries `MB.6`'s static wording: add the
  `[route.php]` entry and leave every static sentence exactly as it is.
- Entry existence, root containment and writability are explicit calls
  (`check_php_entries` plus the host-`libphp` check), never `validate`, so `docs_fences.rs`
  needs no host library.
