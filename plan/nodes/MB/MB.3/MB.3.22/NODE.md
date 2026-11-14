---
id: MB.3.22
title: "Close MB.3.5.4 review MINORs: PHP-only BLASTIT_APP refusal, check's pytest branch, Route::executor, dead key field"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.5.4]
paths:
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/check.rs
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/tests/critical_php_manifest.rs"
  - plan/nodes/MB/MB.3/MB.3.5/MB.3.5.4/NODE.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T16:39"
updated: "2026-11-14T16:39"
---

# Goal

Close the five MINORs from `MB.3.5.4`'s review. Two are behavioural; three are dead-surface or
stale text.

# Context

1. **`stage0::static_app_refusal` keys on `is_all_static()`** (`crates/blastit/src/stage0.rs:295`),
   so a **PHP-only** manifest with `BLASTIT_APP` set boots with the variable **silently
   ignored** — R13 says such a setting is refused, not dropped. Fix: predicate
   `can_boot_without_app() && config.app.is_some()`; keep the exact existing "every route is
   static" sentence for the all-static case, add a php sentence, and assert the refusal
   (denied + accepted twin).
2. **`check` still branches on `is_all_static()`** (`crates/blastit/src/check.rs:161`), so PHP
   routes take the WSGI/pytest path. `model.rs:727`'s doc sentence promising otherwise is
   therefore wrong. Either the branch follows `can_boot_without_app()`/executor kinds, or the
   doc is corrected and the behaviour is filed to `MB.3.5.2`/`MB.3.5.3` — decide with a test,
   not prose.
3. `PhpDeadDeclaration::key` (`error.rs:521-535`) is never rendered nor read outside tests:
   drop the field and derive the text from `why`.
4. `Route::executor()` does not exist, so the acceptance's "report `Executor::Php`" cannot be
   asserted from a parsed manifest (`critical_php_manifest.rs:90-116` hand-builds it and would
   pass if no route ever became a php one). Add `Route::executor()` (`Php(policy)` when the
   block is set, else `View`) and assert it there.
5. `plan/nodes/MB/MB.3/MB.3.5/MB.3.5.4/NODE.md:61` still carries a stale acceptance clause
   ("`[route.fastcgi]` … with `listen` alone it parses") that contradicts the shipped
   unknown-key rule. Delete it — plan text, not code.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked
$ cargo test -p blastit --locked
$ make check
```
- Each item is a named test (denied + accepted) or a deletion with no dangling reference
  (`grep -rn 'PhpDeadDeclaration' crates/ | grep key` empty).
- `Route::executor()` is asserted on a *parsed* manifest, so a manifest that never becomes a
  php route fails the test.
- No protected `critical_*.rs` edited outside this node's paths.

# Notes

- Do not expand into `MB.3.20`/`MB.3.10` doc work; item 5 is this node's file only.
