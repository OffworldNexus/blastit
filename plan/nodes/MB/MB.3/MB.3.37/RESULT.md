# Result MB.3.37

Protected oracle gaps in `critical_php_embed`: shim inode pin, `egress = true`
twin, `cpu_ns` vs wall, POST body.

**Status: done.** Four legs added, all four mutation controls run red→restored
green, `make check` exit `0`, guard authorized. Only
`crates/blastit/tests/critical_php_embed.rs` is modified
(`git status --short crates/` → ` M crates/blastit/tests/critical_php_embed.rs`).

## What was added

Additions only: `+620 / −18` in the protected file. The 18 deletions are one
reworded `Swap` doc line and the hang leg's inline shape-file parse, moved
**verbatim** into a shared `probe_shape()` helper (no assertion changed, none
deleted, no `#[ignore]`, no skip construct — `make critical-skip-check` prints
`protected critical tests hold no skip construct`).

| Leg | Rule it pins | Gap closed |
|---|---|---|
| `a_shim_replaced_between_the_stages_refuses_the_boot` | the **shim**'s half of the `(path, st_dev, st_ino)` pin (`ADR-0010` §3, `R7`) | G1 |
| `an_egress_route_lets_the_probe_open_a_socket` | `egress = true` reaches the probe's `SeccompRole::EmbeddedView` filter (`ADR-0010` §3, §7.3) | G2 |
| `a_blocking_script_is_charged_cpu_time_not_wall_time` | `cpu_ns` is `CLOCK_PROCESS_CPUTIME_ID`, not the request's duration (`ADR-0010` §1.1, `R19`) | G3 |
| `a_form_body_reaches_the_front_controller` | a request body reaches `$_POST`; `max_body_bytes` caps it (`R12`/`R14`/`R15`) | G4 |

Supporting additions (no behaviour change to existing legs):

* `Boot::post(path, content_type, body)` — a POST over a fresh connection. A
  write error is tolerated because a `413` can be answered before the client
  finishes sending; the response is asserted either way.
* `probe_shape(&Stubs, all) -> (String, BTreeMap)` — the shape-file parse the
  hang leg already had, now shared with its `egress` twin so the `pid`/`ppid`/
  `pgid`/`uid`/`euid`/`inet` names cannot drift between two stubs.
* `hang_shape_lock()` — `HANG_SHIM_STUB_C` bakes **one** `BT_SHAPE` path, so the
  two probe legs share one file. `Boot`'s lock does not cover it (it is taken
  after the `clear` and released after the read), so the two legs serialize on
  their own poisoned-proof mutex. The hang leg gains one
  `let _shape_guard = hang_shape_lock();` line; nothing else in it changes.
* Constants: `BLOCK_MS = 200`, `CPU_WALL_SHARE = 4`, `BLOCKING_PHP`,
  `FORM_PHP`, `FORM_MANIFEST`, `FORM_BODY_MAX = 4096`, `EGRESS_MANIFEST`
  (`PHP_MANIFEST` + `egress = true`), and leg 2 reusing `HANG_SHIM_STUB_C` — no
  second stub was invented.

Leg 2 reuses `stubs.hang_shim` / `stubs.hang_shape` verbatim, so the
`pid`/`ppid`/`pgid`/`uid`/`euid` assertions live in one place; only `inet`
differs (`0` for egress, `1` for the denied twin).

## Acceptance

### `cargo test -p blastit --locked --test critical_php_embed`

The node's sample output says `12 passed`; that count is **stale** — the suite
has **14** tests at `55a3380` and **18** now. Nothing was deleted to match a
number.

```console
$ cargo test -p blastit --locked --test critical_php_embed
running 18 tests
test a_shim_replaced_between_the_stages_refuses_the_boot ... ok
test an_egress_route_lets_the_probe_open_a_socket ... ok
test a_blocking_script_is_charged_cpu_time_not_wall_time ... ok
test a_form_body_reaches_the_front_controller ... ok
…
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.55s
```

This host has the real pair (`/usr/lib/libphp8.4.so`, `php-config 8.4.27`,
`target/php-sapi/libblastit_php84-x86_64-gnu.so`), so legs 3 and 4 ran **for
real**, not through the refusal branch. Their `--nocapture` evidence:

