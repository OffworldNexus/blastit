---
id: MB.56.5.1
title: "Coverage-driven dead-code prune + split the god-files"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["crates/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T22:57"
updated: "2026-11-20T23:02"
---

# Goal
Use `make coverage` (over the consolidated E2E and §7 suites) to delete dead and speculative Rust production code, applying the candidates the earlier slices recorded in this node's LOG.md.

1. Run `make coverage` per crate with the suites that own each surface (`e2e_cli`, `e2e_dev`, `e2e_check_fmt`, `e2e_prod_refusals`, `blastfile_*`, `sql_policy`/`sql_templates`, `http_framing`, `e2e_lsp`, plus `blastit-sys`/`blastit-frame` boundary suites).
2. Delete, in this order of confidence: symbols with **no** production caller anywhere (`StatementClass::is_plain`, `Analysis::is_plain`, `StaticRoot::open_with`'s dead twins, `mem_usage::advisory`, `unified` renderer leftovers, `py::refusal_message`, `forking::agrees`, `is_serveable_entry`, the `dev_reload` `#[cfg(any(test, debug_assertions))]` fakes whose last caller is gone, `blastit-sys/src/mem.rs` items used nowhere outside the crate), then partially-dead modules whose remaining branches no story reaches.
3. **Never** delete a symbol that only an E2E-spawned child exercises (the coverage harness cannot see its `.profraw`): name each keeper and its owning suite in `RESULT.md` so a later reader does not re-litigate it.
4. If an uncovered branch corresponds to a real `TESTING_PLAN.md` story, add the assertion to the owning E2E suite — never a unit test.
5. Split the god-files the log names (`blastit-events/src/lib.rs` 1813, `json.rs` 1402, `blastit-lsp/src/lib.rs` 1386, `blastit-php-sapi/src/loader.rs` 1075, `blastit-sys/src/fanotify.rs` 1031, `blastit/src/supervise.rs` 2767, `blastit-scheduler` worker/discovery) into cohesive units `< 500` prod LOC — behaviour identical, no API change.

# Acceptance
- `make fast-check` green, zero warnings; `make coverage` run and its keep/delete decision recorded per crate; no `#[allow(dead_code)]` added.
