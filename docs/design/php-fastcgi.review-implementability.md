# Implementability Review — `php-fastcgi` (MB.3.3), Round 18

Lens: **implementability** (cheap-model buildability). Targets:
`docs/design/php-fastcgi.md` (§4.1, §8, §8.1, §9) and
`docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md` (decision 2).
Checked against `crates/blastit-sys/src/{seccomp,privs}.rs`,
`crates/blastit/src/{child,forking,stage0}.rs`, `crates/blastit/Cargo.toml`,
`crates/blastit-manifest/src/lib.rs`.

## 0. Round-17 closure (verified on disk, not taken on trust)

| R17 finding | Where it now lives | Verdict |
|---|---|---|
| MAJOR-1 `rustix::accessat` cannot take `AT_EMPTY_PATH` | §4.1 step 4: `unsafe { libc::syscall(libc::SYS_faccessat2, fd, c"".as_ptr(), libc::R_OK, libc::AT_EMPTY_PATH \| libc::AT_EACCESS) }` + `// SAFETY:` | **Closed.** `libc = "0.2.191"` is a direct dep of `crates/blastit` (`Cargo.toml:29`); `libc::SYS_faccessat2`, `AT_EMPTY_PATH`, `AT_EACCESS` all exist. No `blastit-sys` helper, no unowned file. |
| MAJOR-2 false "kernel drops caps" claim | §4.1 step 2: "call **`blastit_sys::drop_privileges(uid)` and nothing else** … **Inlining `setresgid`/`setresuid` is forbidden**" | **Closed.** Matches `privs.rs:54` (`caps::clear_before_uid_change` → `setgroups(0,NULL)` → `setresgid(uid,uid,uid)` → `setresuid` → `caps::clear_after_uid_change` → read-back). `gid == uid` is exactly what `drop_privileges` does, so the "no separate route gid" note is true. |
| MAJOR-3 `O_RDONLY` on a FIFO hangs PID 1 | §4.1 step 4 `O_PATH\|O_NOFOLLOW\|O_CLOEXEC` + `fstat` `S_ISREG` + `faccessat2`; step 6 `child::wait_readable(…, 5 s)` → `Timeout` | **Closed.** `faccessat2` on an `O_PATH` fd is the documented use; `wait_readable(fd: BorrowedFd, timeout: Duration) -> io::Result<bool>` exists at `child.rs:155`; `Fork::Parent(Pid)` at `proc.rs:45`. |

All eleven R17 MINORs are also present in the text (step 1 `fork(interpreter)`, step 5 `M`/no `ForeignOwner`, step 6 `waitpid` not `reap()`, `ProductionProbe` comment, `manifest_dirfd` flags, `strip_prefix`, `render_pool_conf`/`pool_test` `pub`, `.2a`/`.2b` fallback, type matrix, deleted duplicate paragraph, errno table).

## 1. Probe-child contract: codeable end to end

**Parent/child syscall list.** Step 1 `crate::forking::fork(interpreter)` (`forking.rs:41`), step 2 `drop_privileges`, step 3 `harden_child(Signal::KILL)` + `install_filter(seccomp, SeccompRole::View { egress: false })` (`child.rs:140`) + `set_dumpable(false)` (`privs.rs:238`), step 4 `openat2`/`fstat`/`faccessat2`, step 5 one byte + `_exit(0)`, step 6 `wait_readable`/`kill`/`waitpid`/`read_one`. Every function named exists with a compatible signature except the two conversions in MINOR-5.

**Ordering is correct and non-obvious.** `privs.rs:46` warns `setresgid`/`setresuid` clear `PR_SET_PDEATHSIG`, so `harden_child` must follow `drop_privileges` — the design's step 2 → step 3 order is right, and it is the *opposite* of the order a naive reader would pick.

**`SeccompRole::View { egress: false }` does not forbid anything the child needs.** `blastit_sys::install` is a **denylist** (`seccomp.rs:210-246` `DENIED` + socket-family checks, default `SECCOMP_RET_ALLOW`). `openat2`, `fstat`/`newfstatat`, `faccessat2`, `write`, `_exit`, `close` and `prctl` are all absent from `DENIED`, so they are allowed. `setgroups`/`setresgid`/`setresuid`/`capset` run in `drop_privileges` **before** `install_filter`; `prctl` also runs **after** the filter (`set_dumpable`) and is allowed. `install()` refuses a multi-threaded caller (`seccomp.rs:128-132`), and the probe child is forked from stage 0, which starts no runtime or thread (`stage0.rs` has no `Runtime::new`/`thread::spawn`), so the child is single-threaded. **No MAJOR.** (Wording nit: MINOR-9.)

