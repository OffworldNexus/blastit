# MB.3.3 RESULT — design `php-fastcgi.md` + ADR-0008 (`architect` resolution, Escalation #9)

## Summary

Resolved the 3-round critic deadlock on `docs/design/php-fastcgi.md` and
`docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md` (companion ID-by-ID matrix
in `docs/design/php-fastcgi.resolve-architect.md`):

1. **Pool socket lifecycle, fd/inode identity, seccomp & untrusted pool bytes (`security`):**
   Pinned the normative `B1`–`B3` / `S0`–`S7` / `U1` / `C1` syscall table (§4.8.3 &
   ADR-0008 §6), server-agnostic `PinnedSocket` (`BuildCtx.socket`) carrying `parentfd` +
   `(parent_dev, parent_ino)` + `bound_inode` (`Ctl::PoolReady { route, sock_dev, sock_ino }`
   over private `ctl.chan`), `SeccompRole::Scheduler` `socket(AF_UNIX)` with
   `SYS_bind`/`SYS_listen` `EPERM`, `String::from_utf8_lossy` + `SessionDoneGuard` `Drop`
   slot release on panic, proxy-control response header denial (`X-Accel-*`, `X-Sendfile`,
   `X-Lighttpd-Send-File`, `X-Rewrite-URL` → `Fault` `502` in `cgi` and `ResponseSink`),
   hard `2048`-byte STDERR accumulator cap, `check_sql_obs_fd` closed in stage 0 outside
   `dbproxy`, and `HTTP_PROXY` strip + private per-route `TMPDIR` + `host=<runtime>/views/<uid>`.
2. **Surface & contract coherence (`consistency`):** Reconciled `[route.fastcgi]` across
   `php.md`, `manifest.md` (3 exact edits in §4.1) and `php-fastcgi.md`, made
   `blastit_manifest::Executor` the single source of executor kinds, added
   `WireRequest.executor_ran` so pre-executor `413` fires `missing_ingress` while keeping
   `exercised: 0`, unified numeric plan IDs and the executable §8.1/§8.2 checklists, and
   recorded truthful Round 1–3 history in §9 (on-disk `*.review-*.md` files remain `FAIL`
   until Round-4 critics run).
3. **Oracles & protected suites (`testability`):** Kept `critical_static_cli` `MIXED` on
   the pytest branch (`can_boot_without_app() == false`), pinned whole-slice
   `assert_eq!(IMPLEMENTATIONS, …)` + 5-binary `Makefile` `rs-test-conformance` gate,
   assigned one `Owner file::fn` per §4.7 row (`24` client `E` probes, row 18 `—` internal
   `500`, row 25 `504` + `ABORT_REQUEST` fake-peer oracle), and added the active decoy
   `/sentinel/fpm.sock` `0`-`accept` oracle + `route_uid` `chmod`/`O_CREAT`/`unlink`
   negatives.
4. **Buildability (`implementability`):** Threaded `PinnedSocket` and shared
   `Arc<RouteState>` (`SchedulerConfig.pool_spawn`), added `<CheckRoot>/dev`
   (`BLASTIT_CHECK_DEV_ROOT`) socket-dir handoff, split all 5 oversized leaves to ≤ 7 files
   / ≤ 550 lines, added `MB.3.5.1` to `MB.9.1.1` `depends_on` for `Executor`, and pinned
   the two-step unlocked-then-`--locked` rule for lock-owning leaves.

## Acceptance Evidence

