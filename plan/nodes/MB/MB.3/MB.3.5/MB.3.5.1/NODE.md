---
id: MB.3.5.1
title: "Parse [route.fastcgi], Executor and the boot predicates in blastit-manifest - design MB.3.5 part 1"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.4]
paths:
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/tests/docs_fences.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T10:51"
---

# Goal

Teach `blastit-manifest` the `[route.fastcgi]` grammar and the executor model the
contract crate consumes: `FastCgiPolicy`, `Executor`, `Route::fastcgi()`,
`Manifest::route_lines()`, the two canonical boot predicates, the new error variants,
the crate-root re-exports and the `docs_fences.rs` bump (design leaf `MB.3.5`, part 1).

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.1 (grammar, defaults, boot-fatal
  list, dead-declaration rule), §4.2 "one predicate, one spelling", §4.2.1
  (`Executor`/`FastCgiPolicy` derives), §8's `MB.3.5.1` row and §8.2's `MB.3.5` row.
- Files (the whole leaf, 6): `crates/blastit-manifest/src/{model,parse,validate,error,lib}.rs`
  and `crates/blastit-manifest/tests/docs_fences.rs`.
- What lands:
  - `FastCgiPolicy` `#[derive(Debug, Clone, PartialEq, Eq)]` — `entry`, relative
    `listen`, `workers` (default `4`).
  - `Executor` `#[derive(Debug, Clone, PartialEq, Eq)]` — **not** `Copy` (the `Pool` arm
    owns a `FastCgiPolicy` with `String`s) — `kind(&self) -> &'static str`
    (`"wsgi"`/`"fastcgi"`), `pub const ALL_KINDS: &'static [&'static str] = &["wsgi", "fastcgi"]`.
    **Both types are appended to `lib.rs`'s `pub use model::{…}` list** so
    `blastit_manifest::Executor` resolves at the crate root; a `lib.rs` doctest proves it.
  - `Route::fastcgi()`, `Manifest::can_boot_without_app()`,
    `Manifest::is_all_static()` (0.2.0's meaning **plus** `&& r.fastcgi().is_none()`),
    `Manifest::route_lines()` (boot-only `[[route]]` line scan, routing order).
  - Error variants `FastCgiEntryInvalid`, `FastCgiListenInvalid`,
    `FastCgiListenParentInvalid`, `FastCgiWorkersInvalid`, `FastCgiDeadDeclaration`,
    each message naming `file:line`.
  - Dead declaration keeps **both** arms: `static_policy.is_some() && fastcgi.is_none()
    && (spa || app.is_none())` keeps `StaticDeadDeclaration` and its exact 0.2.0 `why`
    strings; `spa = true` with `[route.fastcgi]` is `FastCgiDeadDeclaration`.
  - `docs_fences.rs`, three edits only: `const FILES: [&str; 7]` → `8` with
    `docs/frameworks/php.md`, `assert_eq!(total, 7)` → `8`, module-doc "seven" → "eight".
- Existence/ancestor probing is **not** here: `SocketProbe`, `check_fastcgi_entries` and
  `check_fastcgi_sockets` are `MB.3.5.2`'s, so `docs_fences.rs` still needs no socket.
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no workspace member.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked
$ cargo test -p blastit-manifest --test docs_fences --locked
$ grep -q 'Executor' crates/blastit-manifest/src/lib.rs && echo "Executor is at the crate root"
$ grep -q 'ALL_KINDS' crates/blastit-manifest/src/model.rs && echo "one source of executor kinds"
$ ! grep -qE 'rustix|libc' crates/blastit-manifest/Cargo.toml && echo "manifest stays syscall-free"
$ make check
```

The `lib.rs` doctest
`use blastit_manifest::{Executor, FastCgiPolicy}; assert_eq!(Executor::View.kind(), "wsgi"); assert!(Executor::ALL_KINDS.contains(&"fastcgi"));`
runs inside the first command.

# Notes

- `critical_static_manifest.rs` and `critical_static_cli.rs` are **not** in `paths`; the
  two dead-declaration arms exist to keep them green with zero edits.
- `docs/reference/manifest.md` and the probe trait are `MB.3.5.2`'s — do not touch them.
- `RouteInfo`/`SchedulerConfig` consumption of `Executor` is `MB.9.2.1.1`'s; this leaf
  only defines the type and its kind table.
