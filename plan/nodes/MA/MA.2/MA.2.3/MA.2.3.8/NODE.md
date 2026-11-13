---
id: MA.2.3.8
title: Make a clean SIGTERM to blastit dev exit 0 (late ctl.chan EPIPE is not fatal)
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [crates/blastit/src/supervise.rs, crates/blastit/tests/critical_dev_and_fail_closed.rs]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T20:50"
updated: "2026-11-12T21:52"
---

# Goal

Make a clean `SIGTERM` to `blastit dev` always exit `0`, including `--learn`, and make the two
`--learn` protected tests deterministic.

# Context

`docs/testing/AUDIT-MA.2.3.md` finding F4. 33 runs of
`cargo test -p blastit --test critical_dev_and_fail_closed`: **3 failures** in two tests
(`learn_writes_routes_and_queries_on_sigterm:1100`,
`first_declared_route_keeps_all_queries_when_unrouted_requests_exist:1299`), both on
`assert_eq!(status.code(), Some(0))`:

```text
INFO  shutdown    drain
INFO  process.exited a  pid=… uid=1000  status=0
ERROR app.output  stderr  ctl.chan: Broken pipe (os error 32)
```

`Supervisor::send_ctl` (`crates/blastit/src/supervise.rs:759-783`) returns quietly when the ctl fd
is gone, but maps `EPIPE` to `self.fail("ctl.chan: {err}")` → `_exit(1)`. During `drain` the
scheduler is legitimately gone, so a late `KILL_VIEW`/`EXITED` ack turns a clean shutdown into exit
`1`, violating §1.1 (`blastit dev` clean `SIGINT`/`SIGTERM` is `0`) and flaking the protected
suite at the epic gate.

# Acceptance

```console
$ for i in $(seq 30); do cargo test -p blastit --test critical_dev_and_fail_closed -- --test-threads=4; done
# 30/30 green
$ cargo test -p blastit --test critical_dev_and_fail_closed
$ make test-critical
```

- A late `ctl.chan` write during/after `drain` is treated like the closed-fd arm (best effort, not
  fatal). `EPIPE`/`ECONNRESET` must not change the exit code once shutdown has begun; a genuine ctl
  failure *before* shutdown still fails closed.
- A deterministic regression covers it: a supervisor-level test with a closed ctl fd (or a dev run
  that kills the scheduler first), asserting exit `0`. Prefer the unit-level test over more sleep.
- Do not weaken `SEND_CTL_TIMEOUT` or the "init must not pretend a dead component is alive" rule for
  the running phase.

# Notes

- `crates/blastit/tests/critical_dev_and_fail_closed.rs` is protected: adding tests is allowed,
  modifying existing ones needs this node's `paths` (they are listed) and a `# Goal` note.
- Re-run the suite at least 30 times before reporting done: the flake is ~1 in 10.
