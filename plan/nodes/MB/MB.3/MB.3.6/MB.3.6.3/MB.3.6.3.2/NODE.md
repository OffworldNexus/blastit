---
id: MB.3.6.3.2
title: "render_pool_conf, the S0-S7/U1 spawn sequence and the fake php-fpm helper - design MB.3.6b part 2"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.3.6.3.1]
paths:
  - crates/blastit/src/supervise.rs
  - crates/blastit/tests/support/fake_php_fpm.rs
  - crates/blastit/tests/fake_php_fpm_smoke.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T14:39"
---

HISTORICAL — written for the retired FastCGI/php-fpm design. ADR-0010 replaced it (embedded host NTS libphp in per-route view workers). Do not implement from this node.

# Goal

Implement the production pool: `render_pool_conf`, the `S0`–`S7` bind/freeze/verify
sequence with its `U1` unwind, the `forking::fork(true)` spawn as the route uid, the inline
`pool_test` harness and the fake `php-fpm` helper (design leaf `MB.3.6b`, part 2).

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.8.2 (steps 1–4, zero variants),
  §4.8.3 (the normative socket lifecycle, syscall invariants and unwind matrix), §4.8.1
  (the pool file and its `env[]`) and §8's `MB.3.6.3.2` row; §8.2's `MB.3.6.3` row.
- Files (the whole leaf, 3): `crates/blastit/src/supervise.rs`,
  `crates/blastit/tests/support/fake_php_fpm.rs`,
  `crates/blastit/tests/fake_php_fpm_smoke.rs`.
- What lands:
  - `render_pool_conf` + `PoolConf` (`Debug, Clone, Copy`): every directive,
    `pm.max_children == workers`, `pm.max_requests = max(BLASTIT_VIEW_MAX_REQUESTS, 500)`
    (never `0`), no `listen.owner`, `env[]` with no `BLASTIT_*` and no
    `HTTP_PROXY`/`http_proxy`, `HOME == TMPDIR == <runtime>/views/<uid>/tmp` (both inside
    `open_basedir`), `host=<runtime>/views/<uid>` rewrite, INI escaping, mode constants
    `0600`/`0555`.
  - `S0`–`S7`/`U1`: parent-side `setpgid`, `drop_privileges(route_uid)`, the
    `php-fpm -F -y <runtime>/views/<uid>/php-fpm.conf` exec as the route uid, `php-fpm -t`
    in a forked child that drops to the route uid first, `S1` reap-to-`ECHILD`, `S4`/`S6`
    pgid-membership, the **single** `*at` sequence (`unlinkat` → bind window
    `fchownat("", route_uid, route_gid, AT_EMPTY_PATH)` + `fchmodat(".", 0755, 0)` → spawn
    → `fstatat(sock, AT_SYMLINK_NOFOLLOW)` (`S_ISSOCK`, `route_uid:route_gid`,
    `st_nlink == 1`) → hand to scheduler (`fchownat(sock, scheduler_uid, scheduler_gid,
    AT_SYMLINK_NOFOLLOW)` + `fchmodat(sock, 0600, …)`, with the `/proc/self/fd`
    `EOPNOTSUPP` fallback) → restore owner and mode (`fchownat("", init_uid, init_gid,
    AT_EMPTY_PATH)` + `fchmodat(".", 0555, 0)`) → post-freeze `fstatat` re-verify →
    `Ctl::PoolReady { route, sock_dev, sock_ino }`), and `U1` freeze-before-unlink
    (`kill(-pgid, SIGKILL)`, `unlinkat`, restore `init_uid:init_gid 0555`, **no**
    `PoolReady`).
  - `S0` and `S3` fork through `child::ChildIo { keep: vec![], stdout, stderr }` so only fd
    0/1/2 reach the `execve` (`config_test_and_pool_child_exec_with_only_stdio`).
  - The inline `#[doc(hidden)] pub mod pool_test` harness (`PoolHarness`, `PoolReadyInfo` —
    distinct from `Ctl::PoolReady`), the fake `php-fpm` helper
    (`tests/support/fake_php_fpm.rs`), and a one-line `tests/fake_php_fpm_smoke.rs`
    (`#[path = "support/fake_php_fpm.rs"] mod fake_php_fpm;`) so Cargo actually builds the
    helper.
- This leaf does **not** edit `crates/blastit/src/lib.rs`: `MB.3.5.2` already made
  `supervise` public and narrowed `Parts`/`Supervisor`/`new` to `pub(crate)`.
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no member.

# Acceptance

```console
$ cargo test -p blastit --test fake_php_fpm_smoke --locked
$ cargo test -p blastit --locked
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
$ grep -q 'pub fn render_pool_conf' crates/blastit/src/supervise.rs && echo "pool file is pure and testable"
$ grep -q 'PoolReady' crates/blastit/src/supervise.rs && echo "spawn_pool publishes PoolReady"
$ ! grep -rn '"BLASTIT_' crates/blastit/src/supervise.rs && echo "pool env carries no BLASTIT_* name"
$ make check
```

# Notes

- The clippy gate belongs to this leaf (it is the one that adds the inline `pub` test
  module); `MB.3.5.2` deliberately did not run it.
- `SYS_bind`/`SYS_listen` are `EPERM` for the scheduler (landed in `MB.3.6.1.1`); the bind
  belongs to the route uid in the forked child — never "fix" a bind failure by loosening
  seccomp.
- The no-sudo promise is the page's headline: if the pool cannot start unprivileged as
  designed, stop and `human add` the conflict instead of shipping a `sudo` step.
- Fallback split if it overruns 550 lines: `.1` (`render_pool_conf` + `PoolConf` +
  `pool_test`) + `.2` (`S0`–`S7`/`U1` + fake helper + smoke test).