```console
$ cargo test … -- a_blocking_script --nocapture
blocked 200 ms in usleep: status 200  cpu_ns=124912  wall_ns=214928557  cpu/wall=1/1720  answered in 216.736541ms

$ cargo test … -- a_form_body --nocapture
form body reached `$_POST` (x=1, 200); a 8194-byte body was refused with
`HTTP/1.1 413 Content Too Large`; the slot still answered x=2
```

Leg 1 and leg 2 need no host PHP; their `--nocapture` evidence:

```console
$ cargo test … -- a_shim_replaced --nocapture
refused (stage 1 shim inode pin) on attempt 1:
… "line":"route \"app\": shim /tmp/blastit-php-embed-…-swap-shim/libblastit_php84-x86_64-gnu.so
is not the file stage 0 inspected (st_dev/st_ino moved), so the worker would load something
nobody verified; restore the package and restart blastit"

$ cargo test … -- an_egress_route --nocapture
killed at BLASTIT_PROBE_TIMEOUT_MS=250 after 477.274097ms; probe pid 3592910 ppid 3592907
pgid 3592910 uid 1000 egress socket allowed (inet 0), gone from /proc
```

### Mutation controls (each red, then restored green)

**(a) drop the shim comparison in `php_view::open_pinned`** — mutated to
`if what == "libphp" && (…)` in a working copy, then restored from a saved copy.

```console
$ cargo test … -- a_shim_replaced            # mutated
thread … panicked at crates/blastit/tests/critical_php_embed.rs:2009:5:
the stage-0→stage-1 window was missed in 4 attempt(s) inside 20 s: … BUT a **removed inode pin**
produces exactly these same served attempts, so do not dismiss this as harness noise before
checking that stage 1 still compares the re-opened **shim**'s `(st_dev, st_ino)` …
test result: FAILED. 0 passed; 1 failed; … finished in 0.52s

$ cargo test … -- a_libphp_replaced          # mutated — the old legs stay green
test result: ok. 1 passed; …

$ cp /tmp/opencode/php_view.rs.orig crates/blastit/src/php_view.rs
$ git status --short crates/
 M crates/blastit/tests/critical_php_embed.rs

$ cargo test … -- a_shim_replaced            # restored
test result: ok. 1 passed; …
```

**(b) pass `false` instead of the route's egress in `probe_bindings`** (`let
egress = false;` keeping the old expression alive as a `let _ =`).

```console
$ cargo test … -- an_egress_route            # mutated
thread … panicked at crates/blastit/tests/critical_php_embed.rs:1736:5:
assertion `left == right` failed: `egress = true` must let the probe child open an `AF_INET`
socket (0) …
probe pid=3598861 ppid=3598857 pgid=3598861 uid=1000 euid=1000 inet=1
  left: 1
test result: FAILED. 0 passed; 1 failed; … finished in 0.50s

$ cargo test … -- a_probe_that_hangs        # mutated — the denied twin stays green
test result: ok. 1 passed; …

$ cp /tmp/opencode/php_view.rs.orig crates/blastit/src/php_view.rs   # restored
$ cargo test … -- an_egress_route
test result: ok. 1 passed; …
```

**(c) make the shim report the request's wall clock as `cpu_ns`** — the C source
was **copied** to `/tmp/opencode/mut-shim/`, `CLOCK_PROCESS_CPUTIME_ID` →
`CLOCK_MONOTONIC` in the copy, and rebuilt with the Makefile's own flags. **The
repository was never touched** (`git diff --stat` unchanged throughout), so the
"restored" run is the unmutated build with no env override.

```console
$ cc -O2 -D_GNU_SOURCE -fPIC -shared -Wall -Wextra -Werror \
     -Wl,-soname,libblastit_php84-x86_64-gnu.so -Icrates/blastit-php-sapi/c/include \
     $(php-config --includes) -o /tmp/opencode/mut-shim/libblastit_php84-x86_64-gnu.so \
     /tmp/opencode/mut-shim/blastit_php_sapi.c

$ BLASTIT_PHP_SHIM_DIR=/tmp/opencode/mut-shim cargo test … -- a_blocking_script --nocapture
thread … panicked at crates/blastit/tests/critical_php_embed.rs:2459:5:
`cpu_ns` (200502401 ns) must be under a 4th of the wall time (215034319 ns) for a 200 ms
`usleep`: … {"cpu_ns":200502401,"duration_ms":215.034319,"status":200,"outcome":"ok", …}
test result: FAILED. 0 passed; 1 failed; … finished in 0.38s

