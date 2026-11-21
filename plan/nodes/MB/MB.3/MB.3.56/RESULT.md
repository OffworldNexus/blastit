# RESULT MB.3.56 (attempt 2)

**What is pinned, and when it is checked:** the boot records the resolved
`[route.php] entry`'s `(st_dev, st_ino)`, `st_size` and `st_ctime`, and holds an
`O_PATH` descriptor on that inode; every PHP worker compares the whole record,
at the start of every request, before the script is handed to `libphp` — and
answers an empty `500` (route, entry and fix on stderr) for a file that was
**replaced** *or* **rewritten in place**. Never a silent re-resolution.

## What changed in attempt 2 (the review's MAJOR and both MINORs)

| Finding | Change |
|---|---|
| MAJOR: identity-only pin ran an in-place rewrite | `EntryPin { dev, ino, size, ctime_s, ctime_ns }` recorded by `pin_entry` (`fstat` of the descriptor that is kept) and compared whole in `verify_entry`; the refusal sentence distinguishes "another file" (inode moved) from "rewritten in place" (`st_size`/`st_ctime` moved). |
| MAJOR: a freed inode number can be handed back | `PhpBinding.entry_fd`, an `O_PATH \| O_CLOEXEC` descriptor on the entry opened at boot, added to `pinned_fds()` so `ChildIo::keep` carries it into the probe and every worker: the inode cannot be freed while the boot pins it. |
| MAJOR: no witness for the shape that failed | Protected leg `an_entry_rewritten_in_place_is_refused_and_a_restart_serves_the_new_file` (same inode, **same size**, `IN-PLACE-DECOY-RAN`), plus a `php_view` unit twin; the e2e's rootless leg now rewrites with `cp` (the reviewer's act) and asserts refusal, 0 decoy opens, and that only a restart serves. |
| MINOR: "the instant before the script is handed to `libphp`" was false | Corrected (the reviewer's alternative): the check runs at the start of `serve`, **before** the body read, `Prepared::new` and `php_request_startup`; the remaining window is the client-paced body read plus startup, bounded by `max_body_bytes`/`request_timeout`, and the doc says so and why the record is not threaded into `PhpRuntimeConfig` (a runtime constructible without a pin would be a fail-open seam). |
| MINOR: `command_kind(" ".join(server.arguments))` was the constructor's vector | The php2shell `--learn` leg reads `/proc/<pid>/cmdline` of the live process and asserts `--dev-obs-fd=`, which stage 0 passes **only** for a `--learn` boot (`stage0::exec_stage1_dev`); the pid is stable across the re-exec. |

Accepted from attempt 1 and kept: pin placement, the descriptor stop-condition
analysis (no fd *as* `SCRIPT_FILENAME`; an `O_PATH` fd is now held beside the
record), the inside-decoy flip, the protected red for that shape, the four e2e
cleanups.

### One consequence, stated plainly

`st_ctime` moves on any write **and on any metadata change**, and it is not
user-settable (that is why it is in the record), so a `chmod`/`chown` of the entry
— like a deploy — also invalidates the pin until a restart. The containment legs
used to rewrite the entry between their two requests; that rewrite is now itself
a refusal, so their fixture became one page that crashes only on `/crash`
(`crash_page`, `EXIT_BODY`/`FATAL_BODY`/`SLEEP_BODY`). The assertions are
unchanged; the leg no longer depends on touching the pinned file, which is
stronger.

## Acceptance

### The IN-PLACE red (the review's exact measurement, on a live `blastit dev` route)

```console
$ sed -i 's/    if now != pin {/    if now.dev != pin.dev || now.ino != pin.ino {/' crates/blastit/src/php_view.rs   # attempt 1's comparison
$ cargo test -p blastit --locked --test critical_php_embed an_entry_rewritten
thread 'an_entry_rewritten_in_place_is_refused_and_a_restart_serves_the_new_file' panicked:
an entry rewritten in place must be refused, not served: HTTP/1.1 200 OK
Content-type: text/html; charset=UTF-8
Transfer-Encoding: chunked
Connection: close

13
IN-PLACE-DECOY-RAN

0

test result: FAILED. 0 passed; 1 failed; 0 measured; 20 filtered out

$ cargo test -p blastit --locked --features python --lib php_view::tests::an_entry_rewritten
a same-inode rewrite must be refused: ()
test result: FAILED. 0 passed; 1 failed; 0 measured; 226 filtered out
```

Restored (`if now != pin`), both green.

### The IN-PLACE green (both entry legs)

```console
$ cargo test -p blastit --locked --test critical_php_embed -- --nocapture a_decoy_inside an_entry_rewritten
a decoy inside manifest_dir is refused; the restored tree serves after a restart
  (HTTP/1.1 200 OK -> HTTP/1.1 500 Internal Server Error -> HTTP/1.1 200 OK)
test a_decoy_inside_the_manifest_dir_is_refused_and_a_restart_serves ... ok
an in-place rewrite is refused; the restored page serves after a restart
  (HTTP/1.1 200 OK -> HTTP/1.1 500 Internal Server Error -> HTTP/1.1 200 OK)
test an_entry_rewritten_in_place_is_refused_and_a_restart_serves_the_new_file ... ok
test result: ok. 2 passed; 0 failed
```

The in-place leg keeps the inode **and** `st_size` (a same-length decoy), asserts
the record moved in `st_ctime`, and the body never carries `IN-PLACE-DECOY-RAN`.

### `cargo test -p blastit --locked --test critical_php_embed`

```console
running 21 tests
test a_decoy_inside_the_manifest_dir_is_refused_and_a_restart_serves ... ok
test an_entry_rewritten_in_place_is_refused_and_a_restart_serves_the_new_file ... ok
test an_exit_is_contained_and_the_next_request_is_served ... ok
test a_fatal_is_contained_and_the_next_request_is_served ... ok
test a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time ... ok
...
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### `cargo test -p blastit --locked --features python`

```console
test result: ok. 225 passed; 0 failed; 2 ignored; ...   # lib unit tests
test result: ok. 21 passed; 0 failed; ...                # critical_php_embed
...
EXIT=0
```

### `make rs-test-php-image` (the image's `libphp8.4` + shim)

```console
test result: ok. 5 passed; ...                          # critical_php_backend
an in-place rewrite is refused; the restored page serves after a restart
  (HTTP/1.1 200 OK -> HTTP/1.1 500 Internal Server Error -> HTTP/1.1 200 OK)
test result: ok. 21 passed; ...                         # critical_php_embed
EXIT=0
```

### `make e2e` (rootless leg is the witness; wheel + `blastit-e2e-php` rebuilt)

```console
$ make e2e
...
  ok   a privileged harness makes the entry unreadable
  ok   `chmod 000` on the entry is refused for the route uid (500)
  ok   and the answer has zero body bytes (0)
  ok   and it stays refused until a restart (500)
  ok   a restart serves the restored entry
  ok   and serves again (200)
  ok   a privileged harness points `public/` at a decoy inside the manifest dir
  ok   a decoy inside `manifest_dir` is refused (500)
  ok   and the decoy's marker never reaches the client
  ok   the decoy recorded 0 opens (0)
  ok   and the worker names the entry and the fix in its log
  ok   the application's own inode serves again (200)
  ok   a privileged harness rewrites the entry in place (what `cp` does)
  ok   and the inode is unchanged ((110, 311783144) -> (110, 311783144))
  ok   an entry rewritten in place is refused (500)
  ok   and the in-place decoy never reaches the client
  ok   and no decoy marker at all
  ok   the decoy recorded 0 opens (0)
  ok   and the worker names the change and the fix in its log
  ok   restoring the page in place succeeds
  ok   and it stays refused until a restart (500)
  ok   a restart serves the page as it is now
  ok   and the restarted boot serves it (200)
  ok   still 0 decoy opens (0)
EXIT=0
```

411 `ok` lines, zero `FAIL` across the static, php2shell and rootless legs. The
php2shell `--learn` observable:

```console
PASS: the live boot ran as `--learn` (`--dev-obs-fd` in its argv:
  ['--stage=1', '--config-fd=4', '--dev-root=…', '--dev-host=127.0.0.1', '--dev-obs-fd=3'])
```

### `make check`, clippy, rustdoc, guards, ruff

```console
$ make check
guard: OK ... critical-guard: OK ... test result: ok. 21 passed ... protected
critical tests hold no skip construct ... default build: 0 pyo3 dependencies
uv run --no-project tools/plan.py validate
plan OK
EXIT=0

$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Finished `dev` profile ... (no warnings)

$ RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
    Finished `dev` profile ...

$ uv run --no-project tools/critical_guard.py --base 2dbb17e --node MB.3.56
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.56

$ make critical-skip-check
protected critical tests hold no skip construct

$ uvx --offline ruff check e2e/php2shell.py e2e/rootless/php_entry_swap.py e2e/php/entry_probe.py
All checks passed!
$ uvx --offline ruff format --check … (same three)
3 files already formatted
```

## The mechanism's edge, recorded

`st_ctime` is stamped from the kernel's **coarse** clock (one tick, ~1–4 ms), so a
same-size rewrite **inside the tick the boot stamped** is invisible to any
metadata record. A live swap happens after boot (the reviewer's `cp`, a deploy,
an attacker's write) and is caught; the two tests that rewrite same-size fixtures
sleep past a tick first, or they measure the tick rather than the pin
(`docs/knowledge/LESSONS.md`).

## Attempt 3 — owner requirements and reviewer MINORs (no behaviour change)

- **`docs/security.md` ("PHP front controllers"), the operator sentence:**
  > The front controller itself is pinned at boot by its inode, its size and its
  > status-change time, and the worker re-checks that record before every request:
  > a script that was replaced **or rewritten in place** is refused (`500`) until a
  > restart records the tree again. One residual: a **same-size** rewrite in place
  > that lands inside the same coarse-clock tick (1–4 ms) as the boot's own `stat`
  > is invisible to that record — and it takes write access to `manifest_dir`,
  > which production denies to every route uid.
- **`php_view.rs`, on the record itself, one line:** `st_size` catches a rewrite
  that changes length; `st_ctime` catches one that does not **unless it lands
  inside the same coarse-clock tick (~1–4 ms) as this record** — the documented
  residual, `docs/security.md` "PHP front controllers". So nobody simplifies the
  comparison away believing metadata is exact here.
- The end-to-end same-size witness now asserts the premise the refusal rests on —
  `assert_ne!((ctime, ctime_nsec), recorded)` — so on a filesystem whose tick is
  longer than the 20 ms sleep it reddens for the tick, not for the pin.
- The in-place leg's doc no longer claims an e2e `rm`-shaped leg: it names
  `php_view`'s `/proc/self/fd` readlink and the child-sequence test, which is where
  the descriptor half is actually asserted.
- The child-sequence test now reads `/proc/self/fd/<entry_fd>` **after**
  `prepare`, which is the line that witnesses the `O_PATH` descriptor surviving
  `close_fds_except` (the parent test calls it through the re-exec, and it passed).
- `plan/nodes/MB/MB.3/MB.3.56/LOG.md`: the OWNER RULING's `[route.parp]` typo is
  `[route.php]`; nothing else in the ruling was touched.

Verified on the final tree: `critical_php_embed` 21/21 (both entry legs), `cargo
test -p blastit --locked --features python` exit 0, `make check` exit 0,
`make docs-check` "No issues found", `critical_guard --base 2dbb17e --node MB.3.56`
OK, `critical-skip-check` OK, ruff check+format clean on the three e2e files. No
non-test code path changed, so `make e2e` was not re-run (its attempt-2 run is the
witness for the production path, which is untouched).

## Follow-ups

- `e2e/rootless/run.py` still fails the repo's ruff defaults (pre-existing,
  untouched here; `make py-lint` does not cover `e2e/`).
- Closing H-007 in `plan/HUMAN.md` is a plan mutation this node may not make.
- `README.md` churn is `scripts/build-wheel.sh` regenerating the long description.
- If the owner wants the check *inside* `PhpEmbedRuntime::serve`'s last line (the
  reviewer's preferred option for the MINOR), that is a change to
  `blastit-view`'s construction seam — the pin would have to travel through
  `PhpRuntimeConfig`, and a constructor that can omit it would be fail-open.
