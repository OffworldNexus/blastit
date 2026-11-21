---
id: MB.56.3.4
title: "Create e2e_prod_refusals.rs (§6 fail-closed startup, env/WSGI refusals)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.56.3.3]
paths:
  - "crates/blastit/tests/**"
  - "crates/blastit/src/**"
  - Makefile
  - .github/workflows/ci.yml
  - docs/reference/blastfile.md
  - docs/reference/cli.md
  - docs/troubleshooting.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T18:45"
updated: "2026-11-20T21:07"
---

# Goal
Create `crates/blastit/tests/e2e_prod_refusals.rs` (`TESTING_PLAN.md` §6 — fail-closed startup & runtime integrity) and delete the suites it absorbs. Target ≲ 1,000 LOC.

1. Production `blastit` refuses to start before serving when: the caller lacks OS isolation privileges; the binary or a parent directory is group/world-writable; the `Blastfile` is a symlink or FIFO; entry/shim descriptors are tampered; the preset is unknown; a `body raw` route is not marked `unvalidated`.
2. Env/config boot refusals, naming the variable and never its value: missing `$IDENT`, `DATABASE_URL=postgres://h/db?target_session_attrs=any`, a multi-host URL, an output naming `BLASTIT_*`, two proxies producing one variable (all one line each, in one boot); `BLASTIT_*` stripped from the worker environment; an injected default never overriding a present value.
3. WSGI host refusals, verbatim: `blastit dev: CPython 3.12 at <path> is not supported for WSGI views (CPython 3.13 and 3.14 …)`, the free-threaded refusal, and the missing-`libpython` sentence naming the minor and every remedy.
4. Absorb and delete: `critical_dev_and_fail_closed.rs`, `no_python_build.rs`, and any remaining §6 case in `smoke.rs`.

# Acceptance
- `make fast-check` green, zero warnings; `cargo test -p blastit --test e2e_prod_refusals` green; every absorbed file deleted.