$ BLASTIT_PHP_SHIM_DIR=/tmp/opencode/mut-shim cargo test …      # whole suite, mutated
test a_blocking_script_is_charged_cpu_time_not_wall_time ... FAILED
test result: FAILED. 16 passed; 2 failed; …   # the other is MB.3.38's sleeping-bailout leg, same cause

$ git diff --stat crates/blastit/src crates/blastit-php-sapi   # nothing
$ cargo test … -- a_blocking_script --nocapture                # unmutated shim
blocked 200 ms in usleep: status 200  cpu_ns=124912  wall_ns=214928557  cpu/wall=1/1720 …
test result: ok. 1 passed; …
```

**(d) drop the body from `RequestV1`** — `Prepared::view` set to
`content_length: 0`, `body: std::ptr::null()`, `body_len: 0`; restored after.

```console
$ cargo test … -- a_form_body --nocapture    # mutated
thread … panicked at crates/blastit/tests/critical_php_embed.rs:2529:5:
the body did not reach `$_POST`: the front controller echoed
"HTTP/1.1 200 OK …\r\n9\r\nx=absent\n\r\n0\r\n\r\n"
test result: FAILED. 0 passed; 1 failed; … finished in 0.18s

$ cargo test …                                # whole suite, mutated
test a_form_body_reaches_the_front_controller ... FAILED
test result: FAILED. 17 passed; 1 failed; …

$ cp /tmp/opencode/view_php.rs.orig crates/blastit-view/src/php.rs   # restored
$ git status --short crates/
 M crates/blastit/tests/critical_php_embed.rs

$ cargo test … -- a_form_body
test result: ok. 1 passed; …
```

### Serialized runs, and 0 directories left behind

```console
$ cargo test -p blastit --locked --test critical_php_embed -- --test-threads=1   # 1/2
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.26s
$ cargo test -p blastit --locked --test critical_php_embed -- --test-threads=1   # 2/2
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.31s
$ ls -d /tmp/blastit-php-embed-* | wc -l
0
```

(The two new legs are safe under `--test-threads=1` **because** of
`hang_shape_lock`: the shared stub root is rebuilt when a serialized run removes
it, and the shape file is now serialized too.)

`$HOME` also holds **10** stale `blastit-php-provenance-*` directories, all with
pids `3391323`–`3492583` and mtimes `02:33`–`02:46`, i.e. **before this session
started** (`3.59M+`): they are leaked by earlier *panicking* runs of MB.3.40's
provenance legs and are not this node's (see Follow-ups).

### PHP-free host (`unshare -m -r` + `mount --bind /dev/null /usr/lib/libphp8.4.so`)

`unshare` is available here (`/usr/bin/unshare`); the bind mount turns the
library into a character device, so `is_file()` is false for every candidate
name, symlinks included.

```console
$ unshare -m -r sh -c 'mount --bind /dev/null /usr/lib/libphp8.4.so && \
    cargo test -p blastit --locked --test critical_php_embed'
running 18 tests
… all 18 ok …
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.88s
```

With `--nocapture` the two real-pair legs prove they took the §4.1 branch rather
than silently asserting nothing:

```console
test a_form_body_reaches_the_front_controller ... ok
test a_blocking_script_is_charged_cpu_time_not_wall_time ... ok
refused (no usable libphp + shim on this host): blastit dev: route "app": [route.php] entry
"public/index.php": no usable NTS libphp was found (searched BLASTIT_LIBPHP, then libphp8.4.so,
… libphp.so in /usr/lib/x86_64-linux-gnu, /usr/lib64, /usr/lib, /usr/local/lib/x86_64-linux-gnu,
/usr/local/lib); install the PHP embed package — … or set BLASTIT_LIBPHP=/path/to/libphp.so
```

### `uv run --no-project tools/critical_guard.py --base 55a3380 --node MB.3.37`

```console
critical-guard: 55a3380..working tree, 5 changed file(s)
node MB.3.37: paths ['crates/blastit/tests/critical_php_embed.rs']
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/critical_php_embed.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.37
exit=0
```

`make check` runs the guard without `--node`, so it prints
`[VIOLATION: not covered by any node path]` + `report only: 1 breaking
change(s) would fail an enforced run` for any *uncommitted* protected change —
that is the report-only mode, not a failure (`make check` exits `0`), and the
enforced form is the run above. It disappears the moment the node is committed.

### `make check` and clippy

```console
$ make check
… guard: OK … critical-guard (report only) … cargo fmt --all -- --check …
PYTHONHOME=… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
… cargo test --workspace --all-targets … test result: ok. 18 passed (critical_php_embed) …
… plan OK …
real 1m57.428s
$ echo $?
0