```console
$ test -f docs/design/php-fastcgi.md && test -f docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md && echo "design + ADR exist"
design + ADR exist
$ for k in 'route.fastcgi' 'BEGIN_REQUEST' 'requestId' 'SCRIPT_FILENAME' 'PATH_INFO' 'DOCUMENT_ROOT' 'REDIRECT_STATUS' 'PHP_ADMIN_VALUE' 'httpoxy' 'auto_prepend_file' 'public/index.php' 'route.static' 'ABORT_REQUEST'; do grep -q -- "$k" docs/design/php-fastcgi.md || echo "missing: $k"; done
(no output — every key present)
$ grep -q 'CVE-2012-1823' docs/design/php-fastcgi.md && grep -q 'CVE-2024-4577' docs/design/php-fastcgi.md && echo "CVEs present"
CVEs present
$ test "$(grep -c '^|' docs/design/php-fastcgi.md)" -ge 30 && echo "param contract, refusal matrix and CVE table are tables"
param contract, refusal matrix and CVE table are tables        # 329 table lines
$ grep -q 'trait Upstream' docs/design/php-fastcgi.md && echo "one contract trait pinned"
one contract trait pinned
$ grep -q 'Arc<dyn Upstream>' docs/design/php-fastcgi.md && echo "per-route injected handle pinned"
per-route injected handle pinned
$ grep -qi 'conformance' docs/design/php-fastcgi.md && echo "shared conformance table pinned"
shared conformance table pinned
$ grep -q 'never `match`es on backend kind' docs/design/php-fastcgi.md && echo "no backend-kind dispatch pinned"
no backend-kind dispatch pinned
$ grep -q 'MB.9' docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md && grep -q 'before .*MB.4' docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md && echo "ADR-0008 records the split and its order"
ADR-0008 records the split and its order
$ grep -q 'MB.3.7' docs/design/php-fastcgi.md && grep -q 'e2e/php2shell.py' docs/design/php-fastcgi.md && echo "e2e refs pinned"
e2e refs pinned
$ grep -n 'php-fastcgi' docs/design/README.md && grep -n 'ADR-0008' docs/decisions/README.md
9:| [php-fastcgi.md](php-fastcgi.md) | Blastit 0.2.1 Technical Design: FastCGI, Param Lockdown and the Injected Upstream Contract | In review | MB.3.3 |
14:| [ADR-0008](ADR-0008-fastcgi-backend-and-param-lockdown.md) | Injected `Upstream` Contract and FastCGI Param Lockdown | Proposed | MB.3.3 |
$ make docs-check && make guard && make critical-guard BASE=HEAD NODE=MB.3.3 && uv run --no-project tools/plan.py validate
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
guard: OK (28 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 25 changed file(s)
  no change to a protected critical test
plan OK
```

On-disk `docs/design/php-fastcgi.review-*.md` files remain at their pre-architect
`VERDICT: FAIL` state awaiting `lead`/`conductor`'s fresh Round-4 critic gate.

## Files Touched

- `docs/design/php-fastcgi.md`
- `docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md`
- `docs/design/php-fastcgi.resolve-architect.md`
- `plan/nodes/MB/MB.3/MB.3.3/RESULT.md`

## Review Resolution

Mirrored verbatim in `docs/design/php-fastcgi.md` §9 and `docs/design/php-fastcgi.resolve-architect.md`:
all BLOCKER, MAJOR and MINOR findings across `security` (R1–R3), `consistency` (R1–R2),
`testability` (R1–R3) and `implementability` (R1–R3) are resolved.

## Follow-ups

1. `lead` / `conductor`: spawn the fresh-context Round-4 critic pass (`security`,
   `implementability`, `testability`, `consistency`) on `docs/design/php-fastcgi.md` +
   `ADR-0008` to overwrite the four `docs/design/php-fastcgi.review-*.md` files with
   `VERDICT: PASS`.
2. `planner` / `lead`: before dispatching `MB.3.4+` / `MB.9.*`, apply the leaf splits,
   `depends_on` edges (`MB.9.1.1` → `MB.3.5.1`) and `paths` updates in
   `docs/design/php-fastcgi.md` §8.1 and the brief re-syncs in §8.2.

## Round-4 `security` designer pass (attempt 2)

Fixed the two MAJORs and ten MINORs from `docs/design/php-fastcgi.review-security.md` §2
without touching the four review files, any `crates/**` file, or the accepted
`docs/frameworks/php.md` page:

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-A | §4.2.1, §4.5.2, §4.8.1 (new handoff block), §4.8.3 `B3`/`C1`, §1.3, §4.10.3, §8.2, ADR-0008 d6 | `parentfd` crosses the stage-0→stage-1 `execve` only as internal argv `--fastcgi-parent-fd=<route>:<n>` (`keep` + `set_cloexec(false)`, re-set `FD_CLOEXEC` in stage 1, `ChildIo.keep` extended by `pinned_socket_fds()`); `check` passes `BLASTIT_CHECK_POOL_PARENT_FD`, read by `Config::for_dev()` before `BLASTIT_*` stripping; negative rule: no post-boot `openat*` on the parent segment |
| MAJOR-B | §4.8.2 Command, §4.8.3 `S0`/`S3`/`S7`, §7.2, ADR-0008 d6 | `S0`'s `php-fpm -t` child and the `S3` master fork through `ChildIo { keep: vec![], stdout, stderr }` → only fd 0/1/2; `S7`'s "held only by init and scheduler" is true by construction |
| MINOR-19 | §4.2.1, §4.5.1, §4.5.2, §4.11, §5.1, §7.2, §8.2, ADR-0008 d4 | Exactly four control names refused; other `X-Accel-*` dropped (`X-Accel-Buffering: no` → `200`); conformance `benign_x_accel_header_dropped` |
| MINOR-20 | §4.2.1, §4.5.2, §4.8.3 `C1`, ADR-0008 d6 | `expected_uid` = socket owner; `peer_uid`/`peer_gid` = accepting worker (`route_uid:route_gid` prod, `invoking_uid` dev) |
| MINOR-21 | §4.8.2 Command, §4.8.3 `S3`/`U1`, ADR-0008 d6 | Parent-side `setpgid(pid,pid)` (`EACCES` ignored); failed group kill never skips `unlinkat` + re-freeze |
| MINOR-22 | §4.8.2 Pool file, §7.2 `R11`, ADR-0008 d2/d6 | `HOME == TMPDIR == <runtime>/views/<uid>/tmp`, inside `open_basedir` |
| MINOR-23 | §4.1 `R11`, §4.8.2, §7.2 `R11`, ADR-0008 d6 | `pm.max_requests = max(BLASTIT_VIEW_MAX_REQUESTS, 500)`; pool `0` ≠ "never" |
| MINOR-24 | §4.8.3 `B1`/`B2`, ADR-0008 d6 | `manifest_dirfd = open(..., O_NOFOLLOW)` + `fstat`/`lstat` inode equality + `/proc/self/fd` readlink; claim scoped below `manifest_dir` |
| MINOR-25 | §4.7 rows 10–11, §5.2, ADR-0008 d3 | Row 10 = CVE-2024-8926; CVE-2024-8927 moved to row 11 (`REDIRECT_STATUS`/`force_redirect`) |
| MINOR-26 | §5.2, §7.3, ADR-0008 d3 | `P4b` is a positive control (canary **present**), excluded from the canary-absent assertion |
| MINOR-27 | §4.6.2, §7.2 `R7` | Value `> 8192` bytes dropped (`200`, `Param { Dropped }`), test `over_long_header_value_is_dropped_not_refused` |
| MINOR-28 | §4.8.2, §7.2 `R2,R3,R20`, ADR-0008 d6 | Named `critical_fastcgi_pool::setsid_escapee_after_retire_cannot_rebind_and_new_pool_has_new_inode` + `e2e/rootless/run.py::check_fastcgi_setsid_escapee_contained` |

