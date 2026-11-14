# Architect Resolution — `MB.3.3` (`php-fastcgi.md` + `ADR-0008`, Escalation #9)

- **Role:** `architect` (Escalation #9)
- **Branch:** `release/0.2.1`
- **Scope:** Resolve all BLOCKER and MAJOR findings across `security` (Rounds 1–3),
  `consistency` (Rounds 1–2), `testability` (Rounds 1–3), and `implementability`
  (Rounds 1–3) without modifying any protected `critical_*.rs` suite or weakening any
  zero-trust / DX invariant.

## 1. Pool Socket Lifecycle, fd/Inode Proof & Seccomp (`security` BLOCKER-1, R2 MAJOR-2, R3 MAJOR-3, R3 MAJOR-4, R3 MAJOR-6)

- **`security` BLOCKER-1 & R2 MAJOR-2 (bind window, owner+mode freeze, `fchownat` `0600`,
  `PoolReady` ordering, group kill, parent `listen` rules):** Normative syscall table
  added in **`php-fastcgi.md` §4.8.3** (`B1`–`B3`, `S0`–`S7`, `U1`, `C1`) and **`ADR-0008`
  §6**:
  1. **Boot (`B1`–`B3`, pre-fork):** Check ancestors `/` → `manifest_dir` (sticky-bit rule
     of `ARCHITECTURE.md` §5.3), open
     `manifest_dirfd = open(manifest_dir, O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)` and
     require `fstat`/`lstat` inode equality, `S_ISDIR`, `!S_ISLNK` and an absolute
     `/proc/self/fd` readlink; walk `rel_parent` via
     `openat2(O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC, RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS)`
     (creating only an absent final leaf `0555`; per-segment `O_NOFOLLOW` fallback on
     `ENOSYS`), refuse any ancestor writable by **any** route uid/gid or any shared /
     `static.root` / `entry`-ancestor parent, and freeze `parentfd` to
     `init_uid:init_gid 0555` (`B3`; any `EPERM` exits `1` naming `CAP_CHOWN`/`CAP_FOWNER`).
     `parentfd` crosses the stage-0→stage-1 `execve` **only** as the internal argv entry
     `--fastcgi-parent-fd=<route>:<n>` (`FD_CLOEXEC` cleared for that `execve`, re-set in
     stage 1 before any fork; `cli::parse` accepts the form only with `--stage=1` and never
     lists it in `--help`; never `SCM_RIGHTS`, never a path re-open). Stage 1 validates the
     list **before** wrapping any descriptor (unique route/fd, `fd >= 3`, not
     `config_fd`/dbproxy fd, `S_ISDIR`), then shares one `Arc<OwnedFd>` per route between
     `SchedulerConfig.pinned_sockets` and **init's per-route pool state, kept for the process
     lifetime** (`S1`–`S7`/`U1` run on it; it closes only at `init` exit), while
     `fork_scheduler`'s `ChildIo.keep` gives the scheduler its own copy and
     `http`/`dbproxy`/`view`/`pool` drop theirs.
  2. **Spawn (`S0`–`S7`):** Run `php-fpm -t -y` in a `route_uid` fork **before** opening
     the bind window (`S0`) through `child::ChildIo { keep: vec![], stdout, stderr }` so
     `close_fds_except` leaves only fd 0/1/2 — `ctl.chan`, `parentfd`, the events pipes and
     every `BLASTIT_CHECK_*` fd are closed before the `execve` — then `drop_privileges`,
     `harden_child`, `SeccompRole::View { egress }`, `set_dumpable(false)`; reap the previous
     pgid to `ECHILD` (bounded; not `ECHILD` → `U1`, the window never opens) then `unlinkat`
     (`S1`); open bind window `route_uid:route_gid 0755`
     while `RouteState.ready == false` (`S2`); fork `php-fpm` in `setpgid(0, 0)` **and**
     `setpgid(child_pid, child_pid)` in the parent right after the fork (`EACCES` = already
     set), through `ChildIo` with only stdio, with `SeccompRole::View { egress }` (`S3`);
     `fstatat(..., AT_SYMLINK_NOFOLLOW)` requiring `S_ISSOCK`, `route_uid:route_gid`,
     `st_nlink == 1`, **and the inode held by a task whose `/proc/<pid>/stat` `pgrp ==
     master_pid`**, recording `(sock_dev, sock_ino)` (`S4`); `fchownat` + `fchmodat(...,
     0600, AT_SYMLINK_NOFOLLOW)` (`O_PATH|O_NOFOLLOW` + inode check + `/proc/self/fd`
     `chmod` fallback on `EOPNOTSUPP`) (`S5`); **restore both owner and mode**
     `init_uid:init_gid 0555` and **re-verify** `fstatat` matches `(sock_dev, sock_ino)`,
     `scheduler_uid`, `0600`, **plus a re-run of the `S4` pgid-membership check** (`S6`);
     only then emit `Ctl::PoolReady { route, sock_dev, sock_ino }` over private `ctl.chan`
     (`S7`), now true by construction: no `route_uid` process ever holds an end.
  3. **Unwind (`U1`) & orphan containment:** Every failure in `S1`–`S6` (plus `RETIRE` and
     crash exit) runs `kill(-pgid, SIGKILL)`, **freezes first** (`fchownat`/`fchmodat` to
     `init_uid:init_gid 0555`), **then** `unlinkat(parentfd, sock_name, 0)` (`CAP_DAC_OVERRIDE`
     lets init unlink in `0555`; freezing first leaves no `0755 route_uid` gap), and emits no
     `PoolReady`, leaving an empty `0555` directory with nothing usable; a failed group kill
     (`ESRCH`/`EPERM`) never skips the freeze/unlink. A `setsid()` escapee stays capped by
     `route_uid`'s `RLIMIT_NPROC = workers + 2`, cannot write `0555` `parentfd`, and **cannot
     be published**: its socket's inode is not held by a task in the new pgid, so `S4`/`S6`
     fail it (residual: same-uid fd injection/`ptrace` only); named tests
     `critical_fastcgi_pool::pool_ready_inode_is_held_by_a_task_in_the_new_pgid` and
     `critical_fastcgi_pool::setsid_escapee_is_never_published` (`MB.3.6.3`) +
     `e2e/rootless/run.py::check_fastcgi_setsid_escapee_contained` (`MB.3.7`).
  4. **Parent vs role `listen` / seccomp:** `SeccompRole::Scheduler` gains
     `socket(args[0] == AF_UNIX)` while denying `AF_INET`/`AF_INET6`/`AF_NETLINK` and a
     `Scheduler`-only `SYS_bind`/`SYS_listen` `EPERM` rule (`SeccompRole::View` keeps
     `AF_UNIX` `bind`/`listen`).
- **`security` R3 MAJOR-3 / `implementability` R3 M1 (typed `parentfd` carrier in
  `BuildCtx`):** Added server-agnostic `PinnedSocket` (`Arc<OwnedFd>` `parentfd`,
  `sock_name`, `(parent_dev, parent_ino)`, `bound_inode`, `expected_uid`, `peer_uid`,
  `peer_gid`) to `blastit-backend` (§4.2.1), passed via `SchedulerConfig.pinned_sockets` →
  `BuildCtx.socket` in both production and `dev`/`check` (`DevProbe`), updated via
  `UpstreamSignal::ExecutorReady { socket_inode }`, and verified before every
  `connect("/proc/self/fd/<parent>/<name>")` (`C1`). Deleted "pool paths are opened by the
  role that uses them" and keyed `SessionPort` by `RouteId`.
- **`security` R3 MAJOR-4 (wire `String` panics & stranded global slot):** Pinned
  `String::from_utf8_lossy` on all wire/pool string conversions and added
  `SessionDoneGuard` + `RoutePermit` `Drop` guards (§4.2.1, §4.4) so any task panic
  synthesises `SessionMsg::Done(Fault)` once and releases the `BLASTIT_MAX_WORKERS` slot.
- **`security` R3 MAJOR-6 (proxy-control response headers):** Denied exactly four names —
  `X-Accel-Redirect`, `X-Sendfile`, `X-Lighttpd-Send-File`, `X-Rewrite-URL` — as
  `UpstreamError::Fault` (`502` before head) in **both** `blastit-fastcgi::cgi` and the
  shared `ResponseSink::head` (covering WSGI too), with conformance cases
  `forbidden_response_header` and `benign_x_accel_header_dropped` and
  `critical_fastcgi_dispatch` row 24; every other `X-Accel-*` name
  (`X-Accel-Buffering`/`-Expires`/`-Charset`/`-Limit-Rate`) is dropped without a fault, so
  a benign `X-Accel-Buffering: no` keeps the response `200` (Round-4 MINOR-19).

## 2. Consistency & `[route.fastcgi]` Surface (`consistency` R1 C6-M1..M2, R2 C7-M1..M4)

- **`C6-M1` & `C6-M2`:** Preserved: `ADR-0008` amends `ADR-0007` decisions 1, 4, 6;
  `can_boot_without_app()` is the single boot predicate; `docs/reference/manifest.md`'s 3
  exact edits are specified in §4.1 (`MB.3.5.2`), matching `docs/frameworks/php.md`'s
  3-key `[route.fastcgi]` grammar (`entry`, `listen`, `workers = 4`, no `env` key).
- **`C7-M1` (false §9 verdict claim):** Replaced with truthful Round 1–3 history; on-disk
  `*.review-*.md` files remain `VERDICT: FAIL` until the fresh Round-4 critic run.
- **`C7-M2` (trait signature, composition root, WSGI registration leaf):** Synced
  `plan(&UpstreamHead)` across §4.2 and §8.2 (`MB.3.3`, `MB.9.1`); pinned
  `crates/blastit/src/roles.rs::scheduler` calling `run_with(builtin_registry(), …)` as
  the single composition-root call site; pinned builder registration order (`MB.9.2.2`
  `"static"`, `MB.9.3` `"wsgi"`, `MB.3.6.2` `"fastcgi"`).
- **`C7-M3` (executable §8.1 plan checklist & numeric IDs):** Replaced letter aliases with
  real plan IDs, wrote the complete `tools/plan.py` split/`depends_on`/`paths` checklist in
  §8.1, assigned `stage0.rs` solely to `MB.3.6.3`, and added the `MB.3.6.2` `TempDir`
  `UnixListener` (non-socketpair) sync to §8.2.
- **`C7-M4` (`Executor` vs builder registry):** Dropped "manifest backend name"; named
  `blastit_manifest::Executor` (`Executor::kind()`, `ALL_KINDS`) in `ADR-0008` §1 and §4.2
  as the single source of executor kinds (1 enum variant + 1 `Upstream` + 1
  `BackendBuilder`).

## 3. Testability & Protected `critical_static_cli` (`testability` R1–R3 M1..M3)

- **Protected `critical_static_cli` `MIXED` fixture (`R1 M1`):** Scoped `check`'s
  external-executor branch (`§1.3`, `§4.10.3`, `§8`) strictly to
  `!manifest.is_all_static() && config.app.is_none() && manifest.can_boot_without_app()`.
  Because `MIXED` declares plain route `api` (neither `static` nor `fastcgi`) and no `app`,
  `can_boot_without_app()` is `false`, so `MIXED` falls through to branch 3 (pytest) and
  keeps its protected `"no interpreter to run pytest on"` exit `2` with zero edits to
  `critical_static_cli.rs`.
- **`testability` R3 M1 (under-declared capability & missing driver binary):** Pinned
  whole-slice `assert_eq!(IMPLEMENTATIONS, …)` in `conformance_table.rs` (`MB.9.4`) and
  added `Makefile` to `MB.3.6.1.2` and `MB.3.6.2` (plus a 5-binary `Makefile` assertion in
  `conformance_fastcgi.rs`) so neither dropping a capability bit nor deleting a driver
  binary can pass `make rs-test-conformance` or `make check`.
- **`testability` R3 M2 (binary single-owner refusal matrix & row 25 oracle):** Every row
  in §4.7 now names one `Owner file::fn`; row 18 is marked `—` (internal `500` builder
  guard, leaving 24 client `E` probes enforced by `--self-test` with non-empty
  discriminating key sets on `200` rows); row 25 (`504` + `ABORT_REQUEST` observed on fake
  peer + slot release) is pinned in §7.2 `critical_fastcgi_dispatch.rs`.
- **`testability` R3 M3 (discriminating post-`PoolReady` pivot oracle):** Replaced passive
  metadata checks with an active decoy `UnixListener` at `/sentinel/fpm.sock` (§7.2): after
  `PoolReady`, `route_uid` `chmod`/`O_CREAT`/`unlink` fail `EPERM`/`EACCES`, and after a
  privileged rename + `run -> /sentinel` symlink, live `GET /users/42` returns `200` from
  pinned `entry` while `/sentinel/fpm.sock` records **`0` `accept`s** (and respawn while
  symlinked fails closed `503` via `U1` with **`0` `accept`s** on sentinel).

## 4. Implementability (`implementability` R3 M1..M6)

- **`M1`:** Resolved via `PinnedSocket` in `BuildCtx.socket` (see Security MAJOR-3 above).
- **`M2` (`capacity()` data source):** Resolved via shared `Arc<RouteState>` created by the
  scheduler and passed in `BuildCtx.state` (§4.2.1, §4.4, §4.8.1); `SchedulerConfig.pool_spawn`
  (`Managed | External`) scopes external readiness strictly to `Pool` routes so `dev` still
  lazy-spawns WSGI views.
- **`M3` (leaf size caps):** Split all 5 over-budget leaves (`MB.3.5.{1,2}`,
  `MB.9.1.{1,2}`, `MB.9.2.1.{1,2}`, `MB.3.6.1.{1,2}`, `MB.3.6.4.{1,2}`) in §8/§8.1 so
  every leaf is ≤ 7 files and ≤ 550 lines.
- **`M4` (`check -- <suite>` child socket dir handoff):** Added
  `BLASTIT_CHECK_DEV_ROOT=<CheckRoot>/dev` (`check::DEV_ROOT`) + `DevRoot::adopt` +
  `bind_all` at `slot_offset = 0` (closing `check_sql_obs_fd` in `stage0` right after
  `fork_dbproxy_dev`) so `prepare_env` and child `dev`'s `<CheckRoot>/dev/views/0` match.
- **`M5` (`Cargo.lock` + `--locked` legality):** Pinned the two-step rule (first build
  offline without `--locked` to update `Cargo.lock`, then `--locked` and `make check`) in
  §4.2 and every lock-owning leaf's §8 acceptance cell.
- **`M6` (`MB.9.1` needs `MB.3.5`'s `Executor`):** Added `MB.3.5.1` to `MB.9.1.1`'s
  `depends_on` in §8, §8.1, and §8.2.

## 5. Invariants Refused to Trade

1. **Uniform injected `Upstream` dispatch:** Refused any FastCGI special-case branch in
   request dispatch (`StaticUpstream::plan` keeps 0.2.0 `rewrite::decide` semantics intact).
2. **Zero protected-test edits:** Refused modifying `critical_static_cli.rs`,
   `critical_static_manifest.rs`, or the 4 protected scheduler suites.
3. **No caller-path-resolved `chown`/`chmod`/`connect` & post-bind `0555` `init_uid` parent:**
   Refused `LISTEN_FDS` (breaks non-systemd Alpine `php-fpm`), `O_PATH` `parentfd` (`fchmod`
   `EBADF`), leaving `st_uid == route_uid` on `0555`, and `std::fs::canonicalize()`. The one
   path-shaped call is `chmod("/proc/self/fd/<sockfd>")` on an `O_PATH` fd whose inode was
   just verified (`S5`); it is descriptor-derived, not a manifest path.
4. **Closed param wall & proxy-control response wall:** Refused caller-named FastCGI params,
   `[route.fastcgi].env`, ambient `HTTP_PROXY`, forwarding `X-Accel-Redirect`/`X-Sendfile`,
   and faulting benign `X-Accel-Buffering`/`-Expires`/`-Charset`/`-Limit-Rate`.
5. **One descriptor path, no re-open:** Refused `SCM_RIGHTS` over `ctl.chan` (it does not
   exist at the stage-0→stage-1 `execve`), a `BLASTIT_*` env var carrying an fd across that
   `execve`, dropping `O_CLOEXEC` permanently, and any post-boot `openat*` on the parent
   segment; the fd crosses exactly once as an internal argv entry and is re-fenced with
   `FD_CLOEXEC` before any fork.
6. **No app-controlled process holds a control fd:** Refused letting `S0`'s `php-fpm -t`
   child or the `S3` pool master inherit `ctl.chan`/`parentfd`/`obs_fd`; both fork through
   `ChildIo` with only stdio.

## 6. Round-4 `security` Addendum (designer pass)

- **MAJOR-A (dev/check `parentfd` cannot cross `execve`):** Pinned the single handoff in
  `php-fastcgi.md` §4.8.1 (new "parentfd handoff" block), §4.2.1 (`PinnedSocket`),
  §4.5.2 (Socket identity), §4.8.3 (`B3`, `C1`) and `ADR-0008` decision 6: stage 0 opens
  `parentfd` `O_CLOEXEC`, crosses the stage-0→stage-1 `execve` as
  `--fastcgi-parent-fd=<route>:<n>` (the `--dev-obs-fd` pattern, `keep` +
  `set_cloexec(false)`), stage 1 wraps it in `OwnedFd`, re-sets `FD_CLOEXEC` before any
  fork, extends `fork_scheduler`'s `ChildIo.keep` with `pinned_socket_fds()`, and
  `check::serve_and_run` hands it to child `dev` as `BLASTIT_CHECK_POOL_PARENT_FD`, read by
  `Config::for_dev()` before `BLASTIT_*` stripping. Negative rule: no post-boot `openat*`
  on the parent segment; only `/proc/self/fd/<parentfd>/<name>`.
- **MAJOR-B (`S0` child inherits `ctl.chan`/`parentfd`):** `S0` and `S3` fork through
  `child::ChildIo { keep: vec![], stdout, stderr }`, so only fd 0/1/2 reach the `execve`;
  `S7`'s "held only by `init` and `scheduler`" is now true by construction. Test
  `critical_fastcgi_pool::config_test_and_pool_child_exec_with_only_stdio`.
- **MINOR-19..28:** Exact proxy-control names + benign `X-Accel-*` drop; `peer_uid`/
  `peer_gid` defined per mode; parent-side `setpgid`; `HOME` under `TMPDIR`;
  `pm.max_requests = max(BLASTIT_VIEW_MAX_REQUESTS, 500)`; `B1` `O_NOFOLLOW`+`lstat`;
  CVE-2024-8927 moved to row 11; `P4b` a positive control; `> 8192` value dropped;
  `setsid()` escapee test named. All recorded in `php-fastcgi.md` §9 "Round-4 disposition".

## 7. Round-5 `security` Addendum (designer pass)

- **MAJOR-A residual (ownership / argv / probe):** `php-fastcgi.md` §4.8.1 now pins the whole
  chain: stage 0 opens `parentfd` `O_CLOEXEC` → internal argv `--fastcgi-parent-fd` (only with
  `--stage=1`, invisible in `--help`, `cli.rs` owned by `MB.3.6.3`) → stage 1 validates the
  list **before** wrapping (unique route/fd, `fd >= 3`, not `config_fd`/dbproxy fd, `S_ISDIR`)
  → one `Arc<OwnedFd>` per route shared by `SchedulerConfig.pinned_sockets` and **init's
  per-route pool state for the process lifetime** (`S1`–`S7`/`U1` run on it; closes only at
  `init` exit) → scheduler copy by fork → `http`/`dbproxy`/`view`/`pool` drop theirs. The
  misleading "closes when `fork_scheduler` returns" sentence is deleted. `SocketProbe::check`
  returns `Result<Option<OwnedFd>, …>` (`DevProbe` returns the opened fd,
  `check_fastcgi_sockets -> Vec<(usize, OwnedFd)>`, `serve_and_run` threads it), so `dev`/
  `check` resolve once and the child re-validates without a second `openat2`.
- **MAJOR-C (`S2` bind window):** `S1` reaps the previous pgid to `ECHILD` before the window
  opens; `S4` records an inode only if it is held by a task whose `/proc/<pid>/stat` `pgrp ==
  master_pid`, and `S6` re-runs that check after the freeze; `U1` freezes before `unlinkat`.
  The false "never matches a subsequent `PoolReady`" claim is replaced by the membership rule
  + its residual; the §7.2 oracle now asserts `PoolReady.sock_ino` **equals** the inode the
  new master bound, and an escapee rebind yields no `PoolReady`, `U1`, `503`, `0` accepts.
- **MINOR-29/30:** "zero path-based `chmod`" → "no caller-path-resolved `chown`/`chmod`/
  `connect`; the only path-shaped call is `chmod("/proc/self/fd/<sockfd>")` on a verified
  `O_PATH` fd"; `SO_PEERCRED` accepts exactly `{route_uid:route_gid}` (prod) /
  `{invoking_uid:invoking_gid}` (`dev`/`check`), no gVisor tolerance, `scheduler_uid`
  rejected, negative test `foreign_uid_peer_is_fault_502`.
- **MINOR-31..37 (follow-up pass, now fixed):** (31) child `dev` is `Host::stage1()`'s
  `Reexec` + `dev --port 8080` (`/proc/self/exe` or `<python> -I -m blastit`), never
  `<current_exe> dev`; the dbproxy clause is gone. (32) `check_pool_parent_fds`/
  `check_sql_obs_fd` are absent from `Config::to_json`, asserted by
  `config_document_carries_no_check_fd`. (33) the fd-table oracle keeps only the negative
  half (`fcntl(3, F_GETFD)`/`read(3, …)` → `EBADF`, no parent/socket target), because a
  wrapper's own fds make `{0,1,2}` unobservable. (34) the ancestor claim is narrowed to
  `B1`'s sticky-bit rule above `manifest_dir` + `B2` below it (a route-owned `manifest_dir`
  is a self-DoS `503`). (35) a non-`tchar` CGI header name is `Fault`; the four names are
  compared on the token name. (36) `spawn_pool` forks via `crate::forking::fork(true)`, the
  `zygote::fork_view` pattern, never raw `libc::fork`. (37) row 17 is `> 110`/`> 112`
  `HTTP_*` headers derived from the 18-param fixed set.

## 8. Round-4 `implementability` Addendum (designer pass)

- **M1/M2 (`BootError.line`, `run_with`):** `RouteSpec.line` from
  `SchedulerConfig.route_lines`; `run_with -> Result<(), RunError>` with `RunError` and
  `SchedulerError::Boot` in `crates/blastit-scheduler/src/error.rs` (`MB.9.2.1.2`), `run`
  unchanged in signature.
- **M3/M4/M5 (contract API):** `PinnedSocket::new` + accessors (`BadName`/`Poisoned`, pure);
  `BuildCtx.external_executor` (contract-owned) makes `ensure_ready` a no-op in
  `dev`/`check`; `RouteState::acquire(self: &Arc<Self>) -> RoutePermit` (`Send`, owns the
  `Arc`).
- **M6 (probe deps):** `SocketProbe`/`check_fastcgi_sockets` stay pure in `blastit-manifest`;
  `DevProbe`/`ProductionProbe` move to `crates/blastit/src/probe.rs` (no manifest
  `Cargo.toml` change).
- **M7–M9 (paths):** `tests/common/mod.rs` in `MB.3.6.1.1`; `peer_gid = invoking_gid`;
  `MB.3.6.3.1` owns `cli.rs`/`dev_learn.rs` (`DevRoot::adopt` is at `dev_learn.rs:180`),
  `zygote.rs` dropped.
- **M10 (budgets):** `MB.3.6.2` → `.1`/`.2`; `MB.3.6.3` → `.1`/`.2`; Order/§8.1/§8.2 synced;
  fallback splits stated for `MB.9.2.1.2`/`MB.9.2.2`/`MB.9.3`.
- **M11/M12 (types, transitional registry):** `Capability(u32)` + `HeadSpec`/`OutcomeKind`/
  `PeerScript`/`CancelAt` defined; `run_with` leaves `stages` empty until `MB.9.3` deletes the
  fallback (missing builder → `RunError::Boot`).
- **One-liners:** `SocketProbeError` variants; `ProductionProbe -> Ok(None)`; full `ChildIo`
  literal; `builtin_registry() -> Result<Registry, BootError>`; fake `php-fpm` helper at
  `crates/blastit/tests/support/fake_php_fpm.rs`; §8.1 redundancy removed.

## 9. Round-5 `implementability` Addendum (designer pass)

- **MAJOR-1:** `PinnedSocket::new(parent: Arc<OwnedFd>, …)` + `parent_fd()` via `as_fd()`;
  `SchedulerConfig.pinned_sockets` shares the same `Arc` init keeps (no `try_unwrap`/`dup`).
- **MAJOR-2:** `exchange` takes `BodyStream<'static>` + `Box<dyn ResponseSink + Send>`;
  `Scheduler::session` is a scheduler-side `tokio::spawn` returning
  `Session { id, done }`; `SessionDoneGuard` delivers one `Done`; `SessionPort` is WSGI-only.
- **MAJOR-3:** `crates/blastit/src/supervise.rs` joins `MB.3.6.1.1` (production `Ctl` match)
  and its acceptance builds `-p blastit`.
- **MAJOR-4:** `crates/blastit/src/lib.rs` has one owner (`MB.3.5.2`): `pub mod probe;` +
  `pub mod supervise;`; `MB.3.6.3.2` does not edit it.
- **MINOR-5/11 folded** (`run` expression, `SchedulerError::Boot` `#[error("{0}")]` +
  `BootError: std::error::Error`); **MINOR-6..10 remain open** and are recorded in §9 of the
  design.

## 10. Round-6 `implementability` Addendum (designer pass)

- **MAJOR-A:** `Executor`/`FastCgiPolicy` derive `Debug, Clone, PartialEq, Eq` (no `Copy`);
  pool spawn mode renamed `PoolSpawn { Managed, External }` (avoids
  `crate::config::SpawnMode`); owns: `MB.3.5.1` (derives), `MB.9.2.1.1` (`PoolSpawn`).
- **MAJOR-B:** `PoolPort::ensure_pool(route)` (contract) + `CtlPoolPort` from `run_with`,
  passed in `BuildCtx.pool`; `ensure_ready` uses it only when `!external_executor`;
  `SessionPort` stays WSGI-only; first request waits for `PoolReady` under
  `request_timeout`.
- **MAJOR-C:** one `SessionMsg::Done { id, result }`, bounded `mpsc::Sender` (cap `1`),
  receiver in `self.sessions`, `try_acquire` before `Scheduler::session(..., permit)`,
  `SessionDoneGuard` the single emitter.
- **MAJOR-D:** inline `#[doc(hidden)] pub mod pool_test` in `supervise.rs`
  (`PoolHarness`/`PoolReady`), unprivileged subset against a temp `UnixListener`; `lib.rs`
  stays `MB.3.5.2`'s.
- **One-liners:** lifetime-free `BodyStream`; `MB.3.8` merged to 7 entries;
  `pinned_socket_fds()` → `MB.9.2.1.1`; `SocketProbe`/`ProbeUid`/`check_fastcgi_entries` →
  `MB.3.5.2`; `on_ctl` `Ctl::Pool*` → `MB.3.6.2.1`; `foreign_uid_peer_is_fault_502` →
  `MB.3.6.2.2`; fallback splits `MB.9.1.1` `.1a`/`.1b` and `MB.3.6.3.2` `.2a`/`.2b`.

## 11. Round-7 `implementability` Addendum (designer pass)

- **MAJOR-1:** contract `BodySource` + `CancelToken(Arc<AtomicBool>)`; the tokio `BodyStream`
  is scheduler-side; `exchange` takes `Box<dyn BodySource + Send>`; `blastit-backend` has no
  `tokio` (the `MB.9.1.1` grep still passes).
- **MAJOR-2:** §4.4 pool start is `PoolPort::ensure_pool`; every remaining `SessionPort`
  mention is WSGI-only.
- **MAJOR-3:** `crates/blastit/src/stage1.rs` joins `MB.9.2.1.1` (7 paths) with
  `cargo test -p blastit --locked`.
- **MAJOR-4:** unconditional `MB.9.2.1.2` + `MB.9.2.1.3` and `MB.3.6.3.2` + `MB.3.6.3.3`;
  mapping/Order/§8.1/§8.2 updated; lock chain and protected-test rules unchanged.

## 12. Round-8 `implementability` Addendum (designer pass)

- **MAJOR-1:** `builtin_registry()` is defined in `scheduler.rs` (`MB.9.2.1.2`), re-exported
  from `lib.rs` (`MB.9.2.1.3`); `run` delegates there; `MB.9.2.2`/`MB.9.3`/`MB.3.6.2.1`
  append their builders in `scheduler.rs`.
- **MAJOR-2:** one `RouteState::new(pool_spawn == PoolSpawn::External, workers)` rule —
  `Ready` in `External`, `Busy` in `Managed` until `PoolReady`.
- **MAJOR-3:** `ObservationKind` internally tagged (`tag = "kind"`), so `flatten` works;
  exact-line tests named.
- **MINOR-1..6:** §8.2 labels `.1/.2/.3`; `stage1.rs` reason corrected (default tail, not
  exhaustive); `SchedulerConfig.{route_lines,pinned_sockets,pool_spawn}`; `http_obs:
  Option<PathBuf>`; `--kind epic` step; `MB.9.2.1.2` fallback `.2a`/`.2b`.

## 13. Round-9 `implementability` Addendum (designer pass)

- **MAJOR-1:** module declarations have one owner each — `MB.3.4.2` owns
  `blastit-fastcgi/src/lib.rs` (`pub mod lockdown;`), `MB.9.2.2` owns
  `blastit-scheduler/src/upstream/mod.rs` (`pub mod static;`), and `MB.9.2.1.2` declares
  `mod upstream;` in `scheduler.rs`.
- **MAJOR-2:** `MB.3.5.2` owns `supervise.rs`, narrows `Parts`/`Supervisor`/`new` to
  `pub(crate)` (only `render_pool_conf`/`pool_test` public) and runs
  `cargo clippy -p blastit --all-targets --locked -- -D warnings`.
- **MINOR-1..5:** ADR `try_acquire`; `tests/fake_php_fpm_smoke.rs`; `#[path]` include for
  `common/fake_upstream.rs`; `.2b` carries the four remaining deliverables; the
  `--kind epic` list names all eleven parents and `MB.3.4.2` depends on `MB.3.4.1`.

## 14. Round-10 `implementability` Addendum (designer pass)

- **MAJOR-1:** codec-local `LockdownError`/`CgiError`/`RecordError` in `blastit-fastcgi`;
  `MB.3.6.2.1` maps them to `Refusal`/`UpstreamError::Fault`; no early `blastit-backend`
  edge.
- **MAJOR-2:** `FastCgiPaths` is defined in `blastit-fastcgi::lockdown` and built by
  `FastCgiBuilder`; the scheduler row no longer claims it.
- **MAJOR-3:** `render_pool_conf(&PoolConf<'_>) -> String` with std-only `PoolConf`;
  `Parts`/`Supervisor`/`new` stay `pub(crate)`; clippy gate moved to `MB.3.6.3.2`.
- **MINOR-1..6:** `ForeignOwner { uid }`; `.3` re-exports `builtin_registry`/`run_with`/
  `RunError`/`SchedulerError`; `run_with` generic bound; `MB.3.6.3.2` fallback `.2a`/`.2b`;
  §8.1/§8.2 authoritative until applied; clippy gates on every test-adding leaf.
- **Structural:** §8's type-ownership matrix is the single source of truth for type
  assignments; §4.x/§8/§8.1/§8.2 are reconciled against it.

## 15. Round-11 `implementability` Addendum (designer pass)

- **MAJOR-1:** `MB.3.6.1.2` adds `blastit-backend` to `crates/blastit/Cargo.toml`, with
  `cargo test -p blastit --locked` and a `cargo tree` grep in acceptance; `MB.3.6.3.1` is a
  `PinnedSocket` reader.
- **MAJOR-2:** `MB.9.2.1.1` adds `blastit-http` to `crates/blastit-scheduler/Cargo.toml`
  (for `BodyFraming`), with a `cargo tree` grep.
- **MAJOR-3:** observation-path derives pinned (`Debug, Clone, PartialEq, Eq, Serialize,
  Deserialize`; `Copy` on `ParamDecision`/`OutputStream`), `UpstreamHead` `Debug/Clone/
  PartialEq/Eq`, `PinnedSocket` `Clone/Debug`, `PoolConf` `Debug/Clone/Copy`.
- **MINOR-1..7:** clippy only in §8's `MB.3.6.3.2`; `Session`/`SessionMsg` `pub(crate)`;
  `.3` dropped from the `Registry` reader row; `RecordError::Truncated`; `PoolReadyInfo`;
  `new_with_upstream` signature; `MB.3.6.2.1` fallback `.1a`/`.1b`.
- **Structural:** §8 now carries three matrices — type ownership, crate edges, derives —
  and they are the single source of truth for assignments.

## 16. Round-12 `implementability` Addendum (designer pass)

- **MAJOR-1:** `mod upstream;` is declared in `crates/blastit-scheduler/src/lib.rs` (crate
  root); `lib.rs` joins `MB.9.2.1.2` (5 paths), `.3` only appends `pub use` lines.
- **MAJOR-2:** `ReqHeadView::scheme() -> Scheme` (frame-owned); `begin_raw` maps to `Tls`;
  the crate-edge matrix adds `blastit-frame → (no new edge)`.
- **MINOR-1..10:** matrix readers for `MB.3.6.3.1`; `Session`/`SessionMsg` behaviour-only;
  `MB.9.1.2` names all five conformance types; `fastcgi_builder.rs` owned;
  test-lint wording; `POOL_PARENT_FD_ENV`; matrix footnote; conformance binary counts
  3→4→5; `MB.3.6.2.1` drops `lib.rs`; §4.2 block `pub(crate)`.

## 17. Round-13 `implementability` Addendum (designer pass)

- **MAJOR-1:** `MB.3.5.1` re-exports `Executor`/`FastCgiPolicy` from
  `crates/blastit-manifest/src/lib.rs` (`pub use model::{…}`), with a `lib.rs` doctest
  proving `use blastit_manifest::{Executor, FastCgiPolicy};`.
- **MINOR-1..8:** `blastit_frame::Scheme` root path; `Scheme::from_code(..)?` error policy;
  `#![deny(missing_docs)]` sentence; `Tls` in the derive matrix; `PoolConf` + conformance
  types in the type matrix; `blastit-http → (no blastit-backend)` + `serde` `derive`
  feature; `pub async fn run_with`; `MB.3.8` "7 path entries / 9 files".

## 18. Round-14 `implementability` Addendum (designer pass)

- **MAJOR-1:** the entry check is injected: `SocketProbe::check_entry(manifest_dir, entry,
  root, uid)` with `NotRegularFile`/`Unreadable { uid }`/`OutsideRoot`; `check_fastcgi_entries`
  takes `&dyn SocketProbe`; the `openat2`/readability body is in `crates/blastit/src/probe.rs`,
  so `blastit-manifest` stays I/O-free.
- **MINOR-1..7:** `ProbeUid::Dev`; `ALL_KINDS: &'static [&'static str]`; `docs_fences.rs`
  three edits; `FastCgiPolicy`/`Executor` readers; serialized `lib.rs` ownership; doctest
  proves `kind()`/`ALL_KINDS`; crate-edge row wording.

## 19. Round-15 `implementability` Addendum (designer pass)

- **MAJOR-1:** `ProductionProbe::check_entry` uses `setfsuid`/`setfsgid` around
  `openat2` + `faccessat2(AT_EACCESS)`, restores on every path (failure boot-fatal);
  `DevProbe` skips it; oracles are the fake-probe unit and the root-only
  `check_fastcgi_entry_0000_refused_for_route_uid` e2e case.
- **MINOR-1..7:** per-route `check_fastcgi_entries(.., route_index, uid)`;
  `provides_parent_fd()`; three `FastCgiEntryInvalid.rule` phrases; `ProbeUid`
  authoritative; `ProbeUid` derives/docs; the no-`rustix` grep in `MB.3.5.2`;
  `MB.3.5.2` fallback `.2a`/`.2b`.

## 20. Round-16 `implementability` Addendum (designer pass)

- **MAJOR-1:** the readability check is a **probe child**, not `setfsuid`: fork from
  single-threaded stage 0, `setresgid`/`setresuid` to the route uid (caps drop), harden +
  `SeccompRole::View { egress: false }`, `openat2`/`fstat`/`faccessat2(AT_EACCESS)`, one
  reply byte; missing/garbled/non-zero reply is boot-fatal. `DevProbe` needs no child.
- **MINOR-1..4:** gid == uid; `rule: String` + the 7-row phrase table; both call sites
  (`stage0::run_dev` `ProbeUid::Dev`, production `ProbeUid::Route(uid)`); oracle split
  (unprivileged fake probe vs root-only e2e).

## 21. Round-17 `implementability` Addendum (designer pass)

- **MAJOR-1:** `faccessat2` is called through
  `libc::syscall(SYS_faccessat2, fd, "", R_OK, AT_EMPTY_PATH|AT_EACCESS)`; `rustix::fs::accessat`
  rejects `EMPTY_PATH`.
- **MAJOR-2:** the child's only identity change is `blastit_sys::drop_privileges(uid)`
  (explicit `capset`); inlining `setresgid`/`setresuid` is forbidden (rootless `init` keeps
  `CAP_DAC_OVERRIDE`).
- **MAJOR-3:** `O_PATH|O_NOFOLLOW|O_CLOEXEC` + `fstat` `S_ISREG` + `faccessat2`; 5 s parent
  wait, `SIGKILL` + `SocketProbeError::Timeout` on expiry; `DevProbe` same flags.
- **MINOR-1..11:** `fork(interpreter)`; `M` byte + `ForeignOwner` off the entry table; parent
  pipe hygiene with `libc::waitpid`; `ProductionProbe` comment; `manifest_dirfd` doc;
  `strip_prefix(root)`; `render_pool_conf` wording; three named fallbacks; `DevProbe`/
  `ProductionProbe` matrix row; duplicate paragraph removed; errno mapping + distinct
  garbled-reply message.

## 22. Round-18 `implementability` Addendum (designer pass)

- **R18-1:** reply set `O/U/N/S/R/M/I/T`; outside-set or short reply is boot-fatal, diagnostic
  bytes map to their variants and every non-`O` byte aborts boot.
- **R18-2:** `Timeout` in the `SocketProbeError` enumeration + `probe_timeout_is_boot_fatal`.
- **R18-3:** `ProductionProbe.seccomp` feeds `install_filter`, honouring `BLASTIT_SECCOMP=0`.
- **R18-4:** `faccessat2` `ENOSYS` refuses boot with an explicit message; kernels < 5.8 are
  unsupported for `[route.fastcgi]`.

## 23. Round-4 `testability` Addendum (designer pass)

- **MAJOR-T1:** `crates/blastit/tests/critical_fastcgi_parent_fd.rs` (owned by
  `MB.3.6.3.1`, 6th path) drives crafted `--fastcgi-parent-fd` argv per rule (exit 1 +
  message) plus the `FD_CLOEXEC` positive; unprivileged, root-only legs stay in `MB.3.7`.
- **MAJOR-T2:** `lazy_pool_bind_timeout_is_503_retry_after_never_502` asserts `503`,
  `Retry-After: 1`, no `502`, slot release, later `PoolReady` admits.
- **MAJOR-T3:** `workers_cap_answers_503_for_the_second_concurrent_request` (`workers = 2`,
  three concurrent) plus `capacity()` equality for `Pool`/`View`.
- **MINOR-T1..T12:** row 17/2 owners; `MIXED` reasoning; `P10` `431`; exact `cases()` set;
  whole `--json` fence + exempt Deployments fence; `check_entry` `ENOSYS`/reply parser;
  manifest/seccomp fn names; plan drift; `R16` symbols; `R19`/`R10`/`R18` edges; pool
  single-UID fixture.

## 24. Round-5 `testability` Addendum (designer pass)

- **MAJOR-T1:** the view/zygote `ChildIo.keep` omits `pinned_socket_fds()`;
  `critical_fastcgi_parent_fd::view_child_holds_neither_parentfd_nor_ctl_chan` (mixed
  FastCGI + WSGI manifest) asserts `/proc/<view_pid>/fd` holds neither the pinned parent nor
  `ctl.chan`; unprivileged, in `make check`.
- **MINOR-T1..T10:** §7.1 inventory (parent-fd suite `.3.6.3.1`, pool `.3.6.3.3`); §8.1
  item 2 paths; `cli` arm test; invalid `BLASTIT_CHECK_POOL_PARENT_FD` row; pure
  `validate_parent_fds`; `5 s + 1 ms` + `Unavailable` variant; `Barrier` synchronization;
  `R2`/`R3` row + `R9`/`R20` reference; `--self-test` `#n` union `{1..20}`; `cli.rs`
  serialized ownership.

## 25. Round-6 `testability` Addendum (designer pass)

- **MAJOR-T1:** `external_mode_never_calls_ensure_pool` (recording `PoolPort` fake, `0` vs
  `1` calls) and `external_pool_never_spawns_php_fpm` (`/proc/<init_pid>/…/children` has no
  `php-fpm`) prove the External promise; both unprivileged.
- **MINOR-T1..T6:** §7.1 "Proves" clause; `MB.3.6.3.1` Lands names the view oracle; the
  predicate is init's `ctl.chan` inode absent from the view's table (and `view.chan` present);
  preconditions (`200`, view pid via `children`, non-empty fd set, no skip); mixed-fixture
  contract (`app`, `UnixListener` at `listen`, no `php-fpm`, `smoke.rs` helpers); the
  init-side assertion is `requires-root` in `e2e/rootless` plus an unprivileged `dev` inode
  check.

## 26. Round-7 `testability` Addendum (designer pass)

- **MINOR-T1:** the scanner states the `200` precondition, walks
  `/proc/<init_pid>/task/*/children` transitively, and has a positive control (renamed
  `/bin/sleep`).
- **MINOR-T2:** the view predicate asserts both the pinned parent `(st_dev, st_ino)` and
  init's `ctl.chan` inode, with `view.chan` present.
- **MINOR-T3:** `critical_fastcgi_parent_fd.rs` in the `R2`/`R3`/`R20` Suite column.
- **MINOR-T4:** `smoke.rs` is not importable; both leaves duplicate `children()`/`fds()`
  locally (precedent `critical_dev_and_fail_closed.rs:482`), no path change.

## 27. Round-3 `consistency` Addendum (designer pass)

- **C8-M1:** `env[HOME]` is `<runtime>/views/<uid>/tmp` everywhere.
- **C8-M2:** `pm.max_requests = max(BLASTIT_VIEW_MAX_REQUESTS, 500)`; the constant lives in
  `supervise.rs` (`MB.3.6.3.2`).
- **C8-M3:** §8.2 row for the `exchange`/`ExchangeOutcome`/`Observation`/`Scheduler::session`
  contract shape across `MB.3.3`/`MB.9.1.1`/`MB.9.2.1.2`/`MB.3.6.2.1`.
- **C8-M4:** §8.1 split bullets point at "exactly the `Files` cell of §8's row for that id".
- **C8-m1..m9:** `<route_index>` placeholder; fixed-phrase list; `B1`/`B2` ancestor rule;
  app-present predicate; `PoolSpawn` history; numeric fallback children; three §8.2 rows;
  `Registry::new` allowed set; `Ctl::Spawn` doc comment.