$ make rs-lint        # the node's clippy command
cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.74s
exit=0
```

`make check` also runs the suite under `--features python` (18 passed) and in the
`no-default-features`/standalone configurations (18 passed each), where the
real-pair legs take the refusal branch.

### `make rs-test-php-image` (what CI runs) — **red, for an unrelated fixture**

```console
$ make rs-test-php-image
test a_shim_replaced_between_the_stages_refuses_the_boot ... ok
test an_egress_route_lets_the_probe_open_a_socket ... ok
test a_blocking_script_is_charged_cpu_time_not_wall_time ... ok
blocked 200 ms in usleep: status 200  cpu_ns=115659  wall_ns=216073202  cpu/wall=1/1868
test a_form_body_reaches_the_front_controller ... ok
form body reached `$_POST` (x=1, 200); a 8194-byte body was refused with
`HTTP/1.1 413 Content Too Large`; the slot still answered x=2
test a_runpath_into_a_writable_directory_refuses_a_production_boot ... FAILED
test a_world_writable_manifest_root_refuses_a_production_boot ... FAILED
test a_world_writable_shim_directory_refuses_a_production_boot ... FAILED
test the_same_tree_owned_by_init_is_accepted ... FAILED
no directory on this host is one uid's alone (tried ): a deployment boot could not run on this
host either, so the accepted twin has nothing to accept
test result: FAILED. 14 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.56s
exit=2
```

**The image is not green, and this node does not claim it is.** All four
failures are MB.3.40's provenance legs panicking inside `protected_root`
(`tried ` — an empty candidate list: the image runs the pre-built binaries
without cargo, so `CARGO_TARGET_TMPDIR` is unset and `HOME`/`/opt`/`/usr/local`/
`/srv` are not creatable by the runner uid). That is MB.3.43's fixture, not this
node's, and nothing under `protected_root`/`Scratch`/the MB.3.40 legs was
touched here (see the dispatch constraint). The node's own four legs — and the
sibling `critical_php_backend` suite (5 passed) — are green in the image, so the
real-pair witness this node wanted is `rs-test-php-image`'s MB.3.37 rows; a
green image run is a later node's witness.

## Notes / Follow-ups

* **One flake, seen once, not from this node.** The very first baseline run of
  the unmodified suite (`55a3380`, before any edit) failed
  `a_non_library_blastit_libphp_refuses_before_anything_loads` at
  `await_boot()`: the boot had refused, but `TcpStream::connect` to the
  hand-picked ephemeral port succeeded, i.e. *something else on this shared host*
  held the port. It passes standalone and passed in every run afterwards
  (parallel, serialized ×2, PHP-free, image, `make check`). `Boot` serializes
  boots inside one binary but cannot reserve a port against other processes.
  Out of scope here (`Scratch`/`Boot` are the harness, not the four legs) —
  recorded for the harness owner.
* **10 stale `$HOME/blastit-php-provenance-*` directories** (pids `3.39M`–`3.49M`,
  `02:33`–`02:46`) left by earlier panicking runs of MB.3.40's legs. Cleaning
  them is not this node's path; MB.3.43's fixture work is where a `Drop`-based
  cleanup belongs.
* `probe_bindings`' `egress.get(slot).copied().unwrap_or(false)` is now **pinned
  in the protected tier** from both directions, but the `unwrap_or(false)`
  fallback itself is still silent for a *short* `egress` vector (unreachable
  today by construction). A future node could make it a refusal; no test can
  reach it without a `src/` scenario seam.
* `a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time` also fails
  under mutation (c) (its `cpu_ns × 20 <= wall_ns` bound) — a welcome side
  effect, not a change to that leg.
