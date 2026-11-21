# RESULT — MB.25.2

## Summary

The `INFO runtime` line now exists on every surface and in all three boot shapes, and the
`WARN manifest.warning` abuse is retired. A resolved runtime is `info` from
`component: "init"`; `manifest.warning` means what `docs/reference/log-events.md` says again.

- **Event model** (`crates/blastit-events`): `RuntimeEntrypoint`, `PhpRuntime`,
  `PythonRuntime` and the closed `Runtime` enum (`Php` / `Python` / `Static`), wired through
  `EventKind::name`/`default_level`, the JSON encoder and decoder, the truncation string
  slots, and the pretty renderer. The variant *is* the frozen field set, so a `python` line
  cannot grow a `libphp` and a `static` line cannot carry a version. `MAX_EVENT_BYTES` and
  the 2048-byte rules are unchanged; `php_ini` is written `null` (present) when PHP opened no
  file, and pretty prints `php.ini none`. The docs table row already existed (`MB.25.1`), so
  no reference page was edited.
- **PHP** (`php_view.rs`, `blastit-php-sapi`): the probe child reads PHP's own
  `php_ini_opened_path` after `php_module_startup` through one **additive** shim accessor
  (`const char *blastit_php_ini_opened_path(void)`, `dlsym`-resolved, optional) and reports
  `["<php_version()>", <ini or null>]` on its own pipe; the probe's parent emits exactly one
  `runtime` event naming the version, the resolved `libphp` and the ini. Stage 0's two
  `ManifestWarning(php_runtime_line(..))` sites are gone, and `php_runtime_line` (the sentence
  that named the internal shim) is deleted. `BLASTIT_PHP_ABI_VERSION` stays **1**: the
  accessor is optional and additive exactly like `blastit_php_file_sink_install`, and a shim
  from before it reports no path and serves unchanged — nothing in the header's numbered
  surface moved.
- **Python / static** (`stage0.rs`): one `emit_runtime` call after the manifest warnings in
  both `run` and `run_dev`, driven by `runtime_shape(entrypoint_php, has_app, interpreter)`.
  Python reads `sys.version_info()` and `VIRTUAL_ENV` else `sys.prefix` from the interpreter
  already live in stage 0 (`Python::try_attach`, so a wrong claim gets no line, never a
  panic); neither library nor callable is `static`.
- **Tests moved with the line** (authorised by `paths`): `critical_shim_contract.rs` pins the
  accessor's C/header/Rust contract and that the shim reports PHP's global rather than
  deriving a path; `critical_php_backend.rs` reads the `runtime` event from stdout and asserts
  one library, the version's minor, `php_ini` string-or-null, no `manifest.warning` and **no
  shim path** anywhere in normal output; `critical_php_embed.rs` does the same for the served
  pair and asserts every refusal emits no `runtime` line. No leg was removed or weakened.

## Acceptance

```console
$ cargo test -p blastit-events
running 97 tests
test result: ok. 97 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.18s
running 5 tests
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ cargo test -p blastit-php-sapi
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.86s
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ cargo test -p blastit --test critical_php_backend
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.40s
```

```console
$ cargo test -p blastit --features python --test critical_php_embed
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.64s
```

`make php-sapi` rebuilt `/usr/lib/libphp8.4.so`'s shim with the new accessor (`nm -D` shows
`T blastit_php_ini_opened_path` and `U php_ini_opened_path` resolved from the `RTLD_GLOBAL`
libphp), which is what makes the boot tail below carry a real ini.

### The `BLASTIT_LIBPHP=/usr/lib/libphp8.5.so` fail-closed command

```console
$ BLASTIT_LIBPHP=/usr/lib/libphp8.5.so cargo test -p blastit --test critical_php_embed
test result: FAILED. 18 passed; 9 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.36s
```

**Pre-existing and environmental, proved at the base commit.** `Boot::start` calls
`env_clear()`, so that variable reaches only the suite's *own* probe (`host_php`), which then
answers "no pair"; each nine pair-gated leg takes `refused_without_a_host`, whose boot is
started with `&[]` and therefore discovers this host's real `/usr/lib/libphp8.4.so` and
serves. The same nine legs fail identically at `4ee740d`, with the shim copied beside the base
binary so the base boot can find it:

