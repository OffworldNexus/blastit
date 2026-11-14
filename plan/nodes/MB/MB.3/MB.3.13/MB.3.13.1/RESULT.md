# Result MB.3.13.1 — PhpEmbedRuntime worker loop, loader, and PHP spawn path

**Status: done (attempt 3 — review fixes applied).** Every `# Acceptance` command
and `make check` pass on this host. Goldfish attempt 2 verified all attempt-1 items
fixed and left 1 MAJOR + 5 MINORs; their disposition is in `## Review fixes
(attempt 3)`.

**One deviation to note up front: the diff is ~3,400 new lines, not ≤600** (see
`## Budget` at the end). The lead accepted it as a recorded deviation (MAJOR 6).
**Interim state, also lead-decided (MAJOR 5):** no caller forks a PHP view yet, so
a manifest with a `[route.php]` route is **accepted at boot but not served**;
`MB.3.13.3` must add `Ctl::Spawn`/`SpawnSpec::Php` *and* the boot refusal before
any release ships one. That is stated in the module docs of `php_view.rs` and on
`fork_php_view`; no stage-0 change was made here (`.2`'s paths).

## Review fixes (attempt 3)

| # | finding | fix |
|---|---|---|
| **MAJOR** | `CANCEL` was view-fatal: `ChanIo.error` held a `Protocol` error, so `serve_forever` returned `Err` without `RESP_END_COST`/`DONE` and the worker exited — every client disconnect cost a fork + `php_module_startup` | `ChanIo` has a `cancelled` flag set by `cancel()` (window closed, `ended`, **no** `error`). `write_head`/`write_body` become no-ops while cancelled (so PHP finishes without a spurious bailout and the worker is not retired); a cancel met inside `write_body`'s credit pump is swallowed the same way. `serve_forever` forces `RequestEnd.outcome = Aborted`, writes `RESP_END_COST` + `DONE(Aborted)` and **continues the loop**; only a frame/channel error returns `Err`. An idle `Frame::Cancel` before the next `REQ_HEAD` (or a stale `Frame::Cancel` with `id != self.id` inside `read_body`/`pump`/`drain_body`) is dropped as stale scheduling bookkeeping so the next `REQ_HEAD` is still served, matching the WSGI twin (`lib.rs`); protocol-error strings also format `crate::frame_kind(&other)` rather than `{other:?}` so stray body frames never dump payload bytes to stderr (`Credit::try_take` is gated `#[cfg(any(feature = "php", test))]` for zero warnings on non-`php` builds). `a_cancelled_request_abandons_the_response_and_ends_the_view` was replaced by `a_cancelled_request_aborts_but_the_worker_serves_the_next_one`, which asserts `RESP_END_COST` then `DONE(Aborted)` for id 9, **no** `RESP_HEAD` for it, drops both an idle `CANCEL(9)` and an in-flight `CANCEL(9)` after `REQ_HEAD(10)`, and verifies a full `RESP_HEAD…DONE(Completed)` for id 10 on the same thread. |
| m1 | `symbol` doc claimed a wrong-sized/wrong-signature target is a compile error | doc now says what is true: all entry points are pointer-sized function symbols; `transmute` checks the *size*, the signature is guarded by `abi.rs` + the shim's `_Static_assert`s + `critical_shim_contract`. |
| m2 | `serve` never called `flush_response` | called once at the end (after the shim, before the outcome is computed); `Err` maps to `DoneOutcome::Aborted`. |
| m3 | the re-exec'd child test panicked under `cargo test -p blastit -- --ignored` | early-returns when `BLASTIT_PHP_VIEW_CHILD` is unset, like `immortal::marker_child`; `cargo test -p blastit -- --ignored` now passes. |
| m4 | both native harnesses only loaded `SharedObject::Path` | the *view* harness now pins the real `libphp`/shim as `OwnedFd` and loads `SharedObject::Fd` end to end (one `php_module_startup` per process, so the two spellings live in different test binaries); the *sapi* harness gained `an_fd_pinned_libphp_and_shim_load_through_proc`, which pins both files, checks `/proc/self/fd/<n>` and loads them through the loader using `Path` in its request test. |
| m5 | crate docs said feature `php` "is declared here and empty" | now says it forwards `blastit-view/php` and adds `blastit-php-sapi`, keeps "nothing links `libphp`" and names the `dlopen`-only rule. |

## Review fixes (attempt 2)

| # | finding | fix |
|---|---|---|
| M1 | `ChildIo.keep` held only `view_end`, so `close_fds_except` closed the fd-pinned `libphp`/shim (production `ENOENT`) | `PhpBinding` now owns `PhpObject::{Fd(OwnedFd), Path}`; `pinned_fds()` is appended to `keep`. New re-exec'd test `a_pinned_descriptor_survives_the_child_sequence` opens two files, runs the real `ChildIo::prepare()`, and reads both back through `/proc/self/fd/<n>` (and asserts `shared()` is `Fd`, not `Path`). |
| M2 | `sink_body` did `from_raw_parts(NULL, 0)` | null checked first: `len == 0 → 0`, else `1` (abort); the slice is built only when non-null. |
| M3 | a shim `-2` (startup ran and failed) left `initialized == false`, so `Drop` `dlclose`d `libphp` after `php_module_startup` | new `startup_attempted` flag set **before** the `worker_init` call; `Drop` leaks deliberately once it is set. `LoadError::Init` still carries the code and its message. Native harness now also asserts a second `worker_init` is a typed `Init` error and the object stays usable. |
| M4 | the child never pinned `HOME`/`TMPDIR` | `set_view_environment` pins both to the binding's `view_tmp` (`<runtime>/views/<slot>/tmp` in production), mirroring the WSGI path; it runs after `set_dumpable`, before the mask restore, and refuses a NUL path. Covered by the same re-exec'd child test. |
| M5 | inert fork / a `[route.php]` manifest accepted but not served | lead decision: no wiring here. Module doc + `fork_php_view` doc + this file state the interim state and that `.3` owes the spawn and the boot refusal. No stage-0 refusal added. |
| M6 | 4.6× the leaf budget (5.7× after the attempt-2 tests) | lead-accepted recorded deviation; see `## Budget`. |
| m5 | `write_body` accepted a body before the head | now `Protocol` error; twin test `a_response_body_before_the_head_is_a_protocol_error`. |
| m6 | `validate()` did not check `view_tmp` | absolute + existing directory required; denied twins (`missing`, `relative`) in `a_configuration_that_could_not_serve_is_refused_both_ways`. |
| m7 | `SCRIPT_NAME`/`PATH_INFO`/`REQUEST_URI` contract unpinned | `SCRIPT_NAME` = mount point (`/` at root), `PATH_INFO` = mount-stripped at a segment boundary (`strip_mount`, same rule as WSGI/router), `REQUEST_URI` unstripped. Asserted in the `$_SERVER` test plus `the_mount_point_is_stripped_only_at_a_segment_boundary`. |
| m8 | unbounded request-body buffer and header accumulation | `PhpRuntimeConfig::max_body_bytes` (validated ≥ 1) with `read_body` refusing past it (`BodyError::TooLarge` → `Errored`, worker retires); `PhpSink` caps at `MAX_HEADERS = 256` / `MAX_HEADER_BYTES = 64 KiB` and aborts. Twins: `a_request_body_past_the_worker_budget_is_refused`, `a_header_burst_past_the_cap_aborts_the_response`. |
| m9 | `symbol<T>` size check was a `debug_assert`; `sink_cell`/`sink_mut` handed out caller-chosen lifetimes | `symbol` returns `*mut c_void` and every call site `transmute`s to the declared type (a **compile-time** size check); `with_sink(ctx, f)` is the only way to reach the sink, so the `&mut dyn Sink` has a fresh local lifetime and cannot escape the callback. |
| m10 | `php_version()` result was not null-checked | `LoadError::NoVersion` when the symbol returns NULL; `CStr::from_ptr` is only reached for a non-null pointer. |
| m11 | two adversarial tests missing | `a_request_body_past_the_flow_window_is_a_protocol_error` (window spent, one `REQ_BODY` → `Protocol`, error remembered) and `a_bailed_worker_retires_and_max_requests_still_applies` + `a_runtime_that_retires_ends_the_loop_after_its_done` (bailout/`max_requests` exit after `DONE`). |
| m12 | RESULT named the wrong command for `php::tests` | corrected below: the suite runs under `cargo test -p blastit-view --features php` and, in `make check`, through feature unification (`blastit`'s default `php`), never by `cargo test -p blastit-view` alone. |

## What landed

1. **`crates/blastit-php-sapi/src/loader.rs`** — the crate's one `#[allow(unsafe_code)]`
   module (the crate root stays `deny(unsafe_code)`; `abi.rs` is untouched and clean).
   Hand-written `dlopen`/`dlsym` (no new dependency — `cargo tree -p blastit-php-sapi`
   is still just the crate). Two-step order exactly as `src/probe/ffi.rs`:
   `libphp` first with `RTLD_NOW | RTLD_GLOBAL`, the shim second with `RTLD_NOW |
   RTLD_LOCAL`. `SharedObject::{Fd, Path}` makes `/proc/self/fd/<n>` the production
   spelling and a path the native-dev spelling, in one code path. After the load it
   verifies `blastit_php_abi_version() == 1` and maps `blastit_php_minor()`, then
   cross-checks it against the **host's** `php_version()` (a shim of the wrong minor
   is a typed `MinorMismatch`, not a first-request fault). `worker_init` / `request` /
   `worker_shutdown` / `php_version` are safe wrappers; the sink callbacks are
   `extern "C"` trampolines that `catch_unwind` so nothing unwinds into C. The module
   is compiled on **every** host (it needs no toolchain), so `abi` was un-gated from
   `cfg(php_toolchain/probe)`; it is pure declarations and layout pins.
   `Loaded` is `Send` (one process-wide handle, moved) and deliberately not `Sync`, so
   safe code cannot run two NTS requests at once.

2. **`crates/blastit-view/src/php.rs`** (`#[cfg(feature = "php")]`; the crate stays
   `#![forbid(unsafe_code)]`) — `PhpEmbedRuntime` implements `WorkerRuntime`:
   `kind() == "php"`, `max_inflight() == 1`, `serve` takes the process-wide handle
   behind a `Mutex`, builds `ServerVars`/`RequestV1` from the head (adding
   `SCRIPT_FILENAME` from the pinned `entry`, `DOCUMENT_ROOT`, `PATH_INFO`,
   `QUERY_STRING`; headers via `push_header`, so `Proxy:` can never become
   `HTTP_PROXY`), drains the body through `RequestIo::read_body`, drives the shim with
   a `SinkV1` whose callbacks write `RESP_HEAD`/`RESP_BODY`/output, and returns
   `RequestEnd { outcome, cpu_ns: Some(out.cpu_ns) }`. `recycle()` is true after
   `max_requests` or a bailout.

3. **The `view.chan` transport for one in-flight request** — `PhpRuntimeConfig`
   (+`validate`: absolute existing entry, existing document root, existing absolute
   `view_tmp`, non-empty INI, `max_requests ≥ 1`, `max_body_bytes ≥ 1`), `ChanIo` (a
   `RequestIo`: `read_body` consumes `REQ_BODY` and credits `WINDOW`; `write_body`
   refuses before the head and respects the response credit; `output` honours
   `OUTPUT_LINE_CAP` on a character boundary; `flush_response` is the synchronous
   no-op), `read_body` (the whole-body buffer behind `max_body_bytes`), `PhpSink`'s
   header caps, and `serve_forever(chan, Arc<dyn WorkerRuntime>, Limits)`, which sends
   `READY`, reads `REQ_HEAD`, calls `serve`, then writes `RESP_END_COST { wall_ns,
   cpu_ns }` + `DONE` from the `RequestEnd`. The transport is single-threaded (§
   below) and **the caller** owns `RESP_END*`/`DONE`, per MB.3.23's contract.
   `Credit::try_take` was added to `flow.rs` for it (the non-blocking "is there credit,
   or must I read a `WINDOW`?" query the write path needs).

4. **`crates/blastit/src/php_view.rs`** — the PHP view fork, a sibling of `zygote`
   (not a branch inside it): socketpair + pipes + `ChildIo` (private dir as cwd,
   `nofile`, `core_zero`, `nproc` only in production) whose `keep` includes both
   `pinned_fds()` and `view_end`, `child.prepare()`, `drop_privileges(route_uid)` when
   production, `harden_child`, `SeccompRole::EmbeddedView { egress }`,
   `set_dumpable(false)` **after** the drop, `HOME`/`TMPDIR` pinned to the view's
   private `view_tmp`, restored signal mask, then `PhpEmbedRuntime::load` (boot
   refusal → `exit 1`) and `blastit_view::php::serve_forever`. `PhpBinding` owns
   `PhpObject::{Fd(OwnedFd), Path}` so stage 1's fd-pinned objects survive
   `close_fds_except` and a native path uses the same function. Feature wiring:
   `blastit-view: php = ["dep:blastit-php-sapi"]`, `blastit: php =
   ["blastit-view/php", "dep:blastit-php-sapi"]`, `Cargo.lock` updated.

**Deduplication, behaviour-neutral:** `mount_point` moved from `zygote` to
`config::mount_point` and `panic_message` from `zygote` to `child.rs`, so the WSGI and
PHP forks share one implementation (both files are in this node's paths).

**Which layer each proof covers.** The seccomp wall is `SeccompRole::EmbeddedView`;
this node installs it at the real install site (`php_view::child_main`) and
`crates/blastit-sys/tests/critical_embedded_seccomp.rs` (MB.3.25) proves the filter
itself. No `make check` test installs that filter (it needs root/`dev` and the
supervisor wiring of MB.3.13.3), so the `EmbeddedView` install here is proven by
inspection + the `blastit-sys` suite. The child *sequence before the filter* — the kept
descriptors and `HOME`/`TMPDIR` — is proven for real by the re-exec'd
`php_view::tests::a_pinned_descriptor_survives_the_child_sequence`, which runs
`ChildIo::prepare()` in a fresh single-threaded process and reads the pinned files back
through `/proc/self/fd/<n>`. The request path is the native harness below. The INI
`allow_url_fopen` is defence in depth, never claimed as the wall.

## Evidence (all in `.worktrees/release-0.2.0`)

```console
$ mkdir -p /tmp/… && make php-sapi
php-sapi: cc -> …/target/php-sapi/libblastit_php84.so (/usr/bin/php-config8.4 reports 8.4.27)
php-sapi: skipped 8.3: no php-config for PHP 8.3 (…)
php-sapi: 1 shim(s) in …/target/php-sapi; skipped: 8.3 8.2
```

Real `dlopen` + ABI/minor + `worker_init` + one request through the loader
(`blastit-php-sapi`):

```console
$ BLASTIT_LIBPHP=/usr/lib/libphp8.4.so BLASTIT_PHP_SHIM=$PWD/target/php-sapi/libblastit_php84.so \
    cargo test -p blastit-php-sapi --test php_native --locked -- --ignored
running 2 tests
test an_fd_pinned_libphp_and_shim_load_through_proc ... ok
test a_native_libphp_loads_and_serves_one_request ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```
The first pins both files as `OwnedFd`s and loads `SharedObject::Fd` through
`/proc/self/fd/<n>` (the production spelling). The second asserts
`abi_version() == 1`, `minor() == 8.4`, `php_version() == "8.4.x"`, a 200 head with
`X-Native: 1`, the body, `cpu_ns > 0`, `body_bytes`, a second `worker_init` refused
as a typed `Init` error, then `worker_shutdown`.

`[route.php]` answering **through `view.chan`**, two requests on one worker, then the
clean `max_requests` exit (`blastit-view`). This harness loads with
`SharedObject::Fd` (pinned descriptors), so the end-to-end path is the production
spelling too:

```console
$ BLASTIT_LIBPHP=/usr/lib/libphp8.4.so BLASTIT_PHP_SHIM=$PWD/target/php-sapi/libblastit_php84.so \
    cargo test -p blastit-view --features php --test php_native --locked -- --ignored
test a_php_route_answers_two_requests_through_view_chan ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
It drives `READY → REQ_HEAD/REQ_END → RESP_HEAD (200, X-Embedded: 1) → RESP_BODY
(method=GET; …/index.php) → RESP_END_COST { cpu_ns: Some(_), wall_ns: > 0 } → DONE
(Completed)` twice (`max_requests = 2`), then `serve_forever` returns `Ok` — the
exit the supervisor respawns on.  (`SCRIPT_FILENAME` is asserted to be the pinned
entry; `CPU` is `CLOCK_PROCESS_CPUTIME_ID` from the shim, so the number cannot be
forged by the script.)

Acceptance:

```console
$ cargo test -p blastit-view --locked
test result: ok. 58 passed; … 30 passed; … (0 failed; php module not compiled)
$ cargo test -p blastit-view --features php --locked
test result: ok. 74 passed; … 30 passed; … (0 failed, 1 ignored native harness)
$ cargo test -p blastit --locked
test result: ok. 165 passed; 0 failed; 1 ignored … (lib; 13 suites, 0 failed)
$ cargo test -p blastit --locked -- --ignored
running 1 test … test result: ok. 1 passed … (the re-exec'd child returns early
              # without its marker, so a manual --ignored run does not fail)
$ cargo test -p blastit-php-sapi --locked
test result: ok. 5 passed; … 13 passed; … 2 ignored … 1 passed (lib, critical, harness, doctest)
$ cargo check -p blastit --no-default-features --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s)
$ make check
EXIT:0        # 117 `test result: ok`, 0 FAILED; guard, critical-guard (no protected
              # test changed), fmt, clippy -D warnings, rustdoc -D warnings, full
              # workspace tests + doctests, every critical suite, docs-check/unit/test,
              # rs-no-python (portable ELF), plan validate
$ readelf -d target/debug/blastit | grep NEEDED
 (libgcc_s.so.1, libm.so.6, libc.so.6, ld-linux-x86-64.so.2)   # no libphp, no libpython
$ readelf -d target/debug/blastit | grep -c libphp
0
```

**One transient observed, not reproducible.** In the first `make check` of attempt
2, `critical_shim_contract::an_unsupported_minor_is_refused_by_name` failed once
with `expected UnsupportedVersion, got NotFound`; the same test then passed in
isolation, in `make rs-test` (92 suites), and in two subsequent full `make check`
runs (117 suites, 0 failed). The cause is the environment, not the change: the
test execs a fake `php-config` script written under `$TMPDIR`, and
`Toolchain::probe` maps **any** `Command::output()` failure to "not found"
(`.ok()?`), so a spawn failure on a heavily loaded host reads as
`NotFound`. Nothing in this node touched `toolchain.rs`; the fake-script test and
the runner are unchanged from `MB.3.12`.

Ordinary (non-`--ignored`) suites added by this node, which run on a PHP-free host.
The `php::tests` module is behind the `php` feature, so it runs with
`cargo test -p blastit-view --features php --locked` (74 lib tests) and inside
`make check` through feature unification — `blastit`'s default `php` turns
`blastit-view/php` on for the workspace test graph — **never** with
`cargo test -p blastit-view --locked` alone (58 lib tests, php module not compiled).
`php_view::tests` run under `cargo test -p blastit`:

| test | proves |
|---|---|
| `php::tests::a_configuration_that_could_not_serve_is_refused_both_ways` | accepted config passes; missing/relative entry, empty INI, `max_requests=0`, `max_body_bytes=0`, missing/relative `view_tmp` refused (boot refusals) |
| `php::tests::a_non_library_is_a_boot_refusal_and_never_a_request_fault` | `PhpEmbedRuntime::load` on a text file is `LoadError::NotALibrary` |
| `php::tests::a_socketpair_request_is_answered_and_closed_in_order` | `Window` credit, `RESP_HEAD/BODY`, `RESP_END_COST.cpu_ns`, `DONE` order + id |
| `php::tests::a_large_response_waits_for_window_frames_instead_of_deadlocking` | response > `INITIAL_WINDOW` reads its own `WINDOW` (no second thread, no deadlock) |
| `php::tests::a_request_body_past_the_flow_window_is_a_protocol_error` | a `REQ_BODY` beyond the window is `Protocol` and remembered |
| `php::tests::a_request_body_past_the_worker_budget_is_refused` | the worker's own body cap refuses past `max_body_bytes` (accepted twin fits) |
| `php::tests::a_response_body_before_the_head_is_a_protocol_error` | `RESP_BODY` before `RESP_HEAD` is refused |
| `php::tests::a_bailed_worker_retires_and_max_requests_still_applies` + `a_runtime_that_retires_ends_the_loop_after_its_done` | the retirement rule and the loop's `DONE`-then-`Ok` exit |
| `php::tests::a_header_burst_past_the_cap_aborts_the_response` | 256 headers / 64 KiB caps (accepted twin inside the cap) |
| `php::tests::a_cancelled_request_aborts_but_the_worker_serves_the_next_one` | `CANCEL` → `RESP_END_COST` + `DONE(Aborted)`, nothing of the response on the wire, idle/stale `CANCEL(9)` (before and after `REQ_HEAD(10)`) dropped, and request 10 served in full (`RESP_HEAD…DONE(Completed)`) by the same worker |
| `php::tests::a_stray_body_frame_names_only_its_kind_and_never_leaks_bytes` | stray `REQ_BODY`/`RESP_BODY` protocol errors name `frame_kind` only (zero payload bytes), and foreign non-`Cancel` frames stay strict protocol errors |
| `php::tests::a_request_without_a_stream_is_a_protocol_error` | id 0 is a real protocol error and does end the view |
| `php::tests::an_output_line_is_capped_on_a_character_boundary` | `OUTPUT_LINE_CAP` never splits a UTF-8 character |
| `php::tests::the_php_environment_never_carries_http_proxy` | `Proxy:` dropped at the `$_SERVER` seam (httpoxy); `SCRIPT_FILENAME`, `REQUEST_URI`/`SCRIPT_NAME`/`PATH_INFO` contract |
| `php::tests::the_mount_point_is_stripped_only_at_a_segment_boundary` | `strip_mount` matches the WSGI/router rule |
| `php_view::tests::a_pinned_descriptor_survives_the_child_sequence` | re-exec'd child runs real `ChildIo::prepare()`; both `PhpObject::Fd`s stay open through `/proc/self/fd/<n>`, and `HOME`/`TMPDIR` become `view_tmp` |
| `loader::tests::*` | missing/text-file/NUL path are typed refusals; `/proc/self/fd` spelling; ABI/minor messages name the fix |
| `php_native::an_fd_pinned_libphp_and_shim_load_through_proc` (`--ignored`) | the real `libphp`+shim pinned as `OwnedFd` load through `SharedObject::Fd` and `/proc/self/fd/<n>` |

## Readings recorded (per the brief's "record that reading")

- **Public surface.** Only what a cross-crate caller names is public:
  `blastit_view::php::{PhpEmbedRuntime, PhpRuntimeConfig, PhpError, serve_forever,
  is_serveable_entry}` and `blastit_php_sapi::{Loaded, LoadError, SharedObject, Sink}`;
  `blastit::php_view` is crate-private and names `PhpObject`/`PhpBinding`/`PhpViewSpawn`
  for the supervisor (`MB.3.13.3`). `Writer`, `Flow`, `ChanIo`, `Prepared`, `PhpSink`
  and the trampolines stay private; the scheduler side will hold
  `Arc<dyn WorkerRuntime>`, never `ChanIo`.
- **`serve_forever` is generic over `Arc<dyn WorkerRuntime>`** (not `Arc<PhpEmbedRuntime>`)
  so the transport can be driven by an interpreter-free test runtime on a PHP-free
  host; the real caller passes `Arc::new(PhpEmbedRuntime)`.
- **Single-threaded transport is deliberate.** PHP's ABI takes one `body` slice, so the
  body is fully read before `php_request_startup`; after that the only inbound frames
  are `WINDOW`/`CANCEL`, so `write_body` reads them itself when out of credit. A stray
  `WINDOW` or stale `CANCEL` after a finished request is dropped as bookkeeping, not a
  protocol error (the scheduler may still be spending credit or racing `DONE`).
- **`recycle()` on a bailout.** The ABI cannot distinguish `exit()`/`die()` from a
  fatal: both are `zend_bailout` (`OUTCOME_BAILOUT`, `contained = 1`). Per the brief,
  any bailout recycles; the epic's "the next request is served" is satisfied by the
  supervisor respawn, not by reusing the interpreter.
- **`cpu_ns == 0` → `None`.** A 0 reading is "no report" (`Some(0)` would be a claim
  that the request was free); the shim already returns 0 when `clock_gettime` fails.
- **OPcache open question:** the **post-fork `php_module_startup`** path is what this
  node implements (per-worker, no shared writable OPcache SHM), per ADR-0010 §1/§4.3.
- **A failed startup leaks the module on purpose.** The shim's `-2` means
  `sapi_startup`/`php_module_startup` *ran* and failed; `Loaded` records
  `startup_attempted` before the call and never `dlclose`s afterwards, because
  unloading a half-initialised module table would run PHP's own teardown. The boot
  refusal is still returned (with the code) and the worker exits.
- The brief cites `crates/blastit-view/src/serve.rs`; that file does not exist — the
  WSGI loop lives in `lib.rs` and was **not** forked or modified.

## Follow-ups (out of this node's scope; no code written here)

1. **`docs/knowledge/LESSONS.md` (docs/ is not in this node's paths).** Worth adding:
   *"`libphp` records the thread stack in `php_module_startup` and checks each request
   against it (`zend.max_allowed_stack_size`); calling `worker_init` on one thread and
   `php_request` on another fails with 'Maximum call stack size of 2048000 bytes …
   during compilation' even for a two-line script. Fork, load and serve on one thread
   (the fork child's main thread), as `php_view` does; a test harness must too."*
   And: *"a callback context that carries a `&mut dyn Trait` through a `void *` must
   point at a `Sized` cell holding the fat pointer, and the reference must be handed to
   a closure (`with_sink`) rather than returned — returning it lets the caller pick the
   lifetime. Also, `slice::from_raw_parts(NULL, 0)` is UB: a `body(data=NULL, len=0)`
   callback from C must special-case `len == 0`."*
   And: *"`php_module_startup` is process-global: two `PhpEmbedRuntime`s/`Loaded`s in
   one process segfault on the second startup (`Unable to start builtin modules`), so a
   test binary that loads PHP may hold exactly **one** such runtime — put the `Path`
   and `Fd` spellings in different test binaries, or combine them into one test."*
2. **Supervisor wiring — the interim state (lead decision MAJOR 5).** `Ctl::Spawn` →
   `PhpBinding` → `fork_php_view` and stage-1 fd-pinned `PhpObject::Fd` are
   MB.3.13.3; `php_view` is `#[allow(dead_code)]` with the reason until then, and its
   module docs state that a `[route.php]` manifest is accepted at boot but **not
   served**. `spawn_now` is still `#[cfg(feature = "python")]`, so a php-only build
   cannot yet reach the fork. `.3` must add both the selection and the boot refusal
   before a release ships a PHP route.
3. **Header byte fidelity:** `ServerVars`/`ServerVar` (MB.3.12 API) take `&str`, so
   non-UTF-8 header values are `from_utf8_lossy`-mapped here. A latin-1 mapping belongs
   in `blastit-php-sapi::request` (that crate's paths are there, but the API change is
   not this leaf's contract).
4. **`$_SERVER['CONTENT_TYPE']`/`CONTENT_LENGTH`** are not inserted (the shim takes
   them from `RequestV1`); add them if a framework reads `$_SERVER` directly.
5. **`exit()` and a fatal recycle the same way** (see Readings). If MB.3.13.4 needs the
   same process to serve after a non-fatal `exit()`, the ABI needs a distinguishing
   bit in `blastit_php_result_v1` (an ABI change, MB.3.12/3.13.4 call).
6. `PhpEmbedRuntime::php_version()` allocates a `String`; a `&str` accessor is blocked
   by the `Mutex` guard lifetime — fine for a boot log only.
7. **Pre-existing flake, MB.3.12 owns it (do not fix here):** `Toolchain::run` at
   `crates/blastit-php-sapi/src/toolchain.rs:315-318` collapses **every**
   `Command::output()` failure (including a transient `spawn` error under load) into
   `Err(detail)`; `from_exe` at `:202-204` maps that to
   `ToolchainError::NotExecutable`, and `discover_with` discards `NotExecutable`
   (`:157`, and the `PATH` loop) ending at `NotFound` (`:192`). That is why
   `critical_shim_contract::an_unsupported_minor_is_refused_by_name`
   (`crates/blastit-php-sapi/tests/critical_shim_contract.rs:383`) can redden under
   load with `expected UnsupportedVersion, got NotFound`. Observed once in attempt 2,
   then green in isolation, in `make rs-test` (92 suites) and in three full
   `make check` runs.

## Budget

`~3,400` new lines in five files (loader 776, `php.rs` 1,635, `php_view.rs` 502, the
two native harnesses 409) + `~115` added lines in nine tracked files. That is **5.7×**
the 600-line leaf rule in `docs/design/backend-runtimes.md` §8.2, because this leaf was
given four deliverables (loader, runtime, transport, spawn) that the ADR's leaf map
splits across `.1`/`.2`/`.4`. **The lead accepted this as a recorded deviation
(MAJOR 6); it is not split.** Nothing was left half-done to fit. If it is ever
re-split, the natural seams are:

- **a.** `blastit-php-sapi/src/loader.rs` + `abi` un-gating + `tests/php_native.rs`
  (~950 lines) — the `dlopen`/ABI gate, independently proven by its own native harness.
- **b.** `blastit-view/src/php.rs` + `flow::try_take` + `Cargo.toml`/lock
  (~1,670 lines) — `PhpEmbedRuntime` + the single-request transport, proven by the
  socketpair suites and `tests/php_native.rs`.
- **c.** `blastit/src/php_view.rs` + the `mount_point`/`panic_message` moves + features
  (~590 lines) — the fork path and the re-exec'd child-sequence test, provable end to
  end only once MB.3.13.3 wires `Ctl::Spawn`.

(a) and (b) have no dependency on (c), and (c) is the only part that cannot be tested
end to end yet; each compiles and passes `make check` on its own.
