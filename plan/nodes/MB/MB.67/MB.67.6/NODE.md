---
id: MB.67.6
title: "Test fixtures: honour TMPDIR and keep socket paths under SUN_LEN"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.67.1]
paths: [crates/blastit/tests/common/project.rs, "crates/blastit/tests/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T00:51"
updated: "2026-11-23T00:57"
---

# Goal

`crates/blastit/tests/common/project.rs::scratch_root()` hardcodes `/tmp/opencode`, so a
fixture's deepest socket path grows past `SUN_LEN` (108) whenever the pid is long or the test
name is: `e2e_check_fmt::learn_writes_one_grant_without_widening_a_view` fails on this host with
`/tmp/opencode/blastit-e2e-dev-3166132-learn-write/tmp/blastit-check-1000-3166139-376086268-0/pg/.s.PGSQL.5432:
path must be shorter than SUN_LEN` (109 bytes). Pre-existing, unrelated to `MB.67`'s language
table, but it reds `make fast-check` for every slice in this epic.

# Scope

- `crates/blastit/tests/common/project.rs` — `scratch_root()` must honour the process's temp
  directory (`std::env::temp_dir()`, so `TMPDIR` works as it already does for `cargo mutants`)
  and use a **short** per-run directory (`/<tmp>/blastit-e2e-<pid>` + the per-project tag), not
  a fixed multi-segment `/tmp/opencode/...`. Keep uniqueness (pid + tag), keep the `Drop`
  cleanup, keep `Project::path/write` unchanged.
- No product code, no other test semantics: the reported failure is the fixture's own socket
  path, not a product refusal.

# Out of scope

`crates/blastit/src/**`, the runtime directory the product creates, any test that asserts a
specific refusal message.

# Acceptance

1. `cargo test -p blastit --test e2e_check_fmt` green (the 24 tests in that binary).
2. `cargo test -p blastit --test e2e_check_fmt learn_writes_one_grant_without_widening_a_view`
   green, and the fixture root it prints/uses is short enough that the deepest socket path
   stays under 108 (`grep` the path in the failure-evidence before/after or add a one-line
   `assert!` in the test helper that the root is <= 48 bytes).
3. `make fast-check` green (this is the only Cargo writer; no concurrent build).