```console
$ git archive 4ee740d | tar -x -C /tmp/opencode/base
$ CARGO_TARGET_DIR=/tmp/opencode/base-target BLASTIT_LIBPHP=/usr/lib/libphp8.5.so \
    cargo test -p blastit --test critical_php_embed     # in /tmp/opencode/base, shim copied beside the binary
failures:
    a_blocking_script_is_charged_cpu_time_not_wall_time
    a_decoy_inside_the_manifest_dir_is_refused_and_a_restart_serves
    a_fatal_is_contained_and_the_next_request_is_served
    a_form_body_reaches_the_front_controller
    a_queued_request_is_never_handed_to_a_retiring_view
    a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time
    an_entry_rewritten_in_place_is_refused_and_a_restart_serves_the_new_file
    an_exit_is_contained_and_the_next_request_is_served
    no_libphp_is_mapped_above_the_view_worker
test result: FAILED. 18 passed; 9 failed; 0 ignored; 0 measured; 0 filtered out
```

**The intent is proved directly** — the boot itself handed that path fails closed with the
existing sentence, exit `1`, empty stdout, and zero runtime lines:

```console
$ env -i BLASTIT_LOG_FORMAT=json BLASTIT_LIBPHP=/usr/lib/libphp8.5.so blastit dev --port 18953
blastit dev: route "app": entrypoint php "public/index.php": BLASTIT_LIBPHP (/usr/lib/libphp8.5.so)
cannot be opened or is not a regular file (No such file or directory (os error 2)); install the
PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php85-embed,
Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
exit: 1   stdout: (empty)   `"event":"runtime"` lines in stdout+stderr: 0
```

(`env -i` alone on the same fixture *serves* and prints the line, because stage 0's discovery
finds the host's 8.4 pair — which is exactly why the nine legs above cannot pass here at any
commit.)

### The 250 ms probe row (genuine host flake)

At base, that row flaked once with the full panic captured to
`/tmp/opencode/mb252/base-embed.log`:

```text
thread 'a_probe_that_hangs_is_killed_and_refuses_the_boot' panicked at
crates/blastit/tests/critical_php_embed.rs:1507:9:
no probe shape was written (/tmp/blastit-php-embed-stubs-996490/hang/shape): No such file or directory (os error 2)
… the probe child (pid 996898) did not load `libphp` and its shim within 250 ms, so it was killed
```

Re-run alone on this tree it passes; no timeout was raised and nothing was disabled:

```console
$ cargo test -p blastit --features python --test critical_php_embed a_probe_that_hangs
test a_probe_that_hangs_is_killed_and_refuses_the_boot ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 26 filtered out
```

### Lint, format, full gate, docs, guard

```console
$ cargo clippy -p blastit-events -p blastit-php-sapi -p blastit --all-targets --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.79s
$ cargo clippy -p blastit --all-targets --locked -- -D warnings        # standalone config
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.18s
$ cargo fmt --all -- --check
fmt: clean
```

```console
$ make check
…
uv run --no-project tools/plan.py validate
plan OK
MAKE_CHECK_EXIT=0
```

