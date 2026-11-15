# MB.3.48 — the two PHP suites are deterministic (one flake fixed, one diagnosed
# as a lost `DONE` and left to `MB.3.45`)

## Summary

Both PHP suites are now deterministic **as harnesses**. Flake 1 is fixed twice over:
the refusal text is no longer read before it is written, and "the port accepts" is no
longer taken for "this boot served". Flake 2 is **reproduced, diagnosed, and not
touched**: it is a lost `DONE` in `crates/blastit-scheduler/src/scheduler.rs`, i.e.
`MB.3.45`'s seam, exactly as this node instructed.

`crates/blastit/tests/critical_php_backend.rs`, `crates/blastit/tests/critical_php_embed.rs`
only. `crates/blastit/src/**`, `crates/blastit-scheduler/**`, `.github/workflows/**`
and the `Makefile` are untouched.

## 1. Flake 1 — reproduced, two races, both fixed by a handshake

### 1.1 Reproduction (the same message the gate printed)

The gate's red (`/tmp/opencode/mc8.log:344-356`, `make check` exit 2) was
`critical_php_backend.rs:477`, `not a library: the route is named: ` and nothing after
the colon — `assert_package_sentence` reading an **empty** stderr from a boot that had
exited `1` with an empty stdout.

Reproduced by looping the suite the way `make check` runs it (the `python`-feature
binary, `--workspace --all-targets`'s feature set) under scheduler pressure: 24 copies
of the *whole file* at once, pinned to 4 CPUs, 50 rounds = **1200 suite runs**.

```console
$ ./loop: 50 rounds x 24 copies of `critical_php_backend` on taskset -c 0-3
FAIL round=36 copy=9
$ cat /tmp/opencode/mb348/p2/r36-9.log
running 5 tests
test a_blastit_libphp_that_is_not_a_library_refuses_the_boot ... FAILED
...
thread 'a_blastit_libphp_that_is_not_a_library_refuses_the_boot' (542167) panicked at crates/blastit/tests/critical_php_backend.rs:477:5:
not a library: the route is named:
test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.25s
```

Byte-for-byte the gate's message. The test binary used is preserved at
`/tmp/opencode/mb348/prefix-python-binary`.

### 1.2 Reason: reaping the child says nothing about the pipe readers

`Dev::await_boot` returned on `child.try_wait()` and the caller then read
`stderr()`/`drain_stdout()` — snapshots of a `Mutex<String>` and an `mpsc::Receiver`
filled by two **independent reader threads**. `_exit(1)` after `eprintln!`
(`stage0.rs:1974-1978`) puts the sentence in the pipe, but the parent's `waitpid`
wakeup and the reader thread's `read(2)` wakeup are unrelated: under load the parent
wins, and the refusal reads as the empty string. Nothing in the harness ordered those
two events, so no loop count could make it safe — only a handshake could.

### 1.3 A second race, found by the loop after the reader fix

Fixing only the readers left 1 red in the same 1200 runs, with a *different* message:

```console
FAIL round=26 copy=15
thread '...' panicked at crates/blastit/tests/critical_php_backend.rs:820:10:
a boot with a non-library BLASTIT_LIBPHP must refuse, not serve
```

`await_boot` had returned `None` ("served") for a boot that refuses **before the
listener**. `Dev::start` probes a free port by binding and releasing it; a *foreign*
listener — the other copies of this very suite — took that port in the window, and
`TcpStream::connect` succeeded against a socket this boot never opened. That is the
same shape `MB.3.40`'s follow-up 4 recorded for `critical_php_embed` (1 run in ~12).

### 1.4 Fix: the boot answers for itself, and its text is waited for

`crates/blastit/tests/critical_php_backend.rs`

- **`Dev::await_readers`** (new, called from `await_boot`'s exit branch): each reader
  thread sends `bool` (clean end of file / read error) on a channel when its
  `BufReader::lines()` loop ends, and the exit path waits for **both** before returning.
  The wait is bounded by the existing `BOOT_TIMEOUT` and cannot panic spuriously: a
  reaped child has closed its descriptors. A read error or a pipe still held open after
  the boot is gone is a named failure, not a partial snapshot. No retry, no sleep, no
  new constant.
- **`Dev::await_boot` now partitions on the boot's own `ready` line**, not on the port:
  `refused` = exited, `served` = the `ready` event/line that init emits *after* the
  listener is bound and the roles are forked (`is_ready`, both log formats). The
  ephemeral-port probe is no longer part of any verdict, so a stolen port cannot flip
  it. The three refusal legs, the dangling-link leg and the `(None, None)` arm are all
  exact now. The human format is the one the refusal legs (and that `(None, None)` arm)
  run in, and it is a real line from a served boot:
  `04:03:28.442 INFO  ready       serving  routes=1  workers=16`.
- **`Dev::stdout_seen`** keeps every line the wait consumed, so `drain_stdout` still
  returns *everything* the boot wrote — without it, waiting for `ready` would have made
  the refusal legs' `assert_eq!(stdout, "")` tautological. `request_event` scans those
  lines first (`request_event_in`), so the two sources are one event stream.

`crates/blastit/tests/critical_php_embed.rs`

- **`Boot::ready`** (new) + `Boot::await_boot` partition on `ready` instead of
  `TcpStream::connect`, closing `MB.3.40`'s follow-up 4 (that suite already joined its
  readers in `stop()`, so it never had flake 1's shape).

### 1.5 Loops after the fix — zero retries

| loop | pre-fix | post-fix |
|---|---|---|
| `critical_php_backend`, 24 copies x 50 rounds (1200 suite runs, 6000 boots), `python` build, `taskset -c 0-3` | 1 red (the gate's message) | **0** |
| `critical_php_embed`, 50 sequential runs, PHP-free host, `python` build | — | **0** |
| `critical_php_embed` + `critical_php_backend` in the `php-sapi-e2e` image (real pair), 50 sequential runs + 2 `make rs-test-php-image` | — | **0** (see §2 for the one *other* red) |

```console
$ (50 x 24) x taskset -c 0-3 critical_php_backend-98e99394a636fd05
fails=0 of 1200 suite runs
$ (50 x) critical_php_embed-499a3d8ce0151128
fails=0 of 50 embed suite runs (PHP-free host)
```

## 2. Flake 2 — reproduced under load, diagnosed as a lost `DONE` → `MB.3.45`

Not the harness, not the fixture, not the charge arithmetic: **the scheduler fails a
request as `view_died` because init's `Ctl::Exited` reaches the scheduler before the
`view.chan` frames the view already wrote**, so `DONE{Bailed}` is never accounted.

### 2.1 Reproduction (post-fix tree, real pair, `--nocapture`)

1 run in 20 of the whole embed suite in the `php-sapi-e2e` image, with 8 host spinners
(the load the original 1-in-7 was observed under), `v=MB.3.48`:

```console
$ docker run ... blastit-php-sapi-e2e:local .../critical_php_embed-0b784173b4fc9736 --nocapture
FAIL run 17 exit 101
thread 'a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time' panicked at
  crates/blastit/tests/critical_php_embed.rs:2315:5:
assertion `left == right` failed: usleep(): a 502 contained bailout is a handler that
  never returned:
{"bytes_in":0,"bytes_out":12,"component":"scheduler","cost_ns":0,"cpu_ns":null,
 "duration_ms":90.766983,"event":"request","level":"warn","method":"GET",
 "outcome":"view_died","path":"/crash","queued_ms":26.665633,
 "remote_addr":"127.0.0.1","request_id":"0000000000000001","route":"app","rule":0,
 "status":502,"ts":"2026-11-15T04:18:04.581Z"}
```

`queued_ms: 26.67` under the original's `26.35`, `cost_ns: 0`, `cpu_ns: null`,
`bytes_out: 12` — the length of `fail_request`'s canned `"bad gateway\n"`. This run is
the *no-head-yet* variant (502); the gate's was the *head-sent* variant (500, a
`RESP_ABORT` after the shim's 500 head). Both are the same `fail_request` call.

### 2.2 The trace (read-only; `crates/blastit/src/**` and the scheduler are out of scope)

1. `crates/blastit/src/supervise.rs:805` — `view_exit` reaps the view and immediately
   pushes `Ctl::Exited` on **ctl.chan**. It knows nothing about the view's own channel.
2. `crates/blastit-scheduler/src/scheduler.rs:1078` — `Ctl::Exited` → `view_gone`.
3. `scheduler.rs:3326` — `view_gone` fails every request still dispatched to that key:
   `fail_request(stream_id, Outcome::ViewDied, 502)`.
4. `scheduler.rs:3524` — `fail_request` with no head sent answers the client itself
   (`bytes_out: 12`, status 502) and **emits before charging** → `cost_ns: 0`,
   `cpu_ns: null` (`MB.3.45` item 4's own observation, seen from the request side).
5. The view had already written `RESP_HEAD(500)`, `RESP_END_COST` and `DONE{Bailed}`
   (`crates/blastit-view/src/php.rs:445-465`: `DONE` then `recycle()` → the process
   exits *by design* after a bailout), but they are on **view.chan**, a different input
   of the same single-threaded loop. They arrive after `view_gone` has already
   completed the request and are counted as dropped.
6. `ViewIn::Closed` (`scheduler.rs:1040`) *is* ordered after every frame on that
   channel — which is why "defer `Ctl::Exited`'s failure to `ViewIn::Closed` (bounded by
   `request_timeout`)" is a scheduler-only fix.

### 2.3 Why this is `MB.3.45`'s and not this node's

- The fix is in `crates/blastit-scheduler/src/scheduler.rs` (or `supervise.rs`), neither
  of which this node may touch, and `MB.3.45` (queued next) owns exactly this accounting
  seam — a `DONE` that is never accounted is the hole item 3 describes, and item 4's
  "emit before charge" is how the hole becomes `cost_ns: 0`.
- Papering over it here would mean accepting `view_died` for a handler that bailed — the
  assertion `MB.3.41` added on purpose — or `cost_ns: 0` as "charged its wall time".
  Both would be weakenings of a protected test. **The assertion is unchanged**
  (renumbered `:2294` → `:2314-2317` by this node's doc lines, not reworded).
- **Consequence for the gate:** the required `php-sapi` job can still go red at random
  (1 in 20 here, under load) until `MB.3.45` lands. That red is now a known, named
  product race rather than a mystery.

## 3. The two `//!` corrections (doc-only)

- `critical_php_embed.rs:70`: "(plus the sibling suite's served leg)" → both pair-gated
  legs of `critical_php_backend` named:
  `a_php_route_is_served_or_the_boot_refuses_naming_the_package` and
  `a_symlinked_blastit_libphp_is_resolved_before_it_is_served` (`:628` and `:800`).
- `critical_php_backend.rs:34-38`: "…this suite proves the composition root wires the
  worker on a host that builds it" (false since `MB.3.39`) → the packaged library is
  `make php-sapi-e2e`'s proof, `make rs-test-php-image` runs **this** suite inside that
  image against its `libphp8.4.so` and shim, so the `php-sapi` job proves the
  composition root and `make check`'s PHP-free run is the refusal half.

## Evidence

### `cargo test -p blastit --locked --test critical_php_backend`

```console
running 5 tests
test check_reaches_the_same_discovery_gate ... ok
test a_blastit_libphp_that_is_not_a_library_refuses_the_boot ... ok
test a_static_only_manifest_is_not_refused_by_this_gate ... ok
test a_php_route_is_served_or_the_boot_refuses_naming_the_package ... ok
test a_symlinked_blastit_libphp_is_resolved_before_it_is_served ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.28s
```

### `cargo test -p blastit --locked --test critical_php_embed`

```console
running 19 tests
test a_non_library_blastit_libphp_refuses_before_anything_loads ... ok
test a_dev_boot_of_a_world_writable_tree_still_serves ... ok
...
test an_exit_is_contained_and_the_next_request_is_served ... ok
test no_libphp_is_mapped_above_the_view_worker ... ok
test an_egress_route_lets_the_probe_open_a_socket ... ok

test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.54s
```

### `make rs-test-php-image` (real pair, the two suites in the image)

```console
$ make rs-test-php-image; echo exit=$?
docker run blastit-php-sapi-e2e:local .../critical_php_backend-3b9e55e09c71c369 --nocapture
symlinked /tmp/blastit-php-backend-1-symlink/libphp8.so -> /usr/lib/libphp8.4.so  served: HTTP/1.1 200 OK
served: HTTP/1.1 200 OK  cpu_ns=17955474  wall_ns=36388225  (libphp /usr/lib/libphp8.4.so)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
docker run blastit-php-sapi-e2e:local .../critical_php_embed-0b784173b4fc9736 --nocapture
usleep(): status 500  outcome=bailed  cpu_ns=119280  wall_ns=75991886  cost_ns=75991887  retired pid 130  respawned pid 140 (ppid 100) uid 1000  live=[140]
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
exit=0
```

Both served legs of the backend suite pass under the new `ready` handshake, and the
`ready`-based `await_boot` was exercised on every real-pair leg (no 20 s timeout).

### `make check`

```console
$ make check; echo exit=$?
...
clippy      -p blastit{,-view} (standalone)
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

(`real 1m20.733s`; log `/tmp/opencode/mb348/make-check.log`, re-run after the last
doc-only edit: exit 0, `/tmp/opencode/mb348/make-check.final.log`.)

### `make critical-skip-check` and the protected guard

```console
$ make critical-skip-check
protected critical tests hold no skip construct
$ uv run --no-project tools/critical_guard.py --base 5710182 --node MB.3.48
critical-guard: 5710182..working tree, 4 changed file(s)
node MB.3.48: paths ['crates/blastit/tests/critical_php_embed.rs', 'crates/blastit/tests/critical_php_backend.rs']
  M  crates/blastit/tests/critical_php_backend.rs  [authorized by crates/blastit/tests/critical_php_backend.rs]
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/critical_php_embed.rs]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.3.48
```

`cargo fmt --all -- --check` is clean. Both files' assertions are unchanged except the
two documented `//!` corrections; no `#[ignore]`, no skip, no added `sleep`, no new
timeout constant (the reader wait reuses `BOOT_TIMEOUT`).

## Follow-ups (outside this node's `paths`)

1. **`MB.3.45` owns flake 2** (§2): on `Ctl::Exited`, do not fail a view's in-flight
   requests before its `view.chan` reaches `ViewIn::Closed` (bounded by
   `request_timeout`), so a `DONE{Bailed}` written just before the process retired wins.
   A test for it needs the `DONE`-then-exit ordering pinned, not a load loop.
2. **`docs/knowledge/LESSONS.md`** deserves two lines this node could not add (not in
   `paths`): (a) "reaping a child says nothing about its pipe readers: wait for the
   readers' end of file before asserting on the text, or a refusal reads as an empty
   string" (`MB.3.48`, 1 in 1200 under 24-way pressure); (b) "`/proc/<pid>/fd` is
   `EACCES` for a role that set `PR_SET_DUMPABLE(0)`, so a test cannot prove port
   ownership through `/proc` — ask the boot for `ready` instead".
3. **`MB.3.40`'s follow-up 4 is now closed** for both suites (the port hand-off is no
   longer part of any verdict); its text can be marked done when the plan is next
   groomed.
