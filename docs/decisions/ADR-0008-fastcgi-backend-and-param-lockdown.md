# ADR-0008: Injected `Upstream` Contract and FastCGI Param Lockdown

- **Status:** Proposed
- **Date:** 2026-11-14
- **Node:** MB.3.3
- **Owner:** architect

## Context

`0.2.1` promises PHP-FPM support (`docs/frameworks/php.md`): a `[route.fastcgi]`
block, a front controller that runs with no nginx regex soup, zero `sudo` locally, a
pool per route as that route's uid in production, and a harness that refuses the
`php2shell` class by construction. Today the scheduler selects a backend in place:
`static_asset.rs`/`static_response.rs` are called from `scheduler.rs`/`rewrite.rs`,
`RouteInfo` carries `Option<StaticPolicy>`/`static_forward`, and the WSGI path is wired
inline. Adding PHP that way would add a third branch, a third observation shape and a
fourth parser differential. The owner requirement (2026-11-14) is explicit: one common
trait implementation per language/server, injected — DI/IoC in the spirit.

## Decision

1. **One object-safe contract, injected per route.** `crates/blastit-backend` defines
   `pub trait Upstream` (`kind`, `capacity`, `ensure_ready`, `signal`, `plan`, `exchange`),
   its request/response/observation/error types (`RouteId`, `RouteState`, `PinnedSocket`,
   `UpstreamHead`, `ResponseSink`, `Observation`, `UpstreamError`, `Refusal`), a `BoxFuture`
   alias (no `async_trait`, no new dependency) and a boot `Registry` of `BackendBuilder`s.
   `blastit_manifest::Executor` (`Executor::kind()`, `Executor::ALL_KINDS`; `Executor` and
   `FastCgiPolicy` are re-exported at the crate root by `MB.3.5.1`, with a `lib.rs` doctest
   proving `use blastit_manifest::{Executor, FastCgiPolicy};`) is the single
   source of executor kinds alongside `"static"`; `Registry::new` rejects duplicate kinds
   and any kind outside `"static"` ∪ `Executor::ALL_KINDS`, and `Registry::build_route` looks
   up `executor.kind()` (failing
   closed at boot with `./blastit.toml:<line>: no builder registered for backend "…"` when
   omitted). The scheduler holds one non-optional stage chain per route (`RouteRuntime {
   info, stages }`), built once at boot; request dispatch calls `plan`/`exchange` and never
   matches on backend kind (backend kind is consulted only at boot in `Registry::build_route`
   and in process lifecycle `Ctl::Spawn`/`Ctl::Pool*`). Every session task holds a
   `SessionDoneGuard` (`Drop` synthesises `Done(Fault)` once on panic so no global slot can
   strand) and converts wire/pool strings via `String::from_utf8_lossy`.
   **Buildability pins:** `RouteSpec` carries `line: u32` (from
   `SchedulerConfig.route_lines`, built by `stage1` from `Manifest::route_lines()`), so
   `BootError` can print `./blastit.toml:<line>`; `run_with(...) -> Result<(), RunError>`
   with `RunError { Boot(BootError), Scheduler(SchedulerError) }` (and
   `SchedulerError::Boot` in `crates/blastit-scheduler/src/error.rs`), while `run` keeps its
   0.2.0 signature; `PinnedSocket::new` + accessors make the type constructible and readable
   outside `blastit-backend` (pure name validation; `Poisoned` for the `Mutex`, no `unwrap`);
   `RouteState::try_acquire(self: &Arc<Self>) -> Option<RoutePermit>` gives a `Send + 'static` permit;
   `BuildCtx.external_executor` is contract-owned so `ensure_ready` never spawns a pool in
   `dev`/`check`; production pool starts use the distinct `PoolPort::ensure_pool(route)`
   (one `Ctl::Spawn`, `CtlPoolPort` from `run_with`), not `SessionPort`. `PinnedSocket::new`
   takes the `Arc<OwnedFd>` the caller keeps (no `try_unwrap`/`dup`/`unsafe`);
   `Upstream::exchange` takes `Box<dyn BodySource + Send>` (the contract trait; the concrete
   tokio `BodyStream` lives in `blastit-scheduler`, and the contract's `CancelToken` is
   `Arc<AtomicBool>`, so `blastit-backend` stays free of `tokio`/`std::sync::mpsc`) and
   `Box<dyn ResponseSink + Send>` so a `'static` task can hold both, and the scheduler's
   `Session`/`SessionMsg::Done { id, result }` (`Scheduler::session(..., permit)`, one
   `tokio::spawn` per request, bounded `mpsc::Sender` capacity `1`, receiver kept in
   `self.sessions`) delivers exactly one `Done` via `SessionDoneGuard`; admission is
   `RouteState::try_acquire() -> Option<RoutePermit>` before the task (`None` → `503`).
   `SessionPort` is WSGI-only. `Executor`/`FastCgiPolicy` derive `Debug, Clone, PartialEq,
   Eq` (no `Copy`) to satisfy `RouteInfo`, and the pool spawn mode is
   `PoolSpawn { Managed, External }` (not `SpawnMode`, which already means `Lazy`/`Eager`).
   `Capability` is a hand-rolled `u32` bitflag (no `bitflags`) and
   `HeadSpec`/`OutcomeKind`/`PeerScript`/`CancelAt` are defined in `conformance.rs`.
   `builtin_registry()` is defined in `scheduler.rs` (where `run` lives) and only
   re-exported from `lib.rs`; each builder-registering leaf appends to the list in
   `scheduler.rs`. A managed pool's `RouteState` starts `Busy`
   (`RouteState::new(pool_spawn == PoolSpawn::External, workers)`) and flips only on the
   verified `Ctl::PoolReady`; `External` starts `Ready`. `ObservationKind` is internally
   tagged (`#[serde(tag = "kind", rename_all = "snake_case")]`) so `#[serde(flatten)]` on
   `Observation` can produce the pinned `{"kind":"wire_request",…}` line; every type on that
   path derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` (`ParamDecision`/
   `OutputStream` also `Copy`) because `CheckObsV1` is `PartialEq, Eq`.
   **Cargo edges:** `blastit-backend → blastit-http|blastit-manifest|serde`;
   `blastit-scheduler → blastit-backend|blastit-http|blastit-fastcgi|blastit-frame`;
   `blastit → blastit-backend|blastit-fastcgi|blastit-scheduler`; `blastit-fastcgi` stays
   production-zero-dep; `blastit-frame` gains **no** edge (`ReqHeadView::scheme()` returns
   the frame-owned `blastit_frame::Scheme` root re-export — `raw.rs` uses
   `crate::frame::Scheme` — and `begin_raw` maps `Scheme::Http|Https → Tls::Http|Https`;
   `Tls` derives `Debug, Clone, Copy, PartialEq, Eq`).
   The §8 crate-edge matrix is normative for the owning leaf and the `cargo tree`
   acceptance.
   **Transitional registry:** until `MB.9.3`, `run_with` builds a stage only for a route with
   a registered builder and leaves `stages` empty (the 0.2.0 path serves it); `MB.9.3`
   deletes that fallback and a missing builder becomes `RunError::Boot` — no fallback backend
   is ever added.
2. **Per-route `[route.fastcgi]`, three keys.** `entry`, `listen`, `workers` (default
   `4`), matching `[route.static]` and `[route.ingress]`. `entry` is a `.php` file beneath
   `static.root` when both blocks are declared, checked through the injected
   `SocketProbe::check_entry` (`openat2(RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS)`, final
   component not a symlink, regular file, readable as the route/invoking uid; never
   `std::fs::canonicalize()`), whose syscall body lives in `crates/blastit/src/probe.rs` so
   `blastit-manifest` stays I/O-free. `ProductionProbe::check_entry` forks a **probe child**
   from single-threaded stage 0 (`crate::forking::fork(interpreter)`, the same flag
   `stage0` passes to `fork_dbproxy`) over a pipe. The child calls **only**
   `blastit_sys::drop_privileges(uid)` for its identity change (it `capset`s explicitly;
   inlining `setresgid`/`setresuid` is forbidden because the kernel clears caps only when all
   uids cross 0 and a rootless `init` never was 0), hardens, installs
   `SeccompRole::View { egress: false }`, opens the entry with `O_PATH|O_NOFOLLOW|O_CLOEXEC`
   (never a readable `O_RDONLY`, so a route-owned FIFO cannot block), `fstat`s `S_ISREG`, and
   calls `faccessat2(fd, "", R_OK, AT_EMPTY_PATH|AT_EACCESS)` through
   `libc::syscall(SYS_faccessat2, …)` (`rustix::fs::accessat` rejects `EMPTY_PATH`), replying
   one byte from the pinned set (`O`/`U`/`N`/`S`/`R`/`M`/`I`/`T`). The parent closes the write
   end, waits at most 5 s (`SocketProbeError::Timeout` + `SIGKILL`), maps a diagnostic byte to
   its variant, and treats any byte outside the set (or a missing/short reply) as boot-fatal,
   never "readable"; `install_filter(self.seccomp, …)` honours `BLASTIT_SECCOMP=0`. On
   `faccessat2` `ENOSYS` the probe refuses to boot (kernels < 5.8 are unsupported for
   `[route.fastcgi]`), with no weaker fallback. `DevProbe` needs no child.
   `check_fastcgi_entries` is called once per FastCGI route with that route's uid, and
   `stage0::run_dev` calls it with `ProbeUid::Dev`. The External promise is proven twice:
   `critical_fastcgi_dispatch::external_mode_never_calls_ensure_pool` (recording `PoolPort`
   fake, counter `0` with `external_executor = true`, `1` with `false`) and
   `critical_fastcgi_learn::external_pool_never_spawns_php_fpm` (no `php-fpm` child of the
   `dev` init after a request);
   `spa = true` with `[route.fastcgi]` is fatal (`FastCgiDeadDeclaration`); `env` is not a
   key — production renders `env[…]` from init's sanitized stage-1 environment minus
   `BLASTIT_*` and `HTTP_PROXY`/`http_proxy`, rewriting proxy `host=/proc/self/cwd` to
   `host=<runtime>/views/<uid>` and setting private `HOME`/`TMPDIR` (both
   `<runtime>/views/<uid>/tmp`, so `$HOME/.cache`/`.config` stay inside `open_basedir`; the
   private dir itself is deliberately not in `open_basedir`).
3. **The client computes every param.** `SCRIPT_FILENAME`/`DOCUMENT_ROOT` are pinned to
   declared values; the non-`HTTP_` set is a closed allowlist and `PHP_VALUE`,
   `PHP_ADMIN_VALUE`, `auto_prepend_file`, `allow_url_include`, `open_basedir`,
   `disable_functions`, `REDIRECT_STATUS` and `HTTP_PROXY` are never emitted;
   `HTTP_PROXY` is additionally unreachable from a `Proxy:` header (httpoxy) and stripped
   from `env[…]`. A query string is data and never becomes argv, INI text or a script path,
   so CVE-2012-1823, CVE-2024-4577/8926 and CVE-2024-8927 (the `cgi.force_redirect`/
   `REDIRECT_STATUS` collision) have no path even on an unpatched runtime. A single header
   value over 8192 bytes is dropped (`Param { Dropped }`, `200`), never a `400` and never a
   silent loss. The generated pool adds `php_admin_value` backstops (`allow_url_include=0`,
   empty `auto_prepend_file`, empty `user_ini.filename`,
   `open_basedir=<manifest_dir>:<runtime>/views/<uid>/tmp`, `cgi.fix_pathinfo=0`) so a
   `.user.ini` under the web root cannot re-open the chain while the param stream is the
   harness's fixed set. `P4b` in the e2e suite is a positive control (a `PHP_VALUE`-accepting
   pool executes the chain, canary present) and is excluded from the canary-absent assertion.
4. **A pure codec crate and a scheduler client.** `crates/blastit-fastcgi` (records,
   name-value codec, param lockdown, CGI head parser) does no I/O and has zero production
   dependencies; it therefore returns **codec-local** errors (`LockdownError`, `CgiError`,
   `RecordError`), never `Refusal`/`UpstreamError`, and `MB.3.6.2.1` maps them at the
   adapter boundary (`UnknownParam → Refusal::UnknownParam` 500, `ParamsTooLarge |
   DuplicateParam → Refusal::ParamsTooLarge` 400, `Malformed → Refusal::MalformedPath` 400,
   `CgiError::* | RecordError::* → UpstreamError::Fault` 502). `FastCgiPaths` is defined in
   `blastit-fastcgi::lockdown` (std types, no I/O) and built by `FastCgiBuilder`; `blastit-scheduler` owns connect/spawn/pump/account. One `AF_UNIX`
   connection and one `BEGIN_REQUEST` per request, `requestId = 1`, `keepConn = 0`;
   response `Content-Length` is stripped, exactly four proxy-control response headers
   (`X-Accel-Redirect`, `X-Sendfile`, `X-Lighttpd-Send-File`, `X-Rewrite-URL`) are refused
   as `Fault` (`502` before head) in both `blastit-fastcgi::cgi` and the shared
   `ResponseSink::head`, every other `X-Accel-*` name is dropped without a fault (so
   `X-Accel-Buffering: no` stays `200`), a CGI header **name** that is not an RFC 7230
   `tchar` token (empty, NUL/CTL, space, `:`, obs-fold) is `Fault` before any head is sent,
   and
   `STDERR` caps both line length and its partial-line accumulator at 2048 bytes.
   `ABORT_REQUEST` is best-effort on cancel and socket close is the real signal;
   `END_REQUEST.appStatus` and `reserved[3]` are ignored.
5. **For a route with `[route.fastcgi]`, a `.php`/`.phar` path under `root` is a final
   `404` (`Refusal::EntryScript`) before method (`405`), body (`413`) and `Content-Type`
   checks.** Only `entry` executes, and `entry` is never served as a file.
6. **Pool ownership reuses the view role with a pinned-fd/inode socket lifecycle.** No new
   role, uid or supervisor (ADR-0001, ADR-0007): init spawns `php-fpm -F -y` per route as
   `UID_BASE+1000+i` in its own process group (child `setpgid(0, 0)` **and** parent
   `setpgid(child_pid, child_pid)` right after the fork, `EACCES` ignored, so `pgid` exists
   before any `kill(-pgid)`), forked through **`crate::forking::fork(true)`** (the GIL is held
   for the whole `Supervisor::run(py)` loop, so `PyOS_BeforeFork` and
   `PyOS_AfterFork_{Parent,Child}` are the matching halves; same pattern as
   `zygote::fork_view`, never a raw `libc::fork`) and then
   `child::ChildIo { keep: vec![], stdout, stderr }`
   so `close_fds_except` leaves only fd 0/1/2 (`ctl.chan`, `parentfd`, the events pipes and
   every `BLASTIT_CHECK_*` fd are closed before `execve`, in `S0`'s `php-fpm -t` child and in
   the `S3` pool master alike), with `SeccompRole::View { egress }`,
   `RLIMIT_NPROC = workers + 2`, `clear_env = yes`, `pm.max_children = workers`,
   `pm.max_requests = max(BLASTIT_VIEW_MAX_REQUESTS, 500)` (a pool never means "never", so a
   future param-wall regression cannot persist for the process lifetime),
   `security.limit_extensions = .php` and `request_terminate_timeout`. The conf is rendered
   by `#[doc(hidden)] pub fn render_pool_conf(&PoolConf<'_>) -> String` with
   `#[doc(hidden)] pub struct PoolConf<'a>` (std types only: paths, uids, workers,
   max_requests, timeout, listen, entry, env), so the integration test can call it while
   `Parts`/`Supervisor` stay `pub(crate)`. `php-fpm` binds its
   configured absolute `listen` path
   itself; Blastit performs **no caller-path-resolved** `chown`/`chmod`/`unlink`/`open`/
   `connect` — every operation is on `parentfd` (`*at` with `AT_SYMLINK_NOFOLLOW`), or the
   one descriptor-derived `chmod("/proc/self/fd/<sockfd>")` on an `O_PATH` fd whose inode was
   just verified. At boot,
   **before any route process exists**, init checks ancestors from `/` to `manifest_dir`
   (sticky-bit rule per `ARCHITECTURE.md` §5.3), opens
   `manifest_dirfd = open(manifest_dir, O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)` and
   requires `fstat`/`lstat` inode equality, `S_ISDIR`, `!S_ISLNK` and an absolute
   `/proc/self/fd` readlink, resolves
   `parentfd` (`O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC`) via
   `openat2(RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS)` (creating only an
   absent final leaf `0555`, per-segment `O_NOFOLLOW` fallback on `ENOSYS`), requires
   `manifest_dir` and every segment below it through `parentfd` to be non-symlink, not
   writable by any route uid/gid, and
   unique per route (not `manifest_dir`, `static.root`/beneath, or an `entry` ancestor),
   freezes `parentfd` `init_uid:init_gid 0555` (any `EPERM` exits `1` naming
   `CAP_CHOWN`/`CAP_FOWNER`), cleans stale sockets, and hands `parentfd` across the
   stage-0→stage-1 `execve` **only** as the internal argv entry
   `--fastcgi-parent-fd=<route>:<n>` (the `--dev-obs-fd` pattern: `FD_CLOEXEC` cleared for
   that `execve`, re-set in stage 1 before any fork; `cli::parse` accepts the form only with
   `--stage=1` and never lists it in `--help`; never `SCM_RIGHTS` and never a path re-open).
   Stage 1 validates the list **before** wrapping any descriptor (unique route, unique fd,
   `fd >= 3`, not `config_fd`/dbproxy fd, `S_ISDIR`), then wraps each in `OwnedFd` and shares
   one `Arc<OwnedFd>` per route between `SchedulerConfig.pinned_sockets` and **init's
   per-route pool state, which keeps it for the process lifetime** (`S1`–`S7`/`U1` run on it;
   it closes only when `init` exits), while `fork_scheduler`'s `ChildIo.keep` gives the
   scheduler its own copy and `http`/`dbproxy`/`view`/`pool` drop theirs. `PinnedSocket`
   (`BuildCtx.socket`) is built from it. The argv validation is proven by
   `crates/blastit/tests/critical_fastcgi_parent_fd.rs` (one row per rule, unprivileged:
   crafted argv → `exit 1` + the exact message, plus a positive case asserting the fd is
   `FD_CLOEXEC` before the first fork). One spawn transition (`php-fastcgi`
   §4.8.3): `php-fpm -t` as `route_uid` before opening the window → reap the previous pgid to
   `ECHILD` (bounded; not `ECHILD` → `U1`, the window never opens) → `unlinkat` →
   `fchownat(parentfd, "", route_uid, route_gid, AT_EMPTY_PATH)` + `fchmodat(".", 0755, 0)`
   → spawn → `fstatat(sock, AT_SYMLINK_NOFOLLOW)` (`S_ISSOCK`, `route_uid:route_gid`,
   `st_nlink == 1`, **and the inode must be held by a task whose `/proc/<pid>/stat`
   `pgrp == master_pid`**, record `(sock_dev, sock_ino)`) → `fchownat(sock, scheduler_uid,
   scheduler_gid, AT_SYMLINK_NOFOLLOW)` + `fchmodat(sock, 0600, AT_SYMLINK_NOFOLLOW)`
   (`O_PATH|O_NOFOLLOW` + inode check + `/proc/self/fd` `chmod` on `EOPNOTSUPP`) →
   **restore owner and mode** `init_uid:init_gid 0555` + post-freeze `fstatat` re-verify
   **plus a re-run of the pgid-membership check** →
   `Ctl::PoolReady { route, sock_dev, sock_ino }` over private `ctl.chan`. Every failure
   runs Unwind `U1` (`kill(-pgid, SIGKILL)`, **freeze first** `init_uid:init_gid 0555`, then
   `unlinkat` — `CAP_DAC_OVERRIDE` lets init unlink in `0555`, and freezing first leaves no
   `0755 route_uid` gap to rebind in — no `PoolReady`); a failed group kill never skips the
   freeze/unlink, and a `setsid()` child is bounded by `RLIMIT_NPROC`, cannot write `0555`
   `parentfd`, and **cannot be published**: its socket's inode is not held by a task in the
   new pgid, so `S4`/`S6` fail it. The residual is same-uid fd injection/`ptrace` into a
   new-pgid task (same-route persistence/DoS only, never cross-route access). The scheduler
   connects only via
   `/proc/self/fd/<parentfd>/<name>` after verifying `(parent_dev, parent_ino)`,
   `(sock_dev, sock_ino)`, `S_ISSOCK`, `expected_uid`, `0600`, and `SO_PEERCRED` against the
   exact accepted set — production `{route_uid:route_gid}` (`peer_uid = route_uid`,
   `peer_gid = route_gid`; the **listening** master is the socket creator, and
   `SO_PEERCRED` reports the creator, not the accepting worker), `dev`/`check`
   `{invoking_uid:invoking_gid}`; `scheduler_uid` is rejected and there is no gVisor
   tolerance. `dev`/`check` (`SchedulerConfig.pool_spawn =
   External`) never spawn pools and pin `parentfd` via `DevProbe::check`, which returns the
   opened `OwnedFd` (`check_fastcgi_sockets -> Vec<(usize, OwnedFd)>`); `check`'s child
   `dev` receives it as `BLASTIT_CHECK_POOL_PARENT_FD`, read by `Config::for_dev()` before
   `BLASTIT_*` stripping, and re-validates it (`fstat`/`fstatat`) without a second
   `openat2`. After boot no code path re-opens the parent segment.