**`DevProbe` inline path.** Same `O_PATH`/`fstat`/`faccessat2` sequence, no fork, `provides_parent_fd() == true`, returns the `OwnedFd` that `check_fastcgi_sockets` collects and `serve_and_run` `clear_cloexec`s — one resolution per parent, matching §4.1 lines 432-440. Codeable. (Unspecified branch: MINOR-6.)

**Two oracles are owned.** `MB.3.5.2` owns the unprivileged fake-probe suite (`crates/blastit-manifest/tests/critical_fastcgi_manifest.rs`); `MB.3.7` owns `e2e/rootless/run.py::check_fastcgi_entry_0000_refused_for_route_uid` and lists `e2e/rootless/**`. Both are in the plan. Gap: MINOR-8.

## 2. Findings

### MINOR-1 — "non-zero reply" contradicts the seven-byte reply set (design **and** ADR)

> §4.1 step 6: "read exactly one byte; a missing, garbled, short or **non-zero** reply is boot-fatal `SocketProbeError::Io` with the distinct message `probe child exited without a reply`"
> ADR-0008 decision 2: "treats a missing/garbled/**non-zero** reply as boot-fatal"

Step 5 two paragraphs earlier defines seven reply bytes `O/M/U/N/S/R/I` — every one of `M`(0x4D), `U`, `N`, `S`, `R`, `I` is non-zero. Read literally, step 6 makes all six failure bytes boot-fatal `Io`, so the errno mapping (`ENOENT → M`, `EACCES/EPERM → U`, …) becomes dead code and every failure prints `must be stat-able` instead of `must be readable by uid <uid>`. **No acceptance command catches this**: `check_fastcgi_entry_0000_refused_for_route_uid` only asserts exit `1`, which the degraded path still produces. A cheap implementer following step 5 gets it right; one following step 6 gets a silently wrong diagnostic.

**Fix:** replace "non-zero" with "unrecognised" in both places: *"a missing, short, or unrecognised reply byte (not one of `O/M/U/N/S/R/I`) is boot-fatal `Io`; `O` alone means readable."*

### MINOR-2 — `Timeout` is missing from §8's `SocketProbeError` list and from the fake-probe oracle

§4.1 line 330 defines `Timeout` and the entry table maps it to `must be stat-able (probe timed out)`; §8 `MB.3.5.2`'s `Lands` enumerates `Missing/NotSocket/NotRegularFile/Unreadable{uid}/OutsideRoot/ForeignOwner{uid}/Symlink/ParentWritableOrSymlink/Io` — **no `Timeout`** — and the protected suite injects only `NotRegularFile`/`Unreadable`/`OutsideRoot`. The one variant that proves the 5 s bound exists is the one with no leaf assignment and no oracle.

**Fix:** add `Timeout` to the §8 list and inject it in `critical_fastcgi_manifest.rs` (`Timeout` → `must be stat-able (probe timed out)`).

### MINOR-3 — `install_filter(seccomp, …)` has no source

> §4.1 step 3: "`install_filter(seccomp, SeccompRole::View { egress: false })`"
> §4.1 line 346: `pub struct ProductionProbe { pub init_uid: u32, pub scheduler_uid: u32, pub layout: UidLayout, pub routes: usize }`

`install_filter` takes `enabled: bool` (`child.rs:140`), but `ProductionProbe` has no `seccomp` field and `check_entry` has no such parameter. The implementer must invent the source; the two obvious choices (`true` vs. `Config.seccomp`) diverge under `BLASTIT_SECCOMP=0` and on a non-`x86_64`/`aarch64` host (`install()` returns `UnsupportedArchitecture` → child exits with no byte → `Io`).

**Fix:** add `pub seccomp: bool` to `ProductionProbe` and state that `MB.3.6.3.1` constructs it from `Config.seccomp`; or write `install_filter(true, …)` with a one-line "production always filters" comment.

### MINOR-4 — pre-check child failures all collapse into "no reply"

If `drop_privileges` (`CAP_SETPCAP` absent), `harden_child`, or `install_filter` (unsupported arch) fails, the child cannot reach step 5 and exits with no byte → `Io("probe child exited without a reply")` → `must be stat-able`. That message names neither the capability nor the arch, unlike `B3`'s "exits `1` naming `CAP_CHOWN`/`CAP_FOWNER`" discipline (§4.8.3).

