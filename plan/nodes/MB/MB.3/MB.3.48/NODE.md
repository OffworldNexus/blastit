---
id: MB.3.48
title: "The two PHP suites are flaky: a boot refusal read before it is written, and a 1-in-7 bailout outcome"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [crates/blastit/tests/critical_php_embed.rs, crates/blastit/tests/critical_php_backend.rs]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T04:46"
updated: "2026-11-15T05:26"
---

# Goal

Make the two PHP suites deterministic. They are the required `php-sapi` CI gate's
payload, and they flake: a red there blocks the release at random, and this session
already produced one (`make check` red on `a_blastit_libphp_that_is_not_a_library_refuses_the_boot`,
green on retry) plus a 1-in-7 on a real-pair leg.

# Context

Observed on this branch (release/0.2.0), both on a host with the real pair:

1. `crates/blastit/tests/critical_php_backend.rs:477`,
   `a_blastit_libphp_that_is_not_a_library_refuses_the_boot`:
   `not a library: the route is named: ` with **nothing after the colon** — the boot's
   refusal text was not there when the assertion read it. `make check` exit 2, then 0 on
   the same tree seconds later (`/tmp/opencode/mc8.log` vs `mc9.log`). `MB.3.40`'s
   follow-up 4 names the suspect: `Boot::launch`'s free-port probe plus an `events()`
   snapshot taken before `ready`.
2. `crates/blastit/tests/critical_php_embed.rs:2294`,
   `a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time`: 1 run in 7
   returned `left: "view_died" right: "bailed"` with `cost_ns: 0, cpu_ns: null,
   queued_ms: 26.35, status: 500` — a view that reported nothing under load (the same
   shape `MB.3.41`'s `deferred` path and `MB.3.45` items 4-5 care about, so a
   misdiagnosis here would be expensive).

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_backend
$ cargo test -p blastit --locked --test critical_php_embed
$ make check
$ uv run --no-project tools/critical_guard.py --base 5710182 --node MB.3.48
```
- (1) A deterministic reason and fix, not a retry. Show the failure first — run the suite
  in a loop (e.g. 100x, or under `--test-threads` pressure) until it reproduces and paste
  it — then fix the reader (wait for the child to exit and drain both streams, or a real
  readiness/synchronisation handshake) and show the same loop green with **zero** retries
  in the harness and no `sleep`-tuned constants.
- (2) Diagnose before fixing: say whether it is the same harness race, the view's DONE
  genuinely lost, or the charge path. If it is a lost DONE, it is `MB.3.45`'s seam — say
  so and stop rather than papering over it in the test. If it is the fixture, fix the
  fixture and loop it 50x.
- (3) TWO DOC CLAIMS, `//!`-only: `critical_php_embed.rs:70` says "(plus the sibling
  suite's served leg)" and undercounts — `critical_php_backend` has two pair-gated legs,
  `a_php_route_is_served_or_the_boot_refuses_naming_the_package` (:503) and
  `a_symlinked_blastit_libphp_is_resolved_before_it_is_served` (:675); name both.
  `critical_php_backend.rs:34-38` still says "The real load — `make php-sapi-e2e` … — is
  CI's proof; this suite proves the composition root wires the worker on a host that
  builds it", which places the suite outside CI — false since `MB.3.39`'s
  `make rs-test-php-image`. Correct it in one clause.
- No `#[ignore]`, no skip, no weakened assertion, no `sleep` added; `make check` green.

# Notes

- Both files are PROTECTED: this node's `paths` name them and this Goal is the reason
  `critical_guard.py` requires. Do not touch `crates/blastit-scheduler/**` or
  `crates/blastit/src/**` — `MB.3.45` owns the scheduler accounting, and a lost DONE is
  its fix, not this node's.
