---
id: MB.26.33
title: "Subreaper + WNOWAIT, BLASTIT_ALLOW_EGRESS=1 as a security switch, R4 budget and the 4x3 residue matrix"
kind: task
status: proposed
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit/src/**", "crates/blastit/tests/**", docs/reference/settings.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

Harden the supervisor's process accounting: a subreaper so no orphan escapes, `waitid` with
`WNOWAIT` so a status is inspected before reaping, `BLASTIT_ALLOW_EGRESS=1` as an explicit
opt-in, the `R4` budget, and the 4×3 residue matrix.

# Context

- `docs/design/backend-runtimes.md` §2 ("what a crash leaves behind") + `ADR-0009`.
- `PR_SET_CHILD_SUBREAPER` makes init the reaper for anything a worker spawns, so a
  grandchild cannot outlive the route; `WNOWAIT` lets the supervisor read a status (uid,
  signalled-ness, cgroup membership) **before** the pid is recycled.
- `BLASTIT_ALLOW_EGRESS=1` is a security switch of the same class as the retired external-PHP
  allow flag (`MB.3.17`) — it must be documented as one, refused under a preset, and named
  in the log.
- `R4` is the per-request budget row in the design; the 4×3 residue matrix is the four
  crash shapes × three roles, each with the residue it is allowed to leave (none, a pid to
  reap, a socket to unlink).

# Acceptance

```console
$ cargo test -p blastit --locked
$ make check
```
- A worker that forks a grandchild and exits leaves **no** process and no zombie (asserted
  via `/proc`), and the supervisor sees the exit status before the pid is reaped.
- `BLASTIT_ALLOW_EGRESS=1` is refused under every preset and documented in
  `docs/reference/settings.md` as a security switch with what it costs.
- The residue matrix is a test table: 12 rows, each asserting the allowed residue and that
  the slot is released exactly once.

# Provenance

Re-filed from `MB.3.26` at the `v0.2.0` gate: `PR_SET_CHILD_SUBREAPER` + `WNOWAIT`, `BLASTIT_ALLOW_EGRESS=1` switch, and the 4x3 crash-residue table are post-`0.2.0` supervisor hardening (`0.2.0` claims neither `BLASTIT_ALLOW_EGRESS` nor subreaper semantics, and `MB.15` proves `dev` tree reaping).