Exit `0` with 180 `test result: ok` lines, zero `test result: FAILED`, zero `FAILED`, zero
`N failed`.

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
...................................                                      [100%]
35 passed in 15.61s
```

```console
$ uv run --no-project tools/critical_guard.py --base 4ee740d --node MB.25.2
  M  crates/blastit-php-sapi/tests/critical_shim_contract.rs  [authorized by …]
  M  crates/blastit/tests/critical_php_backend.rs  [authorized by …]
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by …]
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.25.2
$ git diff --stat 4ee740d -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty)
```

## The three boot tails (verbatim, final artifacts)

`blastit dev` on a scratch fixture in `/tmp/opencode/mb252`, stopped with `SIGTERM`. Each
shows `"level":"info"`, `"event":"runtime"`, the documented fields, no `manifest.warning` for
a resolved runtime, and no shim path.

**(1) `entrypoint php`** — `BLASTIT_LIBPHP=/usr/lib/libphp8.4.so`,
`BLASTIT_PHP_SHIM_DIR=…/target/php-sapi`, Blastfile `entrypoint php "public/index.php"` +
`view app { GET "/*" }`:

```text
  blastit dev · single-uid simulation · 127.0.0.1:18962
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
20:45:12.759 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
20:45:12.863 INFO  runtime     php 8.4.27  libphp /usr/lib/libphp8.4.so  php.ini /etc/php/8.4/embed/php.ini
20:45:12.873 INFO  ready       serving  routes=1  workers=16
20:45:21.800 INFO  shutdown    drain
20:45:21.800 INFO  process.exited -  pid=1098978 uid=0  signal=15
20:45:21.801 INFO  shutdown    stopped
```

```json
{"ts":"2026-11-17T20:45:21.859Z","level":"info","event":"runtime","component":"init","entrypoint":"php","version":"8.4.27","libphp":"/usr/lib/libphp8.4.so","php_ini":"/etc/php/8.4/embed/php.ini"}
```

**(2) `entrypoint python`** — `entrypoint python "app:app"`, booted from the wheel this
`make check` built (`target/docs-wheel/blastit-0.2.0-cp314-…whl`) installed into the
fixture's `.venv` (the documented `uv add blastit` shape; stage 0 is the venv's interpreter):

```text
  blastit dev · single-uid simulation · 127.0.0.1:18960
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
20:44:54.598 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
20:44:54.598 INFO  runtime     python 3.14.10  environment /tmp/opencode/mb252/py/.venv
20:44:55.605 INFO  boot        preload  privilege=unprivileged-dev  build=0.2.0  immortalize=on  immortalized=11272  immortalize_ms=97.5ms  mem_total=32768MiB  mem_used=14918MiB  max_view_processes=455
reload: on (1 directories)
20:44:55.605 INFO  ready       serving  routes=1  workers=16
20:44:59.819 INFO  shutdown    drain
20:44:59.819 INFO  process.exited -  pid=1093830 uid=0  signal=15
20:44:59.819 INFO  shutdown    stopped
```

```json
{"ts":"2026-11-17T20:44:59.852Z","level":"info","event":"runtime","component":"init","entrypoint":"python","version":"3.14.10","environment":"/tmp/opencode/mb252/py/.venv"}
```

**(3) static only, no entrypoint** — `static "/" { root "public" fallback "index.html" }`:

```text
  blastit dev · single-uid simulation · 127.0.0.1:18964
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 0 routes
20:45:32.279 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
20:45:32.279 INFO  runtime     static  serving static assets only
20:45:32.283 INFO  ready       serving  routes=0  workers=16
20:45:40.272 INFO  shutdown    drain
20:45:40.273 INFO  process.exited -  pid=1101678 uid=0  signal=15
20:45:40.273 INFO  process.exited -  pid=1101677 uid=0  signal=15
20:45:40.273 INFO  shutdown    stopped
```

```json
{"ts":"2026-11-17T20:45:40.283Z","level":"info","event":"runtime","component":"init","entrypoint":"static"}
```

All three pretty lines reproduce the `docs/local.md` column layout and field order
(`docs/local.md:22-24`), and the JSON objects carry exactly the fields their row lists.

## Design decisions recorded

`docs/design/runtime-diagnostics.md` (new, excluded from the public site) records: the
closed-enum shape and why, where each entrypoint's line is emitted and why there, the probe's
pipe protocol, the `php.ini` provenance and its four "answer nothing" cases, the cadence
(one per resolution; at boot, and again when a `dev` reload cycle re-probes the pair), and
what is never logged.

Where the line is emitted is a consequence of where the facts exist: only a process that ran
`php_module_startup` can report the ini, so PHP's line is the stage-1 probe's; stage 0 must
never `dlopen` (`ADR-0010` §3, and a stage-0 probe would load `libphp` as root before the
barrier). Python's facts exist in stage 0 only when an interpreter is already live, which is
a wheel install — the build every published artifact uses.

## Follow-ups

1. **`cargo` development builds of `blastit` with `python` emit no `runtime` line for
   `entrypoint python`.** The interpreter is embedded in stage 1 (`Python::initialize()` in
   `stage1.rs`), so stage 0 has nothing honest to report; `try_attach` answers no line instead
   of guessing. Serving is unaffected. Fix belongs to the owner of `stage1.rs` (`MB.24.x`):
   emit next to `emit_preload`/`immortalize`, where the emitter and `py` are both in hand —
   this node may not edit that file.
2. A `blastit dev` reload cycle re-runs the boot half, so a PHP boot emits the line once per
   cycle (identical text when nothing changed). Matches "one line each time the entrypoint
   resolves"; if a quieter cadence is wanted, the announce gate lives in `stage1.rs`.
3. `refused_without_a_host` in `critical_php_embed.rs` is host-dependent by construction
   (`env_clear`): on a host that has a real pair it cannot pass when `BLASTIT_LIBPHP` points
   at nothing. Passing the nonexistent path through `Boot::start` would make it deterministic,
   but that changes what the leg proves, so it was left alone here.
4. `crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h` still points at
   `src/probe/` in one doc comment; that module does not exist in this crate. Pre-existing,
   out of scope.

## Files

| File | Change |
|---|---|
| `crates/blastit-events/src/lib.rs` | `RuntimeEntrypoint`, `PhpRuntime`, `PythonRuntime`, `Runtime`, `EventKind::Runtime`, name/level, unit tests |
| `crates/blastit-events/src/json.rs` | encode, decode, truncation slots, round-trip + shape + unknown-entrypoint + long-path tests |
| `crates/blastit-events/src/pretty.rs` | `runtime_summary`, golden-line tests, escape test, every-kind list |
| `crates/blastit-php-sapi/c/blastit_php_sapi.c` | `extern char *php_ini_opened_path;` + `blastit_php_ini_opened_path()` |
| `crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h` | optional/additive accessor declaration |
| `crates/blastit-php-sapi/src/abi.rs` | `IniOpenedPathFn` |
| `crates/blastit-php-sapi/src/loader.rs` | `Loaded::ini_opened_path()` + `dlsym` resolver |
| `crates/blastit-php-sapi/tests/critical_shim_contract.rs` | the accessor's C/header/Rust contract, ABI 1 kept |
| `crates/blastit/src/php_view.rs` | probe report pipe, `parse_report`, `emit_runtime` |
| `crates/blastit/src/stage0.rs` | `php_runtime_line` deleted, both `WARN` sites retired, `emit_runtime`/`runtime_shape`/`python_runtime`, decision-table test |
| `crates/blastit/tests/critical_php_backend.rs` | `runtime_event()` + served/refusal assertions (one library, no shim, no warn) |
| `crates/blastit/tests/critical_php_embed.rs` | served-pair assertion on the event, no-shim scan, no-runtime-on-refusal |
| `docs/design/runtime-diagnostics.md` | the technical design (new) |
| `docs/knowledge/LESSONS.md` | one line: never rebuild into the same target dir while `make check` boots the binary |

## Polish pass (attempt 2)

Six MINOR findings. Four were code-or-claim accuracy and are fixed here; the other two are
recorded, not fixed (they are outside this node's `paths`).

### 1. The 250 ms hang leg is no longer load-flaky (test only, no assertion dropped)

`crates/blastit/tests/critical_php_embed.rs`. Both hanging-probe legs
(`a_probe_that_hangs_is_killed_and_refuses_the_boot` and its `egress = true` twin
`an_egress_route_lets_the_probe_open_a_socket`) changed bounds:

| Bound | Before | After |
|---|---|---|
| `BLASTIT_PROBE_TIMEOUT_MS` handed to the boot | `250` | `2000` (`HANG_PROBE_TIMEOUT_MS`) |
| Harness wait before "neither served nor refused" | 20 s (`BOOT_TIMEOUT`) | 30 s (`HANG_BOOT_TIMEOUT`, via the new `Boot::await_boot_within`) |
| The legs' own "the knob bounded the boot" bound | 10 s | 20 s (`HANG_REFUSAL_BOUND`) |

Every other leg keeps the 20 s `BOOT_TIMEOUT`; only these two opt into the wider one.

**Why the old value was red and the behaviour was fine.** Before the stub can write the shape
these legs read, the probe child must fork, `ChildIo::prepare`, `setpgid`, drop to the route
uid, install the seccomp filter, `dlopen` the pair and *enter* `worker_init`. On a loaded box
that can take more than 250 ms, so the knob killed a child that had not yet written the shape
(the `:1516` "no probe shape was written" panic) — the timeout had done exactly what the leg
proves. The second failure shape (`:1925`, boot > 10 s) is the same crowding: the refusal path
(kill the group, reap, compose the sentence) ran behind a cold fork.

**Nothing was weakened — the oracles are unchanged, only the headroom:**

- the stub still hangs forever, so a boot that does not kill it still fails: at 30 s in the
  harness, with the leg's own 20 s assertion failing first if the refusal merely *drifts*;
- the refusal is still the proof that the *knob* bounded the boot: it must name the value
  applied — the assertion now reads `"2000 ms"` instead of `"250 ms"`, i.e. it still pins the
  number to the one handed in, not to a constant the implementation could hardcode;
- every containment assertion is untouched: exit `1`, §4.1's package sentence, no fallback
  recipe, `BLASTIT_PROBE_TIMEOUT_MS` named, no `ready`/`spawned`/`request`, nothing listening,
  the shape file (child of the boot, own process group, route uid/euid, **reaped** from
  `/proc`), and `inet == 1` (denied) / `inet == 0` (egress route) from the seccomp filter.

Observed on this host after the change (the leg's own evidence line):

```text
killed at BLASTIT_PROBE_TIMEOUT_MS=2000 after 2.061620154s; probe pid 1253206 ppid 1253198
pgid 1253206 uid 1000 seccomp EPERM, gone from /proc
route "app": entrypoint php "public/index.php": the probe child (pid 1253206) did not load
`libphp` and its shim within 2000 ms, so it was killed; raise BLASTIT_PROBE_TIMEOUT_MS if this
host needs longer; …
test a_probe_that_hangs_is_killed_and_refuses_the_boot ... ok
```

Verification: three full-suite runs idle (`27 passed` in 13.22 / 13.65 / 13.15 s) and one under
load (seven busy loops on eight cores: `27 passed` in 19.10 s).

### 2. The "a PHP build without the symbol reports no path" claim was false (comments only)

`nm -D` on the shim shows `U php_ini_opened_path`: the reference is undefined and resolves at
`dlopen`, which the worker does with `RTLD_NOW`. A `libphp` that did not export that global
would therefore fail the **whole shim load** — loudly, naming the symbol — not degrade to "no
path". Only a **shim** built before the addition degrades, and that happens through
`dlsym`/`Option` on the caller's side. Narrowed in the three places the reviewer named:
`crates/blastit-php-sapi/c/blastit_php_sapi.c` (the accessor's comment),
`crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h` (the declaration's doc) and
`docs/design/runtime-diagnostics.md` (`## php_ini`). No code or behaviour changed; the
`dlsym`-optional path, the `None` cases and `BLASTIT_PHP_ABI_VERSION = 1` are as they were.