`docs/design/php-fastcgi.md` §9 now carries a "Round 4 (`security`, fresh context)" history
entry and a 12-row disposition table. `docs/design/php-fastcgi.resolve-architect.md` gained
§6 (Round-4 addendum) and two new refused-trade invariants (one descriptor path / no
app-controlled control fd).

### Round-4 acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-5 `security` designer pass (attempt 2, continued)

Fixed MAJOR-A (Round-4 residual), MAJOR-C and MINOR-29/30 from
`docs/design/php-fastcgi.review-security.md` §2. `MINOR-31..37` are recorded as open in §9
and are **not** claimed fixed.

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-A residual | §4.1 `SocketProbe`, §4.8.1 steps 1–4 + `check` paragraph, §4.8.2 Who spawns, §8 (`MB.3.5.2`, `MB.3.6.2`, `MB.3.6.3`, `MB.3.6.4.2`), §8.2, ADR d6 | `init` keeps one `Arc<OwnedFd>` per route for the process lifetime (`S1`–`S7`/`U1` run on it; deleted "closes when `fork_scheduler` returns"); `cli.rs` owned by `MB.3.6.3` (internal `--fastcgi-parent-fd` only with `--stage=1`, invisible in `--help`; `zygote.rs` dropped); validate unique route/fd/`fd>=3`/`S_ISDIR` **before** `OwnedFd`; `SocketProbe::check -> Result<Option<OwnedFd>, …>` + `check_fastcgi_sockets -> Vec<(usize, OwnedFd)>` |
| MAJOR-C | §4.8.2 Command, §4.8.3 `S1`/`S2`/`S4`/`S6`/`U1`, §7.2, ADR d6 | `S1` reaps the old pgid to `ECHILD`; `S4`/`S6` require the socket inode to be held by a task with `pgrp == master_pid`; `U1` freezes before `unlinkat`; oracle asserts `PoolReady.sock_ino` **equals** the new master's inode and an escapee rebind yields no `PoolReady`/`U1`/`503`/`0` accepts |
| MINOR-29 | §4.5.2, §4.8.3 preamble, ADR d6, resolve §5.3 | "no caller-path-resolved `chown`/`chmod`/`unlink`/`open`/`connect`; `chmod("/proc/self/fd/<sockfd>")` is descriptor-derived" |
| MINOR-30 | §4.2.1, §4.5.2, §4.8.3 `C1`, §8/§8.2, ADR d6 | exact `SO_PEERCRED` sets `{route_uid:route_gid}` / `{invoking_uid:invoking_gid}`; no gVisor tolerance; `scheduler_uid` rejected; `foreign_uid_peer_is_fault_502` |

### Round-5 acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.23s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-5 follow-up: MINOR-31..37 (designer pass)

