---
id: MA.2.3.3
title: "Leaf 10: unprivileged dev supervisor, execve barrier and dev --learn"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MA.2.3.2]
paths:
  - crates/blastit/src/lib.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/child.rs
  - crates/blastit/src/zygote.rs
  - crates/blastit/src/supervise.rs
  - crates/blastit/src/dev_learn.rs
  - "crates/blastit/tests/**"
  - "crates/blastit-dbproxy/src/**"
  - "crates/blastit-dbproxy/tests/**"
attempts: 5
max_attempts: 5
escalation: expert
created: "2026-11-12T13:12"
updated: "2026-11-12T18:30"
---

# Goal

Implement `ExecutionMode::Dev`: a genuinely unprivileged `blastit dev` that still runs the real five roles under the caller's `geteuid()`, with an ephemeral `0700` runtime root, per-route socket directories, stage-0 `execve` secret stripping, real per-role `seccomp` filters, the 3-line banner and `BLASTIT_DEV_EXPOSE` guard, plus `dev --learn` collection and atomic manifest writes — and prove production still fails closed.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §1.2, §4.2 B/E.2, §5 (`S3`, `S6`, `S10`), §7.2 row `R17, R19..R22, R29 (dev)`, §8 Leaf 10; `ADR-0001`, `ADR-0003`; requirements `R17`, `R19`, `R20`, `R21`, `R22`, `R29`.
- Dev children: `umask 077`, `RLIMIT_NOFILE`, `RLIMIT_CORE=0`, `PR_SET_PDEATHSIG(SIGKILL)`, `PR_SET_NO_NEW_PRIVS(1)`, real `seccomp` per role (honouring `egress`), **no** `setresuid`/`setgroups`, **no** per-UID `RLIMIT_NPROC` lowering.
- `dev` strips every `[proxy.*]` secret input, every `BLASTIT_*`, and every `BLASTIT_ENV_STRIP` name across the stage-0 `execve`; per-route dirs `<root>/views/<idx>/` get `.s.PGSQL.<port>` (`0600`, `SO_PASSCRED`) only for granted proxies (all declared proxies under `--learn`).
- `dev --learn` collects `obs_fd` records and writes routes/queries through `blastit_manifest::edit::write_atomic`; `413` and declared codecs stay enforced. `BLASTIT_DEV_EXPOSE=1` is required for non-loopback `--host` and logs a loud `WARN` naming `<host>:<port>` and `euid`.
- Load skills `rust-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ cargo build --locked -p blastit-dbproxy
$ cargo test -p blastit --test critical_dev_and_fail_closed --locked
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
```

- `crates/blastit/tests/critical_dev_and_fail_closed.rs` is **new** (with `crates/blastit/tests/common/mock_pg.rs`): `blastit dev` boots all five roles as non-root with `"privilege": "unprivileged-dev"`, prints the exact 3-line banner (`1 route` and `2 routes`, plus the `EXPOSED` variant), strips a real `DATABASE_URL`/`BLASTIT_*`/`BLASTIT_ENV_STRIP` name, and `dev --learn` writes routes/queries on `SIGTERM`.
- Negative twins: unprivileged production `blastit` exits `1` with zero single-UID fallback; an ungranted route's socket path is `ENOENT`; an `egress = false` view gets `EPERM|ENOSYS` on `socket(AF_INET)`; `--host 0.0.0.0` without `BLASTIT_DEV_EXPOSE=1` exits `1` with empty stdout.
- Seccomp assertions preflight the filter and accept `EPERM|ENOSYS` with an actionable kernel message; never skip.

# Notes

- ~450 changed lines — the largest leaf; split only with `planner` approval. No `docs/**`, no `python/**`, no root `Cargo.*`.
- `MockPgServer` binds `127.0.0.1:0` and reads `local_addr().port()`; no root, no Docker, no wheel.
