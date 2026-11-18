---
id: MB.26.41
title: "Compile and run critical_parallel_views inside the php-sapi image, so CI proves the measured PHP promise"
kind: task
status: proposed
owner: implementer
priority: 2
depends_on: []
paths: ["e2e/**", Makefile, ".github/workflows/php-sapi.yml"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

CI never runs the measured PHP concurrency promise. `MB.24.4` added the protected
`crates/blastit/tests/critical_parallel_views.rs` to `PHP_SUITE_ARGS`, but
`e2e/php-sapi/Dockerfile` and `e2e/php-sapi/Dockerfile.two-package` hardcode the two older suites,
and `make rs-test-php-image` refuses unless the image records exactly 2 suite executables. On a
release every one of those legs therefore proves `critical_php_backend` and `critical_php_embed`
and silently skips the wall-clock suite.

# Context

- Files: `e2e/php-sapi/Dockerfile`, `e2e/php-sapi/Dockerfile.two-package`,
  `e2e/php_sapi_probe.py` (writes `/src/target/php-suite-executables`), the `rs-test-php-image`
  target in `Makefile` (the `-ne 2` check and the `grep -m1 critical_php_embed` leg, which must
  keep working).
- Read `plan/nodes/MB/MB.24/MB.24.4/RESULT.md` (finding 5) and `.github/workflows/php-sapi.yml`.
- Needs Docker and network (the image's own `apt install libphp8.4-embed`), so it is an `e2e/` node,
  not a `make check` one.

# Acceptance

```console
$ make php-sapi-e2e PHP_MINOR=8.4 PHP_SAPI_KEEP=1 PHP_SAPI_IMAGE=blastit-php-sapi-e2e:local
$ make rs-test-php-image
```

- the image records **3** suite executables and `make rs-test-php-image` runs all three, the
  unprivileged `PR_SET_DUMPABLE(0)` leg still selecting `critical_php_embed`, and the count check
  refusing a 2-executable image with a message naming the three suites;
- both Dockerfiles build the third suite, and the leg is green on the image's real pair;
- `make check` unchanged (no host PHP needed).

# Notes

- Never relax the count check to "at least 2": the point is that a suite silently missing from the
  image is exactly what this node exists to prevent.

# Provenance

Re-filed from `MB.27` at the `v0.2.0` gate: compile and run `critical_parallel_views` inside `e2e/php-sapi/Dockerfile` so GitHub Actions `php-sapi.yml` also witnesses the wall-clock PHP pool proof (`make rs-test-php` already runs it on PHP-equipped hosts).
