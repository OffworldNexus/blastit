# RESULT — MB.3.13.4

Protected suite: `crates/blastit/tests/critical_php_embed.rs` (the only path this
node owns; `MB.3.36`'s probe child tightened three of its legs, and this attempt
made the stub fixture rebuildable and closed three review MINORs). No `src/`, no
`Cargo.toml`, no other test file changed.

8 tests, **0.70 s** here (which has `libphp8.4` + a built shim), **0.58 s** on a
PHP-free host, **1.88 s / 1.35 s** under `--test-threads=1` (CI's runner, where
each test rebuilds the stub fixture), no Docker, `cc` required (a missing
compiler is a panic, never a skip).

| Test | Proves |
|---|---|
| `a_non_library_blastit_libphp_refuses_before_anything_loads` | stage 0 `.dynsym` refusal (exit 1, empty stdout, §4.1 sentence). No canary claim here: a text file has no constructor, so the zero-`dlopen` proof is the ZTS leg (MINOR 1) |
| `a_zts_libphp_refuses_before_anything_loads` | `tsrm_startup` present → ZTS refusal from `.dynsym` alone; the file *is* loadable and its canary stays empty (the suite's zero-`dlopen` proof) |
| `a_shim_for_another_minor_refuses_the_boot_before_the_listener` | stage-1 probe child: shim `blastit_php_minor()` ≠ host → exit 1, §4.1 sentence naming `PHP 8.3` / `8.4.27` / `libblastit_php83`, no `ready`/`spawned`/`request`, nothing listening; canary only in a child (`ppid` = boot); init maps clean |
| `a_probe_that_hangs_is_killed_and_refuses_the_boot` | `BLASTIT_PROBE_TIMEOUT_MS=250` kills a never-returning `worker_init` **and reaps it** (`/proc/<probe>` gone); the probe's own shape (own pgid, route uid, `socket(AF_INET)` → EPERM) is read back from the stub |
| `a_libphp_replaced_between_the_stages_refuses_the_boot` | inode pin (stage 0 `fstat` → stage 1 `O_NOFOLLOW`), exit 1, no `ready`/`spawned`/`request`, nothing listening, canary absent |
| `no_libphp_is_mapped_above_the_view_worker` | real pair serves 200; `/proc/<init>/maps` holds no `libphp`/`libblastit_php`; **every child that is not the route's view** (http/scheduler/dbproxy, taken from the `process.spawned` pids of the view) is content-asserted as root and `EACCES`-asserted for an unprivileged caller |
| `an_exit_is_contained_and_the_next_request_is_served` | `exit()` → 5xx, `cpu_ns`/`wall_ns`, slot respawned (new pid, parent = init, exactly one live worker of the route), next request 200 |
| `a_fatal_is_contained_and_the_next_request_is_served` | `trigger_error(E_USER_ERROR)` → the same, for the `zend_bailout` path |

Two-outcome honesty: `host_php()` is an independent probe (raw `std` file access,
`ADR-0010` §4.1 names, `.dynsym` bytes, shim filename spelling) and the boot is
handed the probe's own answer, so probe and boot cannot disagree. On a host with
the pair the containment/maps legs run for real; on one without it they assert
the §4.1 package sentence. Both faces were executed (see §5).

Stub ELFs (compiled by `cc -shared -fPIC` at test time): NTS `libphp8.4.so`
exporting `executor_globals` + `php_version`, ZTS `libphp8.4.so` exporting
`tsrm_startup`, shim stubs named `84` reporting `804` (accepted), `803`
(mismatch) and a `worker_init` that never returns (timeout), plus a text file
that is not an ELF. Each stub has an `__attribute__((constructor))` canary that
appends `<tag> pid=<pid> ppid=<ppid>` to a `-D`-baked path — no environment is
needed, because `blastit-env` strips every `BLASTIT_*` across the stage-1
`execve`.

## 1. `cargo test -p blastit --locked --test critical_php_embed`

```console
$ cargo test -p blastit --locked --test critical_php_embed
running 8 tests
test a_non_library_blastit_libphp_refuses_before_anything_loads ... ok
test no_libphp_is_mapped_above_the_view_worker ... ok
test a_fatal_is_contained_and_the_next_request_is_served ... ok
test an_exit_is_contained_and_the_next_request_is_served ... ok
test a_zts_libphp_refuses_before_anything_loads ... ok
test a_shim_for_another_minor_refuses_the_boot_before_the_listener ... ok
test a_libphp_replaced_between_the_stages_refuses_the_boot ... ok
test a_probe_that_hangs_is_killed_and_refuses_the_boot ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.69s
```

`-- --nocapture` evidence (the numbers this node is judged by):

```console
exit(): status 500  cpu_ns=1543666  wall_ns=18336040  cost_ns=3221782  retired pid 2853758  respawned pid 2853759 (ppid 2853732) uid 1000  live=[2853759]
init pid 2853702: no libphp/shim; above the view 0 map(s) content-checked, 2 denied (EACCES), 0 gone, of 3 children; euid 1000
fatal: status 500  cpu_ns=777317  wall_ns=19330351  cost_ns=3996608  retired pid 2853693  respawned pid 2853697 (ppid 2853662) uid 1000  live=[2853697]
killed at BLASTIT_PROBE_TIMEOUT_MS=250 after 417.33192ms; probe pid 2853767 ppid 2853764 pgid 2853767 uid 1000 seccomp EPERM, gone from /proc
refused (stage 1 inode pin) on attempt 1: {"event":"boot",…} {"event":"app.output","line":"route \"app\":
  libphp …/libphp8.4.so is not the file stage 0 inspected (st_dev/st_ino moved), so the worker
  would load something nobody verified; restore the package and restart blastit"}
refused at boot (stage-1 probe), load in a child only: [(2853771, 2853768), (2853771, 2853768)]
```

The canary pair is `(pid, ppid)`: both stubs were mapped by pid 2853771, whose
parent is 2853768 — the boot — so the load happened below the fork, and the
boot's own maps hold no stub (asserted in the same test). `cpu_ns <= wall_ns`
and `cost_ns >= cpu_ns > 0` hold in both crash legs (`1543666 ≤ 18336040` and
`777317 ≤ 19330351`); the crash is `500`, never `200`; and each respawn is a new
pid parented by the boot with exactly one live worker of the route.

## 2. `cargo test -p blastit --locked` (node acceptance)

```console
$ cargo test -p blastit --locked
exit=0   (13 × "test result: ok", 0 failures)
```

## 3. `make check`

```console
$ make check
exit=0
cargo test -p blastit --features python --test critical_php_embed
running 8 tests ... test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
```

The suite is green in **both** feature builds (default `php`, and the
`test-critical` leg's `--features python`).

## 4. `plan.py validate` and the critical guard

```console
$ uv run --no-project tools/plan.py validate
plan OK
exit=0

$ uv run --no-project tools/critical_guard.py --base 72e6d1fece8878466d418a5e278c4c5af856d74b --node MB.3.13.4
critical-guard: 72e6d1fece8878466d418a5e278c4c5af856d74b..working tree, 9 changed file(s)
node MB.3.13.4: paths ['crates/blastit/tests/critical_php_embed.rs']
  no change to a protected critical test
exit=0
```

## 5. The PHP-free face, executed

A mount namespace hides the host library (the test binary itself is unchanged):

```console
$ unshare -m -r bash -c 'mount --bind /dev/null /usr/lib/libphp8.4.so; cargo test -p blastit --locked --test critical_php_embed'
crw-rw-rw- 1 nobody nogroup 1, 3 /usr/lib/libphp8.4.so
running 8 tests ... test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.58s
--- threads=1: test result: ok. 8 passed; … finished in 1.35s   (real 1.477s)
```

So the whole suite is **0.58 s** parallel and **1.35 s** serialized on a
PHP-free host, and the refusal branch is a real, executed branch, not a skip.

## 6. CI's own runner: `--test-threads=1`, as root

`ci.yml` runs every test binary with `--test-threads=1`, the second time as uid 0
(`critical_php_embed` is not in `EXCLUDED`). Both paths were executed:

```console
$ cargo test -p blastit --locked --test critical_php_embed -- --test-threads=1
running 8 tests … test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.98s
dirs after run 1: 0
$ cargo test -p blastit --locked --test critical_php_embed -- --test-threads=1   # second, sequential run
test result: ok. 8 passed; … finished in 1.93s
dirs after run 2: 0

$ unshare -r bash -c 'cargo test -p blastit --locked --test critical_php_embed -- --test-threads=1 --nocapture'
init pid 2855562: no libphp/shim; above the view 2 map(s) content-checked, 0 denied (EACCES), 0 gone, of 3 children; euid 0
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.01s
```

The serialized run is slower on purpose: with one thread every test looks like
the last live test, so the fixture is **rebuilt** per test (7 tiny `cc -shared`
calls, ~0.2 s) after the previous test's guard removed it. Escaping that would
mean leaking the directory, which is what the previous attempt was penalised for;
1.9 s is still ~47× inside CI's 90 s per-binary bound.

## 7. Review fixes

### Attempt 4 (review pass — one line per MINOR → change)

1. **`remove_dir_all` ran outside the `STUBS` lock (`:429-430`).** The cache moved to a module-level `static STUBS: Mutex<Option<Stubs>>`; `StubRootGuard::drop` takes that same lock, deletes the root **and** empties the cache, so a thread entering `stubs()` can no longer stat a present root that is being deleted under its clone (cache and directory are one invariant, under one lock).
2. **Root's summary line contradicted itself (`:1681`).** The print is now `above the view {checked} map(s) content-checked, {denied} denied (EACCES), {gone} gone, of {children} children; euid {euid}` — counts, never a claim — and RESULT §1/§6 quote that line instead of `0 of 3 … non-dumpable (maps EACCES)`.
3. **No maps witness for http/scheduler/dbproxy as root (`:1651-1676`).** The view's pids now come from the `process.spawned` events, and **every** other child of init is content-asserted (`no libphp`, no shim); as root the assertion additionally requires that every above-view role was checked or gone, and unprivileged keeps the `EACCES` count. Proof: `unshare -r` → `above the view 2 map(s) content-checked, 0 denied (EACCES), 0 gone, of 3 children; euid 0`, 8/8.
4. **`LESSONS.md`** is the reviewer's (moved to `MB.3.36`'s paths); this suite no longer proposes it.

### Attempt 3

1. **[MAJOR] the fixture is rebuildable (`:402-408`).** `stubs()` was a
   `OnceLock`, so the last `StubRootGuard` deleted the root and every later stub
   test copied from deleted paths — deterministically red under
   `--test-threads=1`, which is how `ci.yml:359` runs every binary. It is now
   `Mutex<Option<Stubs>>` (a `Clone`d `Stubs` carrying its `root`) and rebuilds
   whenever the cached root is gone. Proof above: 8/8 serialized twice, two
   filtered single-test runs, 0 directories after each.
2. **[MINOR] root can read a non-dumpable role's maps (`:1588-1599`).** The
   per-child `EACCES`/content assertions and `denied > 0` are gated on
   `effective_uid()` (parsed from `/proc/self/status`, no `libc`), with the reason
   in the test doc. Proof: the `unshare -r` run above, 8/8.
3. **[MINOR] the swap failure message named only the race (`:1501-1508`).** It
   now says a **removed inode pin** produces exactly the same four served
   attempts, and points at `php_view::boot_bindings`/`open_pinned` as the thing to
   check before dismissing it as harness noise.
4. **[MINOR] the 250 ms leg never checked the probe was gone (`:1389`).** After
   the refusal the test waits up to 2 s and asserts `/proc/<probe pid>` is
   absent, so a boot that refuses but leaves its `pause()` child behind now
   fails. Evidence: `killed at BLASTIT_PROBE_TIMEOUT_MS=250 after 273.6ms; probe
   pid … pgid … uid … seccomp EPERM, gone from /proc`.

### Attempt 2

1. **Non-library canary was unfalsifiable.** Removed: the file is text and has no
   constructor, so nothing could have written the canaries it checked. The ZTS
   leg is named in the code and the module doc as the zero-`dlopen` proof.
2. **`Drop for Stubs` never ran** (`OnceLock` statics are not dropped), so the
   root leaked (76 dirs cleaned at that attempt's start). `StubRootGuard` counts
   live tests and the last guard — dropped by the test's frame on a pass *or* a
   panic — removes the root.
3. **"Same slot" was uid equality,** one constant in single-UID `dev`. Replaced
   with the respawn's `PPid` = init, the crashed pid gone, and exactly one live
   worker of the route (`live=[…]`).
4. **The inode swap is a stderr-timing race:** the retry is bounded (4 attempts /
   20 s) and retries only a *lost race*; a stage 0 that never announces, or a
   refusal with another sentence, panics at once. Every run so far landed on
   attempt 1.
5. **CI only exercises the refusal branch.** Follow-up for the CI owner:
   `.github/workflows/php-sapi.yml`'s `shim` job is the only job whose image has
   PHP **and** a built shim (`php8.4-dev` + `libphp8.4-embed` + `make
   php-sapi-e2e`); adding `make php-sapi && cargo test -p blastit --locked
   --test critical_php_embed` after its shim step would run both real-pair legs
   there. **Still uncovered on every CI run:** both containment legs (the
   `exit()`/fatal `cpu_ns` accounting and the slot respawn) and the maps test's
   *unprivileged* half (`EACCES` on a role above the view) are proved only on a
   developer host with `libphp-embed` + `make php-sapi`; `make php-sapi-e2e`
   never executes this suite. The maps test's *content* half does run in CI, as
   root (`ci.yml:359`).

## Follow-ups outside this node's `paths`

- **`MB.3.36` shipped the ADR §3 probe child** (it landed as a sibling node while
  this attempt was open), so the shim-minor and probe-timeout refusals are now
  boot refusals with §4.1 sentences; the suite asserts them and no longer needs
  the "first request" arm. If any other leg still reads a boot's exit status, it
  should be checked against the probe.
- **A contained bailout is reported `status: 500` with `outcome: "ok"`** (the
  shim's contained-fatal 500 head marks the request complete). The suite asserts
  the status (5xx, never 200) and the honest `view_died`/502 path appears in the
  probe-refusal legs; a `scheduler`/`view` decision on the `outcome` label is
  still worth making.
- **`R19`'s "crashes charge `ceiling`" is not reached for a contained bailout:**
  the worker reports `RESP_END_COST`, so the scheduler charges the reported wall
  (`cost_ns` ≈ served wall, `>= cpu_ns`). Asserted as observable, recorded here.
- **`docs/knowledge/LESSONS.md`** (not this node's `paths`) candidates:
  (a) forked `blastit dev` roles keep `PR_SET_DUMPABLE(0)`, so `/proc/<pid>/maps`
  is `EACCES` even to the same uid — only init's maps are readable, so a "no
  `libphp` above the worker" test needs a stub-ELF canary to place the load;
  (b) `/proc/<pid>/status` *is* still readable, which is how a worker is tied to
  the boot that forked it; (c) two test fixtures sharing one scratch name under
  one pid delete each other's `blastit.toml`, so the boot reports a missing
  manifest; (d) a `OnceLock` fixture needs an explicit guard — a `Drop` impl on
  the value never runs and leaks its temp directory.
