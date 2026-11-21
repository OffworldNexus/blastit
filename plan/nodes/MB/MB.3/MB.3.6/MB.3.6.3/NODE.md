---
id: MB.3.6.3
title: "stage0 no-app boot, parentfd freeze and supervise::spawn_pool single *at sequence + PoolReady - design MB.3.6b"
kind: epic
status: superseded
priority: 3
depends_on: [MB.3.6.2]
paths:
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/config.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/dev_learn.rs
  - crates/blastit/src/supervise.rs
  - crates/blastit/tests/critical_fastcgi_parent_fd.rs
  - crates/blastit/tests/support/fake_php_fpm.rs
  - crates/blastit/tests/fake_php_fpm_smoke.rs
  - crates/blastit/tests/critical_fastcgi_pool.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T03:37"
updated: "2026-11-14T14:39"
---

HISTORICAL — written for the retired FastCGI/php-fpm design. ADR-0010 replaced it (embedded host NTS libphp in per-route view workers). Do not implement from this node.

# Goal

Give the FastCGI route a real pool and a boot that needs no `app`: stage 0 pins and freezes
the `listen` parent before any route process exists, the `parentfd` crosses the
stage-0→stage-1 `execve` exactly once, `supervise::spawn_pool` runs the one `*at`
bind/freeze/verify sequence that ends in `Ctl::PoolReady`, and the protected pool suite
proves it.

| Plan id | Lands | Depends |
|---|---|---|
| `MB.3.6.3.1` | `B1`–`B3`, per-route `check_fastcgi_entries`, `--fastcgi-parent-fd` handoff, `BLASTIT_CHECK_*`, protected parent-fd suite | `MB.3.6.2.2` |
| `MB.3.6.3.2` | `render_pool_conf`, `S0`–`S7`/`U1`, `pool_test` harness, fake `php-fpm` helper | `MB.3.6.3.1` |
| `MB.3.6.3.3` | protected `critical_fastcgi_pool` suite | `MB.3.6.3.2` |

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.8.1 (the parentfd handoff block), §4.8.2
  (steps 1–4, zero variants), §4.8.3 (`B1`–`B3`, `C1`, the unwind matrix), §4.9, §4.10.3 and
  §8's three rows; §8.2's `MB.3.6.3` row.
- **`can_boot_without_app` is §4.2/§8, not §4.4** (§4.4 is the WSGI port). The `app_rule`
  narrowing is `config.app.is_none() && !manifest.can_boot_without_app()`, which keeps
  `critical_static_cli`'s `ALL_STATIC` and `MIXED` assertions green while booting the PHP
  page's manifest.
- **`.1`** = `B1`–`B3` (`openat2` with the `O_NOFOLLOW` fallback, `O_NOFOLLOW`+`lstat`,
  the ancestor walk, the `init_uid:init_gid 0555` freeze, the `CAP_CHOWN` boot error), the
  `BLASTIT_CHECK_*` plumbing (`DevRoot::adopt` already at `dev_learn.rs:180`; `bind_all` at
  `slot_offset = 0`; close `check_sql_obs_fd` right after `fork_dbproxy_dev`), the `host=
  <runtime>/views/<uid>` + private `TMPDIR` config, and the `parentfd` handoff
  (`--fastcgi-parent-fd` in `exec_stage1`/`exec_stage1_dev`; `cli.rs` internal arm only with
  `--stage=1` and invisible in `--help`; `Options.fastcgi_parent_fds` validated **before**
  any `OwnedFd`; `Arc<OwnedFd>` shared with init's pool state; `ChildIo.keep` via
  `pinned_socket_fds()`; `Config::for_dev()` reads `POOL_PARENT_FD_ENV` before the
  `BLASTIT_*` strip, `MB.3.6.4.2` aliases it as `check::POOL_PARENT_FD`; `Config::to_json`
  excludes it).
- **`.2`** = `render_pool_conf` + `PoolConf`, `S0`–`S7`/`U1` (parent-side `setpgid`, `S1`
  reap-to-`ECHILD`, `S4`/`S6` pgid membership, freeze-before-unlink `U1`,
  `forking::fork(true)`), `HOME=<runtime>/views/<uid>/tmp`,
  `pm.max_requests=max(BLASTIT_VIEW_MAX_REQUESTS, 500)`, the fake `php-fpm` helper
  (`tests/support/fake_php_fpm.rs`), the `tests/fake_php_fpm_smoke.rs` one-line include, and
  the inline `#[doc(hidden)] pub mod pool_test` harness.
- **`.3`** = `critical_fastcgi_pool.rs` (`S0` fd-table / `setsid()` escapee /
  pgid-membership / `U1` suite). It duplicates the `children()`/`fds()` helpers **locally**
  (no `smoke.rs` import: it is a separate test binary; precedent
  `critical_dev_and_fail_closed.rs:482`).
- No child edits a `Cargo.toml` or lists `Cargo.lock`; `zygote.rs` is **not** in any path.

# Acceptance

- [ ] All three children `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ cargo test -p blastit --test critical_fastcgi_parent_fd --test critical_fastcgi_pool --locked
$ cargo test -p blastit --test fake_php_fpm_smoke --locked
$ cargo test -p blastit --locked
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
$ grep -q 'PoolReady' crates/blastit/src/supervise.rs && echo "spawn_pool publishes PoolReady"
$ ! grep -rn '"BLASTIT_' crates/blastit/src/supervise.rs && echo "pool env carries no BLASTIT_* name"
```
- [ ] `PoolReady { route, sock_dev, sock_ino }` is published only after the socket is `0600`
  scheduler-owned and the parent is restored to `init_uid:init_gid 0555`; every failure in
  the bind window runs `U1` and sends no `PoolReady`.

# Notes

- **Protected tests: add, never weaken.** `critical_fastcgi_parent_fd.rs` and
  `critical_fastcgi_pool.rs` are new; `critical_static_cli.rs` and
  `critical_static_manifest.rs` stay untouched.
- `SYS_bind`/`SYS_listen` are `EPERM` for the scheduler (`MB.3.6.1.1`); the bind belongs to
  the route uid in the forked child — never loosen seccomp to make a bind succeed.
- The no-sudo promise is the page's headline: a design conflict is a `human add`, not a
  `sudo` step.