The identical sentence still sits on the pre-existing `blastit_php_file_sink_install` comments
(`blastit_php_sapi.c:488`, the header at `:188`, `src/loader.rs:629`); same inaccuracy, same
truth, but pre-existing and outside this finding — recorded as follow-up 6 rather than edited
here.

### 3. Correction of the record: `blastit-events` has no `forbid(unsafe_code)`

`crates/blastit-events` has **never** carried `#![forbid(unsafe_code)]`: `grep -n "forbid"
crates/blastit-events/src/*.rs` is empty, and `json.rs:38` already held a pre-existing
`unsafe { String::from_utf8_unchecked(out) }` (the encoder's "the buffer is always valid
UTF-8" fast path). **This node added no `unsafe` to that crate**: the new code is the enum,
the two payload types, `name`/`default_level`, encode/decode arms, the truncation slots and
the pretty summary, all safe. The false constraint comes from `MB.25.2`'s own `NODE.md:68`
("`crates/blastit-events` stays `#![forbid(unsafe_code)]`"), which is managed goal text I may
not edit; that line is the source of the error and should be corrected by whoever owns the
node text.

### 4. Scope of the `BLASTIT_LIBPHP=/usr/lib/libphp8.5.so` acceptance: **PHP-free host only**

That command can never pass on a PHP-capable host **at any commit**, including `4ee740d`:
`host_php()` returns `None` for a path that is not a file while `Boot::start` calls
`env_clear()` and the boot then discovers the host's real 8.4 pair, so the nine
`refused_without_a_host` legs panic on a boot that served. It is a valid fail-closed test
only where the host genuinely has no usable `libphp` (the CI image). On this host the evidence
is the direct probe recorded above: `env -i BLASTIT_LIBPHP=/usr/lib/libphp8.5.so blastit dev`
exits `1`, writes **0 bytes** on stdout, prints §4.1's package sentence on stderr, and emits
**0** `"event":"runtime"` lines — and a hung/killed probe, a ZTS object, an unsupported minor,
a moved inode and a shim whose minor disagrees each emit none either
(`critical_php_embed`'s `assert_no_serving_surface` now asserts the absence of the `runtime`
event for every refusal leg).

### 5. Recorded, not fixed (outside `paths`)

- **`docs/knowledge/LESSONS.md`** (the "never rebuild into the same target dir while
  `make check` boots the binary" line) is outside this node's `paths`. The content is useful
  and the git skill permits staging it with the node; it is left in the working tree for
  whoever commits. Nothing else in that file was touched.
- **The `blastit dev` reload re-emission of an identical PHP line stays a follow-up.** A reload
  cycle re-execs the boot half and re-probes, so a PHP boot emits one `runtime` line per cycle
  even when the entrypoint did not change. The gate that would make it "only when the
  entrypoint changes" is `announce` in `crates/blastit/src/stage1.rs` (`Boot::announce`,
  `adopted.is_none()`), which this node may not edit. The wording to change with it is
  `docs/reference/log-events.md`'s `## The runtime line` sentence — "one line each time the
  entrypoint resolves: at boot in `blastit dev` and in server mode, and in `blastit dev` again
  after a `Blastfile` change that alters it, so the latest line names what is serving" — which
  `MB.25.1` owns and which today's per-cycle emission slightly over-delivers.

### Verification of this pass

```console
$ for i in 1 2 3; do cargo test -p blastit --features python --test critical_php_embed; done
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.22s
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.65s
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.15s
$ # the same suite with seven busy loops running on this eight-core host
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 19.10s

$ cargo test -p blastit --test critical_php_backend
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s
$ cargo test -p blastit-events
test result: ok. 97 passed; 0 failed; 0 ignored … ; test result: ok. 5 passed … ; test result: ok. 1 passed …
$ cargo test -p blastit-php-sapi
test result: ok. 5 passed … ; 8 passed ; 15 passed ; 0 passed (2 ignored) ; 1 passed
$ cargo clippy -p blastit-events -p blastit-php-sapi -p blastit --all-targets --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.16s
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.71s
$ cargo fmt --all -- --check
fmt: clean
$ uv run --no-project tools/critical_guard.py --base 4ee740d --node MB.25.2
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.25.2
```

`make check` (the final run, after these edits):

```console
$ make check
…
test a_probe_that_hangs_is_killed_and_refuses_the_boot ... ok
test an_egress_route_lets_the_probe_open_a_socket ... ok
…
test result: ok. 27 passed; 0 failed; 0 ignored; … finished in 13.62s
…
uv run --no-project tools/plan.py validate
plan OK
MAKE_CHECK_EXIT=0
```

Exit `0`, 180 `test result: ok` lines, 0 `test result: FAILED`, 0 `FAIL`. **The hang leg and
its egress twin both ran and passed — twice**, once in `rs-test`'s
`--workspace --all-targets` pass (log lines 579/591) and once in `test-critical`'s
`rs-test-critical` pass (lines 4550/4562), each alongside `critical_php_embed`'s `27 passed`.
