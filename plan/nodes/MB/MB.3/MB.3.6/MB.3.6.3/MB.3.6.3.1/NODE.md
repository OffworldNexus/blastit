---
id: MB.3.6.3.1
title: "stage0 B1-B3 boot, parentfd argv handoff and the protected parent-fd suite - design MB.3.6b part 1"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.3.6.2.2]
paths:
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/config.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/dev_learn.rs
  - crates/blastit/tests/critical_fastcgi_parent_fd.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T10:51"
---

# Goal

Give the FastCGI route a boot that needs no `app` and a `parentfd` that crosses the
stage-0→stage-1 `execve` exactly once: the `B1`–`B3` boot rules, `check_fastcgi_entries`
per FastCGI route, the `--fastcgi-parent-fd` argv handoff validated **before** any
`OwnedFd`, the `BLASTIT_CHECK_*` plumbing, and the protected parent-fd suite (design leaf
`MB.3.6b`, part 1).

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.8.1 (the "parentfd handoff" block),
  §4.8.3 (`B1`–`B3`, `C1`), §4.9, §4.10.3 (`dev` config reads `BLASTIT_CHECK_*` before the
  `BLASTIT_*` strip) and §8's `MB.3.6.3.1` row; §8.2's `MB.3.6.3` row is the brief
  re-sync. The `can_boot_without_app` predicate is **§4.2/§8**, not §4.4.
- Files (the whole leaf, 6): `crates/blastit/src/{stage0,stage1,config,cli,dev_learn}.rs`
  and `crates/blastit/tests/critical_fastcgi_parent_fd.rs`.
- What lands:
  - `stage0.rs`: `B1`–`B3` — `openat2(RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS)`
    `parentfd` with the per-segment `O_NOFOLLOW` fallback on `ENOSYS`, `B1`'s
    `O_NOFOLLOW` + `lstat`, the ancestor walk that refuses a symlink or a route-uid/gid-
    writable ancestor, the boot-fatal `fastcgi listen "<v>": parent "<p>" is writable by the
    route uid / contains a symlink`, the immediate `init_uid:init_gid 0555` freeze, the
    missing `CAP_CHOWN` boot error naming the capability, and the `app_rule` narrowing
    (`config.app.is_none() && !manifest.can_boot_without_app()`).
  - Per FastCGI route: `check_fastcgi_entries(…, i, ProbeUid::Route(uid))` in production and
    `stage0::run_dev` with `ProbeUid::Dev`.
  - `stage1.rs`: `--fastcgi-parent-fd=<route>:<n>` in `exec_stage1`/`exec_stage1_dev`;
    `Options.fastcgi_parent_fds` validated **before** any `OwnedFd` is wrapped, via the
    pure `validate_parent_fds(&[(usize, RawFd)], config_fd, dbproxy_fds) -> Result<(), BootError>`;
    `Arc<OwnedFd>` shared with init's pool state; `ChildIo.keep` extended with
    `pinned_socket_fds()`; `SchedulerConfig.{route_lines, pinned_sockets, pool_spawn}`
    populated; `Option<Py<PyAny>>` (a no-app boot has no Python callable and must not panic).
  - `cli.rs`: the internal arm only with `--stage=1`, invisible in `--help`.
  - `config.rs`: `pub(crate) const POOL_PARENT_FD_ENV`, read by `Config::for_dev()` **before**
    the `BLASTIT_*` stripping (`MB.3.6.4.2` aliases it as `check::POOL_PARENT_FD`);
    `Config::to_json` excludes it; `Config.check_*` never appear in `to_json`.
  - `dev_learn.rs`: `DevRoot::adopt` wiring (`dev_learn.rs:180` already has it), `bind_all`
    at `slot_offset = 0`, and closing `check_sql_obs_fd` right after `fork_dbproxy_dev`.
  - Protected `crates/blastit/tests/critical_fastcgi_parent_fd.rs` (new): one row per
    §4.8.1 validation rule driven through the pure `validate_parent_fds` (so "no descriptor
    wrapped" is unit-tested before any `OwnedFd`), `cli::parse` rejecting
    `--fastcgi-parent-fd` without `--stage=1`, `--help` lacking the string, and
    `view_child_holds_neither_parentfd_nor_ctl_chan` (mixed FastCGI+WSGI fixture: bind a
    `UnixListener` at `listen`, `smoke.rs`-style `app`, one WSGI request, enumerate
    `/proc/<view_pid>/fd` — it holds neither the pinned parent inode nor init's `ctl.chan`
    inode). ~150 lines of harness, inside the 550-line cap.
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no member.

# Acceptance

```console
$ cargo test -p blastit --test critical_fastcgi_parent_fd --locked
$ cargo test -p blastit --locked
$ grep -q 'validate_parent_fds' crates/blastit/src/stage1.rs && echo "argv validated before any OwnedFd"
$ grep -q 'can_boot_without_app' crates/blastit/src/stage0.rs && echo "boot predicate is the manifest's"
$ ! grep -rn 'canonicalize' crates/blastit/src/stage0.rs crates/blastit/src/stage1.rs && echo "no caller-path resolution"
$ make check
```

# Notes

- `render_pool_conf` and the `S0`–`S7`/`U1` spawn sequence are `MB.3.6.3.2`'s. This leaf
  only pins the parent and passes the descriptor.
- Do **not** import `smoke.rs` (a separate test binary): duplicate the `children()`/`fds()`
  helpers locally, precedent `critical_dev_and_fail_closed.rs:482`.
- `cli.rs` is owned here for the internal arm; `MB.3.6.4.2` edits it later for
  `CheckOptions`, serialized by `depends_on` — no conflict.
- Fallback split if it overruns 550 lines: `.1` (`B1`–`B3`, `config`, `cli`) + `.2`
  (`stage1` handoff, `dev_learn`, entry call, parent-fd suite).
