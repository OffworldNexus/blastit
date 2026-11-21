# Result MB.3.36 — Stage-1 probe child (ADR-0010 §3)

`STATUS: done`

## 1. What the gap was, and what closes it

`boot_bindings` re-opened the NTS `libphp` and the shim, asserted `(st_dev,
st_ino)` against stage 0's record and handed the descriptors to the view fork —
but nothing **loaded** the shim at boot, so a shim whose `blastit_php_minor()`
disagreed with the library was refused by a lazily forked worker at the first
request (a `500`). `ADR-0010` §3 requires one contained probe child per
`[route.php]` route **before** `bind_tcp`.

Closed by forking exactly that child in stage 1 between `boot_bindings` and the
listener, and by tightening the protected suite to the boot-refusal shape
(reviewer's MAJORs 1+2) plus a timeout leg.

## 2. Files changed (all inside `paths`)

| File | Change |
|---|---|
| `crates/blastit/src/php_view.rs` | `PhpProbeBoot` + `probe_bindings()`: one fork per PHP slot, child sequence `prepare → setpgid(0,0) → drop_privileges(route uid, production) → harden_child(PDEATHSIG=SIGKILL + NO_NEW_PRIVS) → EmbeddedView{egress} → set_dumpable(false) → HOME/TMPDIR → Loaded::load (ABI+minor) → worker_init → worker_shutdown → _exit(0)`; parent polls the child's stderr pipe, `waitpid(WNOHANG)`, and on `BLASTIT_PROBE_TIMEOUT_MS` does `kill(-pgid, SIGKILL)` + `kill(pid, SIGKILL)` + bounded reap. `dlopen` stays inside the child. |
| `crates/blastit/src/stage1.rs` | `read_config` now also returns the probe timeout (document member `php_probe_timeout_ms`); `probe_bindings(...)` is called right after `boot_bindings` and **before** `bind_tcp`; any failure is `fail(...)` → `exit 1` with §4.1's sentence. |
| `crates/blastit/src/stage0.rs` | `BLASTIT_PROBE_TIMEOUT_MS` is read in stage 0 (the only place it can be: `blastit-env` strips every `BLASTIT_*` across the stage-1 `execve`), validated (non-numeric or `0` is a boot refusal naming the fix), and folded into both configuration documents (`exec_stage1` and `exec_stage1_dev`); default `PROBE_TIMEOUT_DEFAULT = 5000 ms`. `php_probe_refusal()` reuses `present_refusal()`, so the post-barrier sentence is the stage-0 sentence word for word. |
| `crates/blastit/tests/critical_php_embed.rs` | Shim-minor leg tightened (exit `Some(1)`, no `ready`/`spawned`/`request`, connect **refused**, `(PHP 8.4, <arch>-<libc>)` named, `dlopen` only below the fork); `stdout == ""` and `assert_never_loaded` dropped there (the probe legitimately loads, and stage 1 writes `boot` on stdout); new hang leg with a `BT_SHAPE`-reporting shim; module doc's "known deviation" replaced with the probe contract. |
| `docs/knowledge/LESSONS.md` | one bullet (standing implementer instruction; not in `paths`): a stage-1 refusal is a JSON `app.output` event with escaped quotes, and a new stage-1 knob must ride the config document, not the environment. |

No `unwrap()`/`expect()` on any untrusted path; `unsafe` blocks carry `SAFETY`
comments (`setpgid`, `waitpid`, `_exit`); every refusal is bounded (`detail()`
caps the child's stderr at 512 bytes and collapses it to one line, well under the
2048-byte log ceiling).

## 3. Acceptance evidence

### `cargo test -p blastit --locked --test critical_php_embed`

```console
$ cargo test -p blastit --locked --test critical_php_embed
running 8 tests
test no_libphp_is_mapped_above_the_view_worker ... ok
test a_fatal_is_contained_and_the_next_request_is_served ... ok
test an_exit_is_contained_and_the_next_request_is_served ... ok
test a_non_library_blastit_libphp_refuses_before_anything_loads ... ok
test a_libphp_replaced_between_the_stages_refuses_the_boot ... ok
test a_zts_libphp_refuses_before_anything_loads ... ok
test a_probe_that_hangs_is_killed_and_refuses_the_boot ... ok
test a_shim_for_another_minor_refuses_the_boot_before_the_listener ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.76s
```

### `cargo test -p blastit --locked`

```console
$ cargo test -p blastit --locked ; echo EXIT=$?
EXIT=0
test result: ok. 184 passed; 0 failed; 1 ignored; …   (unit)
test result: ok. 4 passed … / 19 / 11 / 5 / 8 / 11 / 1 / 3 / 31 passed, 0 failed   (each integration suite)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s   (doc)
```

(the `critical_php_backend`, `critical_dev_and_fail_closed`, `critical_check_engine`,
`no_python_build` and `smoke` suites are inside those lines and green.)

### `make check`

```console
$ make check ; echo EXIT=$?
guard: OK (5 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 9 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
… RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
… cargo test --workspace --all-targets --locked --features python
… py-lint / docs-unit / docs-check / docs-test
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

### `uv run --no-project tools/plan.py validate`

```console
$ uv run --no-project tools/plan.py validate
plan OK
```

### `critical_guard --node MB.3.36`

```console
$ uv run --no-project tools/critical_guard.py --base 72e6d1fece8878466d418a5e278c4c5af856d74b --node MB.3.36
critical-guard: 72e6d1fece8878466d418a5e278c4c5af856d74b..working tree, 8 changed file(s)
node MB.3.36: paths ['crates/blastit/src/php_view.rs', 'crates/blastit/src/stage1.rs', 'crates/blastit/src/stage0.rs', 'crates/blastit/tests/critical_php_embed.rs']
  no change to a protected critical test
EXIT=0
```

**Caveat the lead must know:** `crates/blastit/tests/critical_php_embed.rs` is
still **untracked** in this worktree (MB.3.13.4 created it and has not been
committed), so `git diff <base>` cannot see this node's edit and the guard
reports the file as "no change". The guard *does* echo the node's `paths`,
which include the file verbatim, so the edit is authorized by construction; once
MB.3.13.4 and MB.3.36 are committed the same run reports
`M crates/blastit/tests/critical_php_embed.rs [authorized by …]`. No protected
assertion was weakened: the shim-minor leg went from "no `200` at the first
request" to "`exit 1`, nothing listening, no worker event", and two new legs were
added.

## 4. Probe process shape (evidence)

`… -- --nocapture --test-threads=1`, hang leg (a shim whose `worker_init` writes
its own `/proc`-level shape and then never returns):

```console
killed at BLASTIT_PROBE_TIMEOUT_MS=250 after 272.707054ms; probe pid 2686931 ppid 2686928 pgid 2686931 uid 1000 seccomp EPERM
route "app": [route.php] entry "public/index.php": the probe child (pid 2686931) did not load `libphp` and its shim within 250 ms, so it was killed; raise BLASTIT_PROBE_TIMEOUT_MS if this host needs longer; this host is (PHP 8.4, x86_64-gnu), whose embedded shim is libblastit_php84-x86_64-gnu.so; install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
```

* `pid != boot`, `ppid == boot` → a fork of init, not init itself.
* `pgid == pid` → its **own** process group (`kill(-pgid)` names only the probe).
* `uid == euid == 1000` → the route's uid (in `dev` that is the caller, exactly
  as the views run; production passes `drop_privileges(route_info.uid)`).
* `inet == EPERM (1)` → `SeccompRole::EmbeddedView { egress: false }` was
  installed before `worker_init`, so the shim ran with the worker's filter.
* `seccomp: true` in the same boot's `boot` event.

Shim-minor leg (both stubs' `__attribute__((constructor))` canaries):

```console
refused at boot (stage-1 probe), load in a child only: [(2686936, 2686933), (2686936, 2686933)]
route "app": [route.php] entry "public/index.php": the probe child (pid 2686936) exit code 1: the shim is built for PHP 8.3 but the host's libphp reports 8.4.27; install the libblastit_php83 shim for this host; this host is (PHP 8.4, x86_64-gnu), whose embedded shim is libblastit_php84-x86_64-gnu.so; install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
```

* both canaries: `pid 2686936`, `ppid 2686933` (the boot) — the load is below
  the fork and nowhere above it.
* `Some(1)`, no `ready`/`process.spawned`/`request` event, and
  `TcpStream::connect` to the port errors — asserted by
  `assert_no_serving_surface`, unconditionally.

## 5. Timeout, and the accepted twin

* Hang leg: `BLASTIT_PROBE_TIMEOUT_MS=250` bounds the boot at **272 ms** wall
  (`elapsed < 10 s` asserted inside a 20 s `BOOT_TIMEOUT`), exit `1`, and the
  kill path is `kill(-pgid, SIGKILL)` → `kill(pid, SIGKILL)` → bounded
  `waitpid` reap (`PR_SET_PDEATHSIG(SIGKILL)` is the backstop).
* Twin: `no_libphp_is_mapped_above_the_view_worker` (real `libphp8.4` +
  `libblastit_php84`) still answers `200` with the body, init's maps hold
  neither library, and `3 of 3 forked roles non-dumpable (maps EACCES)`. The
  containment legs (`exit()`, fatal) also pass against the real pair, which
  means the probe's `worker_init` + `worker_shutdown` runs on this host's PHP
  before the listener binds.

## 6. Design notes / decisions taken

* **Where the timeout comes from.** `BLASTIT_PROBE_TIMEOUT_MS` is read by stage 0
  and carried in the configuration document as `php_probe_timeout_ms`, because
  `blastit-env` strips every `BLASTIT_*` variable across the stage-0→stage-1
  `execve` (`ADR-0010` §3, `R12`) — an environment variable cannot reach stage 1
  on its own. `Config` lives in `config.rs` (outside this node's `paths`), so the
  member is added to the document `Value` in `stage0.rs` and read from that
  `Value` in `stage1.rs`; `Config::from_json` ignores unknown members, so nothing
  in `config.rs` had to move.
* **The ABI/minor check is `Loaded::load`.** `blastit-php-sapi`'s loader refuses
  `blastit_php_abi_version() != 1` and a `blastit_php_minor()` that disagrees
  with the library's `php_version()`, so the probe calls `Loaded::load`, then
  `worker_init` (module + INI lockdown actually starting is the deepest check),
  then `worker_shutdown`.
* **One shim for the whole boot.** `R8`'s wording has the child call
  `php_version()` to *pick* the shim; per `ADR-0010` §3 and `R6`, stage 0 already
  selects it from the host tuple and records the inode, and `boot_bindings`
  re-opens that exact file. The probe therefore *verifies* the minor rather than
  selecting a shim — a second selection path in the child would be a second
  verification, which this node explicitly avoids.

## 7. Follow-ups (out of scope)

1. `ADR-0010` §3's `check` half — `Host::stage1()`'s `Reexec --role=php-probe`
   with `env = {}` (rejected in `Production`) — is still unimplemented; `blastit
   check` does not reach stage 1 today. Belongs to a `check`/`MB.3.6.4` node.
2. `docs/settings.md` (not in `paths`) does not document
   `BLASTIT_PROBE_TIMEOUT_MS`; `MB.3.20` owns the public docs sweep.
3. `R8`'s `waitid(WNOWAIT)` peek belongs to `init`'s generic reaper
   (`MB.3.25`/`MB.3.26`); the probe reaps its own pid only, and after
   `kill(-pgid)`, so it never races the supervisor.
4. `crates/blastit/tests/critical_php_embed.rs` is untracked in this worktree
   (see §3 caveat) — the lead's commit will be the first to record it.
