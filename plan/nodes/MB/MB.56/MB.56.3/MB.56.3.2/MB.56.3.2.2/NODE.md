---
id: MB.56.3.2.2
title: "e2e_dev.rs part 2: embedded PHP front controller, per-view grants, php2shell, pinning"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.3.2.1]
paths: ["crates/blastit/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T18:54"
updated: "2026-11-20T18:54"
---

# Goal
Extend `crates/blastit/tests/e2e_dev.rs` with the **PHP** half of `TESTING_PLAN.md` §2 and delete the suites it absorbs.

1. Embedded PHP multi-view front controller (PSR/Laravel-shaped: `public/index.php` + a static mount under it) with **distinct per-view PostgreSQL table/column rights**: a public read-only route, a write route, and proof that a compromised public view is denied `SELECT` on another table's columns and denied `INSERT`.
2. PHP fault isolation: one request calling `exit()` and one hitting a fatal both recover without dropping the worker pool; the next request is served.
3. `php2shell` / CGI-argument-injection immunity: query-string and header vectors never become `-d` flags or `HTTP_PROXY` (behavioural, over the real SAPI).
4. Entry pinning: a rewritten-in-place entry script is refused with the documented sentence until a restart.
5. Absorb and delete: `critical_php_embed.rs`, `critical_php_backend.rs`, and the PHP half of `critical_parallel_views.rs`. Reach the cases `MB.56.2.1` inventoried (grant isolation, `exit()`/`DONE` outcomes, pinning, sink include reporting).

# Acceptance
- `make fast-check` green, zero warnings; `cargo test -p blastit --test e2e_dev` green; every absorbed file deleted; whole file still ≲ 1,500 LOC.