7. **The scheduler gains one `AF_UNIX` socket capability.** In `blastit-sys::seccomp`,
   `SeccompRole::Scheduler` allows `socket(args[0] == AF_UNIX)` (`AF_INET`, `AF_INET6`
   and `AF_NETLINK` stay `EPERM`) while a `Scheduler`-only rule denies `SYS_bind` and
   `SYS_listen` (`EPERM`, leaving `bind`/`listen` allowed for `SeccompRole::View`);
   classic BPF cannot filter `connect(2)` paths, so `0600` inside pinned `parentfd` is the
   path wall.
8. **The retrofit is epic `MB.9`, and it lands before `MB.4`.** The contract
   (`MB.9.1`), the static port (`MB.9.2`), the WSGI port (`MB.9.3`) and the shared
   conformance table gated in `make check` (`MB.9.4`) must complete **before `MB.4` (Node)
   is implemented**; FastCGI implements the same trait inside `MB.3` (`MB.3.6` adds its
   conformance drivers to `Makefile`). Define *static-only* as a `[route.static]` route
   without `[route.fastcgi]` and with either `spa = true` or no `app`. `app` is optional
   iff `Manifest::can_boot_without_app()` (`!routes.is_empty() && all(fastcgi || static)`)
   holds; `Manifest::is_all_static()` keeps its 0.2.0 meaning (`app.is_none() &&
   !routes.is_empty() && all(static && !fastcgi)`); `check` runs all-static first, then
   external-executor (`app.is_none() && can_boot_without_app()` via `<CheckRoot>/dev`
   handoff, closing `check_sql_obs_fd` in stage 0 outside `dbproxy`; the child `dev` is
   spawned as `Host::stage1()`'s `Reexec` — `/proc/self/exe` for a standalone binary,
   `<python> -I -m blastit` for a wheel — plus `dev --port 8080`, never `<current_exe> dev`;
   `check_pool_parent_fds`/`check_sql_obs_fd` are absent from `Config::to_json`, so the
   configuration document is not a second handoff source), then pytest
   (preserving `critical_static_cli`'s app-less `MIXED` `"no interpreter to run pytest
   on"`). Protected suites `critical_static_assets.rs`, `critical_static_resolve.rs`,
   `critical_rewrite.rs`, `critical_ingress_enforcement.rs`, `critical_static_manifest.rs`
   and `critical_static_cli.rs` pass unmodified. The transitional registry rule is part of
   this decision: `MB.9.2.1`/`MB.9.2.2` keep `stages` empty for a route with no registered
   builder (the 0.2.0 path serves it, so `cargo test -p blastit` passes), and `MB.9.3`
   deletes that fallback so a missing builder is `RunError::Boot`.

**Amends.**
- **ADR-0007 decision 1:** Decision 7 supersedes decision 1's "no new socket/filter"
  sentence for `SeccompRole::Scheduler` only (`socket(AF_UNIX)` allowed, `bind`/`listen`
  `EPERM`).
- **ADR-0007 decision 4:** Decision 5 adds the `.php`/`.phar` `404` (`EntryScript`)
  exception on FastCGI routes, and decision 4's forward target on a static miss (when
  `spa` is unset) is **the route's executor** (`[route.fastcgi]` when declared, otherwise
  the manifest's `app`).