**Fix:** define a reply byte for pre-check failure (e.g. `P`) written by the child's error path, mapped to a rule naming `CAP_SETPCAP`/seccomp; or state that step 2/3 failures `_exit(1)` and the parent's message is extended to "…or the child could not drop privileges".

### MINOR-5 — two literal type conversions are unstated

- `Fork::Parent(Pid)` carries `rustix::process::Pid` (`proc.rs:45`), but step 6 writes `libc::waitpid(probe_pid, &mut status, 0)`; `libc::waitpid` takes `pid_t`.
- Step 4 writes `libc::syscall(libc::SYS_faccessat2, fd, …)` where `fd` is the opened `O_PATH` descriptor (`OwnedFd`), not a `RawFd`.

Both are one-token fixes but both are `E0308` on the first `cargo check` if copied verbatim.

**Fix:** write `probe_pid.as_raw_pid()` and `fd.as_raw_fd()` (or `rustix::process::waitpid`), or add "(`Pid`/`OwnedFd` converted to raw)".

### MINOR-6 — `DevProbe::check_entry(ProbeUid::Route(u))` is unspecified

> §4.1 line 432: "`ProbeUid` is authoritative for readability … `check_entry` uses its `uid` argument."

`DevProbe` runs inline as the invoking uid; `faccessat2(AT_EACCESS)` cannot evaluate another uid, so `ProbeUid::Route(u)` with `u != self.uid` is unimplementable in-process. It is unreachable today (`dev`/`check` always pass `ProbeUid::Dev`), but nothing says so, and a literal implementer may ignore the argument — a silently skipped check if a future caller passes `Route`.

**Fix:** state that `DevProbe::check_entry` treats `ProbeUid::Route(u)` with `u != self.uid` as a programmer error (`Io` or `debug_assert!`), since `dev`/`check` only ever pass `ProbeUid::Dev`.

### MINOR-7 — the `provides_parent_fd()` contract violation has no error variant

> §4.1 line 425: "a probe that returns `Some(fd)` while `provides_parent_fd()` is `false` is a `ManifestErrors` programmer error (checked in `check_fastcgi_sockets`, not by downcasting)."

No variant is named, and §8 `MB.3.5.1` only says "error variants". The implementer must invent one inside a protected-adjacent enum.

**Fix:** name it (e.g. `ManifestError::ProbeContract { rule: "returned a parentfd without provides_parent_fd" }`) or state that it maps to `SocketProbeError::Io`.

### MINOR-8 — no unprivileged oracle for the errno→byte mapping or the 5 s bound

The fake probe exercises `check_fastcgi_entries`'s mapping from `SocketProbeError` to `FastCgiEntryInvalid.rule`; it never exercises the child's `errno → byte` table (step 5) or the `wait_readable` timeout (step 6). The root-only e2e is an integration oracle only (`0000` → exit `1`), so a wrong arm (`EPERM → O`, `ENOENT → U`) passes every acceptance command.

**Fix:** require a pure `fn reply_for(errno: i32) -> u8` in `crates/blastit/src/probe.rs` with a unit test covering all seven arms plus the default, keeping the root-only e2e as the integration oracle.

### MINOR-9 — "allowing exactly those syscalls" is false for a denylist filter

> Task brief and §4.1 step 3: the filter "allow[s] exactly those syscalls".

`blastit_sys::install` is a denylist: everything not in `DENIED` and not a forbidden socket family is `SECCOMP_RET_ALLOW`. The child's needs are satisfied, but an implementer reading "exactly" will look for an allowlist that does not exist — and cannot add one, because `crates/blastit-sys/src/seccomp.rs` is owned by `MB.3.6.1.1`, not `MB.3.5.2`.

**Fix:** "the existing `View { egress: false }` filter denies none of these syscalls" (no `blastit-sys` change, no new path).

### MINOR-10 — `openat2`/`faccessat2` `ENOSYS` fallback exists in `B2` but not in the entry probe

