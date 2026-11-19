---
id: MB.41.3
title: "Support indented content-tab fences in docs_fences & runnable_docs, fix rs-test-php-image Docker seccomp, and pass full make check + php-sapi Docker gate"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.41.2]
paths: [Makefile, "crates/blastit-manifest/tests/**", "python/tests/**", "docs/**", README.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T12:44"
updated: "2026-11-19T12:44"
---

# Goal
Update the documentation test harnesses (`crates/blastit-manifest/tests/docs_fences.rs` and `python/tests/runnable_docs/`) to de-indent and validate all code/`Blastfile`/`console` fences inside `=== "..."` content tabs, fix Docker `rs-test-php-image` outer seccomp for `fanotify`, verify rendered browser tabs via `visual-checker`, and pass full `make check` + `make rs-test-php-image`!

# Exact Targets
1. **Indented Tabbed-Fence Support (`crates/blastit-manifest/tests/docs_fences.rs`, `python/tests/runnable_docs/{harness.py,inventory.toml,fixtures/**}`):**
   - In `docs_fences.rs`: update the Markdown fence scanner so indented fences inside `=== "Tab"` blocks (4-space or 8-space indented ```` ```text title="Blastfile" ```` and ```` ```text ````) are recognized, de-indented cleanly, and parsed/validated by `Manifest::parse_blastfile`! Update `PAGES` and `STATEMENT_LISTS` counts to match the updated pages (and add a unit test proving a broken `Blastfile` inside an `=== "PHP"` tab fails `docs_fences`!).
   - In `python/tests/runnable_docs/harness.py` + `inventory.toml`: ensure indented `console` fences inside `=== "..."` tabs are extracted & de-indented cleanly, and update `inventory.toml` (adding a minimal PHP/static fixture or accounted reason for `libphp`/standalone fences where appropriate) so `make docs-unit` and `make docs-test` execute and pass 100%!
2. **Docker `php-sapi` Outer Seccomp (`Makefile:531`):**
   - Docker's default outer seccomp profile (`moby/profiles/seccomp/default.json`) returns `EPERM` on `fanotify_init` (`__NR_fanotify_init = 300`) inside `make rs-test-php-image`. Add `--security-opt seccomp=unconfined` on `Makefile:531`'s `docker run` (Blastit's own inner `seccomp(SECCOMP_SET_MODE_FILTER)` still enforces view/probe sandboxing inside the container!) and verify `make php-sapi-e2e PHP_MINOR=8.4 PHP_SAPI_KEEP=1 && make rs-test-php-image` passes all 29 `critical_php_embed` tests (including `a_dev_boot_reloads_a_php_save_in_place_by_rename_and_through_an_include` AND `a_probe_that_hangs_is_killed` inner seccomp check!) inside Docker!

# Acceptance
- `make check`
- `make php-sapi-e2e PHP_MINOR=8.4 PHP_SAPI_KEEP=1 && make rs-test-php-image`