- **ADR-0007 decision 6:** "`app` is optional iff every route is static-only or has its own
  executor" (`can_boot_without_app()`), and "A static route that can never reach an
  executor — `static_policy.is_some() && fastcgi.is_none() && (spa || app.is_none())` — may
  not declare `[route.ingress]`, `prefork`, a grant or inline `groups`."
- `ARCHITECTURE.md` §7.3, `docs/security.md`, `docs/design/static-and-spa.md` §5.1 and a
  1-line amendment banner on `ADR-0007` are synced by `MB.3.8`.

## Consequences

- **Positive.** A new backend is one `Executor` variant, one `Upstream` implementation and
  one registered `BackendBuilder` — not a new dispatch arm, observation shape or error
  enum. Backend contract, refusal status, body-cap and observation mapping cannot drift
  across static, WSGI and FastCGI (`conformance::cases()`), while static caching/conditional
  semantics remain pinned by `critical_static_*`. The param contract is the sole producer of
  FastCGI params, closing `php2shell` before any byte reaches the pool. One internal argv
  entry (`--fastcgi-parent-fd`) and one `BLASTIT_CHECK_*` variable are the only new
  plumbing, and no app-controlled process (`php-fpm -t`, the pool master, a view) ever
  inherits `ctl.chan` or `parentfd`. The view/zygote path's `ChildIo.keep` omits
  `pinned_socket_fds()`, and
  `critical_fastcgi_parent_fd::view_child_holds_neither_parentfd_nor_ctl_chan` (mixed
  FastCGI + WSGI manifest) asserts `/proc/<view_pid>/fd` holds neither. Local stays
  zero-`sudo`; production stays non-root per
  route with zero route-writable socket parents outside the unserved bind window.
- **Trade-offs.** `RouteInfo` retains `static_policy`/`static_forward` for protected suite
  value assertions while dispatch branches on them are deleted. `raw` bodies are buffered
  for FastCGI (`CONTENT_LENGTH` drives `php://input`, bounded by `BLASTIT_MAX_WORKERS ×
  BLASTIT_MAX_BODY_BYTES`). One connection per request (`keepConn = 0`). The scheduler
  gains `socket(AF_UNIX)` (with `SYS_bind`/`SYS_listen` `EPERM`) and a guarded per-request
  session task.