All seven previously-open Round-5 MINORs are now fixed; §9 has a second 7-row table and the
"open" note is gone.

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MINOR-31 | §1.3, §4.8.1, §4.10.3, ADR d8 | child `dev` = `Host::stage1()`'s `Reexec` (`/proc/self/exe` or `<python> -I -m blastit`) + `dev --port 8080`; "while dbproxy starts" deleted |
| MINOR-32 | §4.8.1, §7.2 `R17/R18`, ADR d8 | `Config::to_json` (manual allowlist) never inserts `check_pool_parent_fds`/`check_sql_obs_fd`; test `config_document_carries_no_check_fd` |
| MINOR-33 | §4.8.3 `S0`, §7.2 `R2,R3,R20`, ADR d6 | oracle is negative-only: `fcntl(3, F_GETFD)`/`read(3, …)` → `EBADF`, no parent/socket fd target; `{0,1,2}` not asserted |
| MINOR-34 | §4.5.2, §5.1, ADR d6 | claim narrowed to `B1` sticky-bit above `manifest_dir` + `B2` below; route-owned `manifest_dir` = self-DoS `503` |
| MINOR-35 | §4.5.1, §4.5.2, §4.7 row 24, §7.2, ADR d4 | non-`tchar` header name is `Fault`; four names compared on the token name; `cgi::tests::non_token_header_name_is_fault` |
| MINOR-36 | §4.8.2 Who spawns, §4.8.3 `S3`, ADR d6 | `spawn_pool` forks via `crate::forking::fork(true)` (GIL held in `Supervisor::run(py)`), the `zygote::fork_view` pattern; no raw `libc::fork` |
| MINOR-37 | §4.6.2, §4.7 row 17 | `> 110` `HTTP_*` with a body / `> 112` without (18-param fixed set), not `> 114` |

### Follow-up acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-4 `implementability` pass (M1–M12 + one-liners)

All twelve MAJORs and the six one-line findings are pinned; nothing remains open from this
lens.

| Finding | Sections changed | What is now pinned |
|---|---|---|
| M1 | §4.2.1 `RouteSpec`, §8 `MB.9.2.1.1` | `RouteSpec.line` from `SchedulerConfig.route_lines` (`Manifest::route_lines()`) |
| M2 | §4.2, §8 `MB.9.2.1.2` | `run_with -> Result<(), RunError>` + `SchedulerError::Boot` in `error.rs`; `run` unchanged |
| M3 | §4.2.1 `PinnedSocket`, §8 `MB.9.1.1` | `PinnedSocket::new` + accessors, `BadName`/`Poisoned`, pure |
| M4 | §4.2.1 `BuildCtx`, §4.8.1, §8 | contract-owned `external_executor`; `ensure_ready` early return |
| M5 | §4.2.1 `RouteState`, §8 | `acquire(self: &Arc<Self>) -> RoutePermit` (`Send`, owns `Arc`) |
| M6 | §4.1, §8 `MB.3.5.2`, §8.1 | probes move to `crates/blastit/src/probe.rs`; manifest stays `rustix`-free |
| M7 | §8 `MB.3.6.1.1`, §8.1 | `tests/common/mod.rs` + the `Ctl::Pool*` arm |
| M8 | §4.8.1 | `peer_gid = invoking_gid` |
| M9 | §8 `MB.3.6.3.1`/`.2`, §8.1, §8.2 | `cli.rs`/`dev_learn.rs` added, `zygote.rs` dropped, `DevRoot::adopt` at `dev_learn.rs:180` |
| M10 | §8, §8.1, §8.2 | `MB.3.6.2` → `.1`/`.2`; `MB.3.6.3` → `.1`/`.2`; Order/§8.1/§8.2 synced; fallback splits stated |
| M11 | §4.11 | `Capability(u32)` + `HeadSpec`/`OutcomeKind`/`PeerScript`/`CancelAt` |
| M12 | §4.2, §8 `MB.9.2.2`/`MB.9.3` | transitional `stages.is_empty()` path; `MB.9.3` deletes it |
| MINORs | §4.1, §4.2, §4.8.2, §8 | `SocketProbeError`; `ProductionProbe -> Ok(None)`; full `ChildIo` literal; `builtin_registry() -> Result<…>`; fake helper path; §8.1 redundancy removed |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.20s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-5 `implementability` pass (MAJOR-1..4 + folded MINOR-5/11)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §4.2.1 `PinnedSocket`, §4.8.1 step 2, §8 `MB.9.1.1`/`MB.3.6.3.1`, ADR d1 | `new(parent: Arc<OwnedFd>, …)`, `parent_fd()` via `as_fd()`; same `Arc` as init (no `try_unwrap`/`dup`) |
| MAJOR-2 | §4.2 trait + snippet + `Session` block, §4.2.1 (`BodyStream`, `SessionPort`, `Session`/`SessionMsg`), §4.5.2, ADR d1 | owned `BodyStream<'static>` + `Box<dyn ResponseSink + Send>`; `Scheduler::session` `tokio::spawn`; one `Done` via `SessionDoneGuard`; `SessionPort` WSGI-only |
| MAJOR-3 | §8 `MB.3.6.1.1`, §8.1 item 2, §8.2 | `supervise.rs` in `MB.3.6.1.1` (6 files) + `cargo test -p blastit --locked` |
| MAJOR-4 | §8 `MB.3.5.2`, §8.1 item 2, §8.2 | `lib.rs` single owner `MB.3.5.2`: `pub mod probe;` + `pub mod supervise;` |
| MINOR-5/11 | §4.2 | written-out `run` expression; `#[error("{0}")] Boot` + `BootError: Error` |