`B2` (§4.8.3) gives a per-segment `openat(…, O_NOFOLLOW)` fallback "on `ENOSYS`"; the entry probe (step 4, and `DevProbe`'s inline path) has none. On a kernel without `openat2` (< 5.6) or `faccessat2` (< 5.8) — RHEL 8 (4.18), Ubuntu 20.04 (5.4) — `ENOSYS` maps to `I` → `must be stat-able` → boot fails, including on a developer laptop in `dev`/`check`.

**Fix:** either state the minimum kernel in §4.1/§1.3, or mirror `B2`'s `ENOSYS` fallback (`openat` + `faccessat` with the same flags) in both the child and `DevProbe`. If RHEL 8 / Ubuntu 20.04 CI images are in scope, this is the one finding that escalates to MAJOR (a boot failure on a supported host).

### MINOR-11 — `ProductionProbe`'s four fields are never read

`check` returns `Ok(None)` in production and `check_entry` uses none of `init_uid`/`scheduler_uid`/`layout`/`routes`. They are not dead-code-linted (public struct in a lib), but an implementer will ask what `check` is supposed to do with them and may invent a production socket check that `B1`/`B2` already own.

**Fix:** drop the four fields (a unit struct) or add "unused by `check`/`check_entry`; kept for `MB.3.6.3.1`'s construction site".

## 3. Sweep (short)

| Check | Result |
|---|---|
| 23 leaves | **23** (`MB.3.4.1`, `3.4.2`, `3.5.1`, `3.5.2`, `9.1.1`, `9.1.2`, `9.2.1.1`, `9.2.1.2`, `9.2.1.3`, `9.2.2`, `9.3`, `9.4`, `3.6.1.1`, `3.6.1.2`, `3.6.2.1`, `3.6.2.2`, `3.6.3.1`, `3.6.3.2`, `3.6.3.3`, `3.6.4.1`, `3.6.4.2`, `3.7`, `3.8`). ✓ |
| ≤ 7 files | `MB.3.5.2` = 7, `MB.9.2.1.1` = 7, `MB.3.8` exempt (9 files / 7 path entries). ✓ |
| ≤ 550 lines | `MB.3.5.2` is the only near-cap leaf (trait + errors + two checks + probe child + `DevProbe` + `ProductionProbe` + `supervise` narrowing + protected suite + 3 doc edits); the named `.2a`/`.2b` fallback (§8.1 item 3) covers it. ✓ |
| No unowned file | `crates/blastit/src/probe.rs` is created by `MB.3.5.2` and in its paths; `crates/blastit/src/{lib,supervise}.rs` single-owner; `child.rs`/`privs.rs`/`seccomp.rs` are **not** modified and need no change (`wait_readable`, `drop_privileges`, `install` already exist). ✓ |
| Cargo edges | `crates/blastit/Cargo.toml:29,31` already has `libc = "0.2.191"` and `rustix = "1.1.5"` with `fs`/`process`/`pipe` features; `blastit-manifest` gains **no** `libc`/`rustix` edge. `MB.3.5.2` correctly claims no `Cargo.toml`/`Cargo.lock`. ✓ |
| Object safety | `SocketProbe` methods take `&self`, return concrete types, no generics, default `provides_parent_fd`; `&dyn SocketProbe` works. ✓ |
| MSRV 1.88 | `c""` (1.77), `libc::SYS_faccessat2`, `OwnedFd`, `let-else` all ≤ 1.88. ✓ |
| No new external crate | Only `libc` + `rustix`, both existing direct deps. ✓ |
| Type ownership | `SocketProbe`/`SocketProbeError`/`ProbeUid` → `blastit-manifest::probe` (`MB.3.5.2`); `DevProbe`/`ProductionProbe` → `blastit::probe` (`MB.3.5.2`); reader `MB.3.6.3.1`. `UidLayout` is re-exported (`blastit-manifest/src/lib.rs:79`). ✓ |

## 4. Verdict

No BLOCKER. No MAJOR: every named syscall exists with a compatible signature, the `View { egress: false }` denylist forbids nothing the child needs, `libc`/`rustix` are already direct deps of `crates/blastit`, every file the leaf touches is owned, and the trait is object-safe. The eleven findings are contract-wording and oracle gaps that a cheap implementer resolves locally without changing a path, a dependency, or a signature.

Accepted follow-ups (all MINOR): **1** "non-zero reply" (design + ADR), **2** `Timeout` missing from §8/oracle, **3** `seccomp` source, **4** pre-check failure byte, **5** `Pid`/`OwnedFd` conversions, **6** `DevProbe` + `ProbeUid::Route`, **7** `provides_parent_fd` variant, **8** pure `reply_for` unit test, **9** "exactly" wording, **10** `ENOSYS` fallback (escalates to MAJOR only if RHEL 8 / Ubuntu 20.04 hosts are supported), **11** unused `ProductionProbe` fields.

VERDICT: PASS
