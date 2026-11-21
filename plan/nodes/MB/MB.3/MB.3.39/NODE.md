---
id: MB.3.39
title: "CI witness for the real-pair PHP legs: run critical_php_embed + critical_php_backend in php-sapi.yml"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: [".github/workflows/php-sapi.yml", Makefile]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T01:42"
updated: "2026-11-15T02:24"
---

# Goal

Give the real-pair half of `crates/blastit/tests/critical_php_embed.rs` a CI witness.

Every CI run of `make check` is on a host **without** PHP, so `critical_php_embed`
exercises its refusal branch only. Three of the epic's four acceptance bullets are
proved on a developer host alone:

- the `[route.php]` page answering `200` through `view.chan` with `cpu_ns` on the
  `request` event (`critical_php_backend.rs`, `critical_php_embed.rs`);
- `exit()`/`die()`/fatal containment and the slot respawn;
- the maps leg's *unprivileged* half (`EACCES` on the roles above the view).

`.github/workflows/php-sapi.yml`'s `shim` job is the only CI job whose image has
`php8.4-dev` + `libphp8.4-embed`, but it runs `make php-sapi-e2e` (the shim-level
probe in Docker) and never these two suites. `make php-sapi-e2e` cannot substitute:
it never boots `blastit`, so it has no view fork, no `view.chan`, no scheduler
`cpu_ns`, no respawn.

Deliver:

1. In `php-sapi.yml`'s `shim` job, after the shim build, build the workspace
   binary and run both protected suites against the real pair:
   `cargo test -p blastit --locked --test critical_php_backend` and
   `--test critical_php_embed`, in an environment that names the pair the way the
   suites' `host_php()` probe expects (`BLASTIT_LIBPHP=/usr/lib/x86_64-linux-gnu/libphp8.4.so`
   if that is where the image puts it, `BLASTIT_PHP_SHIM_DIR=<repo>/target/php-sapi`).
   The suites must **serve**, not refuse: add a guard step that fails the job when
   their output says `refused`, so a silently-PHP-free runner cannot report green.
2. Keep `make check`'s PHP-free path untouched: the second (root) run in `ci.yml`
   must stay green without PHP. Do not add PHP to `ci.yml`.
3. Record the wall time in the job's summary (`timeout-minutes` stays 30).

# Context

- `docs/testing/AUDIT-MB.3.13.md` gap G7; `MB.3.13.4`'s module doc already says
  exactly what is missing, and its follow-up names this file.
- `ADR-0010` §4.2 makes native per-platform execution the CVE/ABI bar, and
  `MB.3.18` owns the full matrix; this node is the one job, one runner, today.
- The suites are 1.9 s serialized and 0.6 s parallel; the added cost is the
  workspace build the job already pays for `php-sapi-e2e`.

# Acceptance

```console
$ gh run view <run> --log | grep -E 'test (a_|no_libphp_is_mapped)'
test a_php_route_is_served_or_the_boot_refuses_naming_the_package ... ok
test no_libphp_is_mapped_above_the_view_worker ... ok
test an_exit_is_contained_and_the_next_request_is_served ... ok
test a_fatal_is_contained_and_the_next_request_is_served ... ok
test a_shim_for_another_minor_refuses_the_boot_before_the_listener ... ok
test result: ok. 5 passed; 0 failed; 0 ignored   (critical_php_backend)
test result: ok. 8 passed; 0 failed; 0 ignored   (critical_php_embed)
```

- The log carries the served evidence lines (`served: HTTP/1.1 200 OK  cpu_ns=…`,
  `retired pid … respawned pid …`), i.e. the real-pair branch ran.
- A deliberately broken pair in that job (e.g. `BLASTIT_LIBPHP=/dev/null` for the
  guard step) fails the job instead of printing `refused` and passing.
- `make check` on the local PHP-free host is unchanged and green.