MINOR-6..10 remain open and are recorded in §9.

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-6 `implementability` pass (MAJOR-A..D + one-liners)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-A | §4.2, §4.2.1, §8 (`MB.3.5.1`, `MB.9.2.1.1`), ADR d1 | `Executor`/`FastCgiPolicy` derive `Debug, Clone, PartialEq, Eq` (no `Copy`); `PoolSpawn { Managed, External }` (renamed) |
| MAJOR-B | §4.2.1 (`PoolPort`, `BuildCtx`), §4.8.1, ADR d1 | `PoolPort::ensure_pool(route)` + `CtlPoolPort` from `run_with`; `SessionPort` WSGI-only; first request waits for `PoolReady` |
| MAJOR-C | §4.2, §4.2.1, §4.4, ADR d1 | one `SessionMsg::Done { id, result }`, bounded sender cap 1, receiver in `self.sessions`, `try_acquire` before `Scheduler::session(..., permit)` |
| MAJOR-D | §4.8.3, §8 (`MB.3.6.3.2`), §8.1, §8.2 | inline `#[doc(hidden)] pub mod pool_test` (`PoolHarness`/`PoolReady`), unprivileged subset, `lib.rs` stays `MB.3.5.2` |
| One-liners | §4.1, §4.2, §4.2.1, §8, §8.1, §8.2 | lifetime-free `BodyStream`; `MB.3.8` 7 entries; `pinned_socket_fds()` → `MB.9.2.1.1`; probes → `MB.3.5.2`; `on_ctl` → `MB.3.6.2.1`; `foreign_uid_peer_is_fault_502` → `.2`; fallback splits `.1a`/`.1b`, `.2a`/`.2b` |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-7 `implementability` pass (MAJOR-1..4)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §4.2, §4.2.1, §4.5.2, ADR d1 | contract `BodySource` + `CancelToken(Arc<AtomicBool>)`; tokio `BodyStream` scheduler-side; `exchange` takes `Box<dyn BodySource + Send>`; `blastit-backend` tokio-free |
| MAJOR-2 | §4.4, §4.5.2, §4.8.1 | pool start is `PoolPort::ensure_pool`; every remaining `SessionPort` mention is WSGI-only |
| MAJOR-3 | §8 (`MB.9.2.1.1`), §8.1, §8.2 | `crates/blastit/src/stage1.rs` added (7 paths) + `cargo test -p blastit --locked` |
| MAJOR-4 | §8, §8.1, §8.2 | unconditional `MB.9.2.1.2`+`.3` and `MB.3.6.3.2`+`.3`; mapping/Order/§8.1/§8.2 updated |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-8 `implementability` pass (MAJOR-1..3 + MINOR-1..6)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §4.2, §8 (`MB.9.2.1.2`/`.3`, `MB.9.2.2`, `MB.9.3`, `MB.3.6.2.1`), §8.1, §8.2, ADR d1 | `builtin_registry()` defined in `scheduler.rs` (`.2`), re-exported by `.3`; each registering leaf appends in `scheduler.rs` |
| MAJOR-2 | §4.2.1, §4.5.2, §4.8.1, §8, ADR d1 | `RouteState::new(pool_spawn == External, workers)`: Ready in `External`, Busy in `Managed` until `PoolReady` |
| MAJOR-3 | §4.2.1 (`Observation`, `ObservationKind`), ADR d1 | `#[serde(tag = "kind", rename_all = "snake_case")]`; exact-line tests named |
| MINOR-1..6 | §8, §8.1, §8.2 | `.1/.2/.3` labels; corrected `stage1.rs` reason; `SchedulerConfig.{route_lines,pinned_sockets,pool_spawn}`; `http_obs: Option<PathBuf>`; `--kind epic` step; `MB.9.2.1.2` fallback `.2a`/`.2b` |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.21s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-9 `implementability` pass (MAJOR-1..2 + MINOR-1..5)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §8 (`MB.3.4.2`, `MB.9.2.2`, `MB.9.2.1.2`), §8.1 item 2, §8.2 | `MB.3.4.2` owns `blastit-fastcgi/src/lib.rs` (`pub mod lockdown;`); `MB.9.2.2` owns `upstream/mod.rs` (`pub mod static;`); `MB.9.2.1.2` declares `mod upstream;` in `scheduler.rs` |
| MAJOR-2 | §8 (`MB.3.5.2`), §8.1 item 2, §8.2 | `supervise.rs` owned by `.5.2`, `Parts`/`Supervisor`/`new` `pub(crate)`, clippy `-D warnings` in acceptance |
| MINOR-1..5 | ADR d1; §8 (`MB.3.6.3.2`, `MB.9.2.2`), §8.1 items 1/3 | `try_acquire` in ADR; `tests/fake_php_fpm_smoke.rs`; `#[path]` fake include; `.2b` carries the four remaining deliverables; all eleven parents in the `--kind epic` list + `MB.3.4.2 depends_on` |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.20s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-10 `implementability` pass (MAJOR-1..3 + MINOR-1..6 + type matrix)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §4.5.1, §4.7 r18, §8 (`MB.3.4.1`/`.2`, `MB.3.6.2.1`), §8.2, ADR d4 | `LockdownError`/`CgiError`/`RecordError` in `blastit-fastcgi`; adapter mapping in `MB.3.6.2.1`; zero-dep grep honest |
| MAJOR-2 | §4.5.1, §8 (`MB.3.4.2`/`MB.3.6.2.1`), §8.2, ADR d4 | `FastCgiPaths` defined in `blastit-fastcgi::lockdown`, built by `FastCgiBuilder` |
| MAJOR-3 | §4.8.2, §8 (`MB.3.5.2`/`MB.3.6.3.2`), ADR d6 | `render_pool_conf(&PoolConf<'_>) -> String`, std-only `PoolConf`, clippy gate moved to `.6.3.2` |
| MINOR-1..6 | §4.1, §4.2, §8, §8.1 | `ForeignOwner { uid }`; `.3` re-exports; `run_with` bound; `.6.3.2` fallback; §8.1/§8.2 authoritative; clippy gates + allow convention |
| Structural | §8 (type-ownership matrix) | one row per public type → defining crate → owning leaf → readers; single source of truth |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.23s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-11 `implementability` pass (MAJOR-1..3 + MINOR-1..7 + crate/derive matrices)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §8 (`MB.3.6.1.2`), crate-edge matrix, type matrix, ADR d1 | `blastit-backend` added to `crates/blastit/Cargo.toml` by `MB.3.6.1.2` + `cargo tree` grep; `MB.3.6.3.1` is a `PinnedSocket` reader |
| MAJOR-2 | §4.2 crate table, §8 (`MB.9.2.1.1`), crate-edge matrix, ADR d1 | `blastit-http` added to `crates/blastit-scheduler/Cargo.toml` by `MB.9.2.1.1` + `cargo tree` grep |
| MAJOR-3 | §4.2.1, derive matrix, ADR d1 | observation path derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` (+`Copy` where noted); `UpstreamHead`/`PinnedSocket`/`PoolConf` pinned |
| MINOR-1..7 | §4.2.1, §4.5.1, §4.8.3, §4.10.1, §8, §8.1, §8.2 | clippy only in §8; `Session`/`SessionMsg` `pub(crate)`; `.3` dropped from `Registry` readers; `RecordError::Truncated`; `PoolReadyInfo`; `new_with_upstream` signature; `MB.3.6.2.1` fallback `.1a`/`.1b` |
| Structural | §8 (crate-edge + derive matrices) | both declared sources of truth beside the type matrix; §4.2 crate table and `Lands` reconciled |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.25s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-12 `implementability` pass (MAJOR-1..2 + MINOR-1..10)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §8 (`MB.9.2.1.2`/`.3`), §8.1 item 2, §8.2 | `mod upstream;` in `crates/blastit-scheduler/src/lib.rs` (crate root); `lib.rs` in `.2`'s paths (5); `.3` only `pub use`; crate-root acceptance |
| MAJOR-2 | §4.2.1, §8 (`MB.9.2.1.1`), crate-edge matrix, ADR d1 | `ReqHeadView::scheme() -> Scheme` (frame-owned); `begin_raw` maps to `Tls`; `blastit-frame → no new edge` row |
| MINOR-1..10 | §4.2, §4.2.1, §8, §8.1 | `MB.3.6.3.1` readers; `Session`/`SessionMsg` behaviour-only; `MB.9.1.2` names all five conformance types; `fastcgi_builder.rs` owned; test-lint wording; `POOL_PARENT_FD_ENV`; matrix footnote; 3→4→5 binaries; `MB.3.6.2.1` drops `lib.rs`; §4.2 block `pub(crate)` |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.21s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-13 `implementability` pass (MAJOR-1 + MINOR-1..8)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §4.2.1, §8 (`MB.3.5.1`), §8.2, type matrix, ADR d1 | `Executor`/`FastCgiPolicy` added to `lib.rs`'s `pub use model::{…}` + doctest proving root reachability |
| MINOR-1..8 | §4.2, §4.2.1, §8, ADR d1 | `blastit_frame::Scheme`; `Scheme::from_code(..)?`; `deny(missing_docs)`; `Tls` derives; `PoolConf` + conformance types in matrix; `blastit-http → no backend` + `serde` derive; `async run_with`; `MB.3.8` 7 entries/9 files |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-14 `implementability` pass (MAJOR-1 + MINOR-1..7)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §4.1, §1.3, §8 (`MB.3.5.2`), ADR d2 | `SocketProbe::check_entry` + `NotRegularFile`/`Unreadable`/`OutsideRoot`; `check_fastcgi_entries(.., probe, uid)`; syscall body in `crates/blastit/src/probe.rs`; manifest stays I/O-free |
| MINOR-1..7 | §1.3, §4.2.1, §8 | `ProbeUid::Dev`; `ALL_KINDS: &'static [&'static str]`; `docs_fences.rs` three edits; readers; serialized `lib.rs`; doctest surface; crate-edge wording |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.20s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-15 `implementability` pass (MAJOR-1 + MINOR-1..7)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §4.1, §8 (`MB.3.5.2`, `MB.3.7`), ADR d2 | `setfsuid`/`setfsgid` around `openat2` + `faccessat2(AT_EACCESS)`, restore on every path (failure boot-fatal); fake-probe unit + root-only `check_fastcgi_entry_0000_refused_for_route_uid` e2e |
| MINOR-1..7 | §1.3, §4.1, §8, §8.1 | per-route `check_fastcgi_entries(.., route_index, uid)`; `provides_parent_fd()`; three rule phrases; `ProbeUid` authoritative; derives/docs; no-`rustix` grep in `MB.3.5.2`; `.2a`/`.2b` fallback |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.24s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-16 `implementability` pass (MAJOR-1 + MINOR-1..4)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §4.1, §8 (`MB.3.5.2`, `MB.3.6.3.1`, `MB.3.7`), ADR d2 | probe child (`setresgid`/`setresuid`, caps dropped, `SeccompRole::View`, `openat2`/`fstat`/`faccessat2`, one reply byte; missing/garbled reply boot-fatal); `DevProbe` no child |
| MINOR-1..4 | §4.1, §8 | gid == uid; `rule: String` + 7-row phrase table; both call sites; oracle split unprivileged vs root-only |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.21s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-17 `implementability` pass (MAJOR-1..3 + MINOR-1..11)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-1 | §4.1 step 4, ADR d2 | `libc::syscall(SYS_faccessat2, fd, "", R_OK, AT_EMPTY_PATH\|AT_EACCESS)` with `// SAFETY:`; rustix's `accessat` rejects `EMPTY_PATH` |
| MAJOR-2 | §4.1 step 2, ADR d2 | child calls **only** `blastit_sys::drop_privileges(uid)`; inlining `setresgid`/`setresuid` forbidden (rootless init keeps `CAP_DAC_OVERRIDE`) |
| MAJOR-3 | §4.1 steps 4–6, ADR d2 | `O_PATH\|O_NOFOLLOW\|O_CLOEXEC` + `fstat` `S_ISREG` + `faccessat2`; 5 s `wait_readable`, `SIGKILL` → `SocketProbeError::Timeout` |
| MINOR-1..11 | §4.1, §8, §8.1 | `fork(interpreter)`; `M` byte, `ForeignOwner` off entry table; parent pipe hygiene; `ProductionProbe` comment; `manifest_dirfd` doc; `strip_prefix(root)`; `render_pool_conf` wording; three fallbacks; probe matrix row; duplicate paragraph removed; errno mapping |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-18 `implementability` pass (four one-liners; lens PASSES)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| R18-1 | §4.1 steps 5–6, ADR d2 | reply set `O/U/N/S/R/M/I/T`; outside-set/short = boot-fatal; diagnostic byte maps to its variant, every non-`O` aborts boot |
| R18-2 | §8 (`MB.3.5.2`) | `Timeout` in the enumeration + `probe_timeout_is_boot_fatal` |
| R18-3 | §4.1 (`ProductionProbe`, step 3) | `pub seccomp: bool` → `install_filter(self.seccomp, …)`, honours `BLASTIT_SECCOMP=0` |
| R18-4 | §4.1 ("`ENOSYS` policy") | `faccessat2` `ENOSYS` refuses boot with an explicit message; kernels < 5.8 unsupported for `[route.fastcgi]` |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-4 `testability` pass (MAJOR-T1..T3 + MINOR-T1..T12)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-T1 | §7.1, §7.2, §8 (`MB.3.6.3.1`), ADR d6 | new `crates/blastit/tests/critical_fastcgi_parent_fd.rs` (6th path): one row per argv rule, exit 1 + message; `FD_CLOEXEC` positive; root-only legs in `MB.3.7` |
| MAJOR-T2 | §7.2 `R9`/`R20` | `lazy_pool_bind_timeout_is_503_retry_after_never_502` (`503`, `Retry-After: 1`, no `502`, slot release, later `PoolReady`) |
| MAJOR-T3 | §7.2 `R4` | `workers_cap_answers_503_for_the_second_concurrent_request` (`workers = 2`, three concurrent) + `capacity()` equality for `Pool`/`View` |
| MINOR-T1..T12 | §4.7, §4.11, §5.2, §7.1, §7.2, §8.2 | row 17/2 owners; `MIXED` reasoning; `P10` `431`; exact `cases()`; whole `--json` fence; `check_entry` ENOSYS/reply parser; fn names; plan drift; `R16` symbols; `R19`/`R10`/`R18` edges; pool single-UID fixture |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-5 `testability` pass (MAJOR-T1 + MINOR-T1..T10)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-T1 | §4.8.1 step 4, §7.2 `R2`/`R3`, §8 (`MB.3.6.3.1`), ADR Consequences | view/zygote `ChildIo.keep` omits `pinned_socket_fds()`; `view_child_holds_neither_parentfd_nor_ctl_chan` (mixed manifest) asserts `/proc/<view_pid>/fd`; unprivileged |
| MINOR-T1..T10 | §7.1, §7.2, §7.3, §8, §8.1 | inventory (`parent_fd` `.3.6.3.1`, pool `.3.6.3.3`); §8.1 paths; cli arm test; invalid `BLASTIT_CHECK_POOL_PARENT_FD`; pure `validate_parent_fds`; `5 s + 1 ms` + `Unavailable`; `Barrier`; `R2`/`R3` row; `--self-test` `#n` union; `cli.rs` serialized |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-6 `testability` pass (MAJOR-T1 + MINOR-T1..T6)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MAJOR-T1 | §7.1, §7.2 (`R9`/`R20`, `R17`/`R18`), §8 (`MB.3.6.2.2`, `MB.3.6.4.2`), ADR d6 | `external_mode_never_calls_ensure_pool` (recording `PoolPort`, 0 vs 1) + `external_pool_never_spawns_php_fpm` (`/proc/<init>/…/children` has no `php-fpm`) |
| MINOR-T1..T6 | §7.1, §7.2, §8 | "Proves" clause; `MB.3.6.3.1` Lands view oracle; init `ctl.chan` inode predicate + `view.chan` positive; preconditions (`200`, `children`, non-empty fd set, no skip); mixed-fixture contract; init-side `requires-root` split |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-7 `testability` pass (MINOR-T1..T4; lens PASSES)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| MINOR-T1 | §7.2 `R17`/`R18` | `200` precondition; transitive `/proc/<init_pid>/task/*/children` walk; positive control (renamed `/bin/sleep`) |
| MINOR-T2 | §7.2 `R2`/`R3` | both halves: pinned parent `(st_dev, st_ino)` + init's `ctl.chan` inode absent; `view.chan` present |
| MINOR-T3 | §7.2 `R2`/`R3`/`R20` Suite | `critical_fastcgi_parent_fd.rs` added |
| MINOR-T4 | §7.2, §8.1 item 2, §8.2 | `smoke.rs` not importable; local `children()`/`fds()` duplication (precedent `critical_dev_and_fail_closed.rs:482`), no path change |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.18s
$ uv run --no-project tools/plan.py validate
plan OK
```

## Round-3 `consistency` pass (C8-M1..M4 + C8-m1..m9)

| Finding | Sections changed | What is now pinned |
|---|---|---|
| C8-M1 | §4.1, §4.8.2, §7.2, ADR d2 | `env[HOME]=<runtime>/views/<uid>/tmp` everywhere |
| C8-M2 | §4.8.2, §4.1, §7.2, ADR d6 | `pm.max_requests = max(BLASTIT_VIEW_MAX_REQUESTS, 500)`; constant in `supervise.rs` (`MB.3.6.3.2`) |
| C8-M3 | §8.2 (new row) | `exchange` boxed `BodySource`/`ResponseSink`, current `ExchangeOutcome`/`Observation`, `pub(crate)` `Scheduler::session` |
| C8-M4 | §8.1 item 1 | every split bullet = "exactly the `Files` cell of §8's row for that id" |
| C8-m1..m9 | §4.1, §4.10.3, §8, §8.1, §8.2, §9, ADR d1 | placeholder; phrase list; `B1`/`B2` ancestor rule; app predicate; `PoolSpawn`; numeric fallback children; three §8.2 rows; `Registry::new` set; `Ctl::Spawn` doc |

### Acceptance evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.20s
$ uv run --no-project tools/plan.py validate
plan OK
```
