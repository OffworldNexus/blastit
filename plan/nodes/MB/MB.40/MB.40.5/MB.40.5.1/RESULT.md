# MB.40.5.1 — result

**Status:** done (attempt 2 of 3)

The developer-facing boot tells the truth at a glance: the banner names a
clickable address (bracketed for IPv6) plus a `[LEARN MODE]` badge and one line
per declared view, live reload is a structured `reload` event in the one shared
event column instead of a free-text line, HTTP status carries colour on a
coloured stream, a phase line is printed before the blocking app import, and a
production refusal with a terminal watching is prose on stderr (stdout stays
empty).

## What changed in this attempt

1. **Compile + `-D warnings` clean.** `crates/blastit/src/check.rs`'s
   `EventDrain` learned `blastit_events::Level`; the dead `fn stderr_fd()` was
   removed; `crates/blastit/src/dev_learn.rs`'s unit test no longer shadows the
   `banner()` fn with a local `banner`; `route_noun()` is now the banner's own
   helper (no dead code); the unused `Emitter::is_json()` and the
   `libc::STDOUT_FILENO` import are gone; `ReloadBoot` is imported only under
   `feature = "python"` (its only caller is `announce_reload`).
2. **`crates/blastit-events/src/pretty.rs`:** the pre-crash unit test
   `name_padding_lines_up_short_and_long_names` pinned a `serving` summary on a
   bare `Lifecycle::default()`, which has an empty summary; it now pins a
   realistic `ready` line (`routes=2`) and still proves one shared summary
   column, measured as `12 + 1 + 5 + 1 + NAME_COLUMN`.
3. **The three authorized protected re-pins** (nothing else in those files):

   | file | lines | new pin |
   |---|---|---|
   | `critical_dev_and_fail_closed.rs` | 1223-1254 | banner line 1 ends `· http://127.0.0.1:{port}`; `banner.len() == 3` kept; the view table is asserted from `dev.stdout_text()` (one `view  …` line per route, `view  POST  /json`, `view  *  …` for a method-less view) |
   | `critical_static_cli.rs` | 288-299 | host form is `http://127.0.0.1:{port}`; `http`/`scheduler`/`dbproxy` and `0 routes` still asserted |
   | `critical_dev_reload.rs` | 1620-1690 | exactly one line whose 3rd whitespace token is `reload`; `<n> directories` / `1 directory` parsed from its tail; its summary column equals the `ready` line's (measured byte offset of the 4th token); exactly one `{"event":"reload"}` with `directories >= 1`; no raw `reload: on (` on either stream |

   Test names and counts are unchanged — `#[test]` counts, HEAD vs working tree:
   `critical_dev_and_fail_closed` 20/20, `critical_static_cli` 12/12,
   `critical_dev_reload` 23/23; `git diff -U0` over the three files adds and
   removes no `fn ` or `#[test]` line.

## Captured output

### pty banner, view table, phase line, one `reload` event, aligned event table

`script -qec "... blastit dev --port 18099"` (a real pty; `NO_COLOR=1`;
carriage returns removed for readability), then `GET /` and `POST /products/9`:

```
  blastit dev · single-uid simulation · http://127.0.0.1:18099
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 2 routes
  view  GET, HEAD  /
  view  GET, HEAD, POST  /products/.*
03:37:47.569 INFO  boot                   stage0  seccomp=on  privilege=unprivileged-dev
  app        importing app:app
03:37:47.633 INFO  boot                   preload  privilege=unprivileged-dev  build=0.2.0  immortalize=on  immortalized=11735  immortalize_ms=13.0ms  mem_total=32768MiB  mem_used=13467MiB  max_view_processes=500
03:37:47.633 INFO  reload                 1 directory
03:37:47.633 INFO  ready                  serving  routes=2  workers=16
03:37:53.559 INFO  process.spawned        home  pid=1422188 uid=1000  fork=0.3ms
03:37:53.562 INFO  request                GET / → home  200  3.3ms
GET / -> 200
03:37:53.573 INFO  process.spawned        products  pid=1422191 uid=1000  fork=0.3ms
03:37:53.575 INFO  request                POST /products/9 → products  200  2.9ms
POST /products/9 -> 200
```

The `reload` line: `INFO  reload                 1 directory` — singular, and its
summary starts in the same column as `ready`, `boot`, `process.spawned` and
`request` (one `NAME_COLUMN` of 23). `  app        importing app:app` is the
phase line printed before the blocking stage-1 import.

### piped banner (stdout is not a terminal)

`... blastit dev --port 18100 2>&1 | cat`:

```
  blastit dev · single-uid simulation · http://127.0.0.1:18100
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 2 routes
  view  GET, HEAD  /
  view  GET, HEAD, POST  /products/.*
03:38:02.135 INFO  boot                   stage0  seccomp=on  privilege=unprivileged-dev
  app        importing app:app
03:38:02.193 INFO  boot                   preload  privilege=unprivileged-dev  build=0.2.0  immortalize=on  immortalized=11735  immortalize_ms=16.8ms  mem_total=32768MiB  mem_used=12169MiB  max_view_processes=541
03:38:02.194 INFO  reload                 1 directory
03:38:02.194 INFO  ready                  serving  routes=2  workers=16
```

Same bytes as the pty run: the banner is plain text and the columns are the
same on both streams.

### IPv6 banner + `--learn` badge

`... blastit dev --host ::1 --port 18101 --learn | head -8`:

```
  blastit dev · single-uid simulation · http://[::1]:18101  [LEARN MODE]
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 2 routes
  view  GET, HEAD  /
  view  GET, HEAD, POST  /products/.*
```

`... blastit dev --port 18102 --learn | head -3`:

```
  blastit dev · single-uid simulation · http://127.0.0.1:18102  [LEARN MODE]
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 2 routes
```

### Production refusal is text on stderr, stdout stays empty

stdout redirected to a file, stderr left on a pty (`script -qec "sh -c '... blastit > prod.out'"`):

```
blastit: init runs as uid 1000 and is missing cap_chown, cap_dac_override, cap_fowner, cap_kill, cap_setgid, cap_setuid.
  A rootless deployment gives the capabilities to a *dedicated copy* of the binary, never to the shared one:
  cp <the blastit binary> <the binary>-init
  chown <the uid init runs as>:<its gid> <the copy>
  chmod 0500 <the copy>
  setcap 'cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setpcap+ep' <the copy>
  then start <the copy>, and set BLASTIT_EXEC=<the blastit binary> so that `--role=dbproxy` runs the capability-free copy
  BLASTIT_EXEC
  To run the same tree locally without root, use `blastit dev`; it is a single-uid simulation, not a sandbox
=== exit 1
--- stdout file (0 bytes) ---
```

## Verification (real command tails)

```
$ cargo test -p blastit --locked --features python --test critical_dev_reload --test critical_static_cli
running 23 tests
...
test the_boot_line_is_one_pretty_line_and_never_an_event ... ok
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.17s
...
running 11 tests
test a_static_mount_the_suite_never_requested_is_a_warning_not_a_finding ... ok
test an_all_static_site_boots_under_dev_with_no_app ... ok
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.72s
```

```
$ cargo test -p blastit --locked --features python --test critical_dev_and_fail_closed
running 20 tests
test dev_boots_five_unprivileged_roles_and_strips_secrets ... ok
test one_route_banner_singularises ... ok
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.41s
```

```
$ cargo test -p blastit-events --locked
running 100 tests                   (lib: 100 passed, 0 failed)
test result: ok. 100 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.47s
running 5 tests                     (tests/lifecycle_build.rs: 5 passed)
running 1 test                      (doc-tests: 1 passed)
```

```
$ make test-critical
protected critical tests hold no skip construct
cargo build --locked -p blastit-py -p blastit-dbproxy --features blastit/python
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.99s
python -m unittest discover -s python/tests/critical
.....................................................
----------------------------------------------------------------------
Ran 53 tests in 11.099s

OK
```

```
$ uv run --no-project tools/critical_guard.py --base 7caf56a --node MB.40.5.1
  M  crates/blastit/tests/critical_dev_and_fail_closed.rs  [authorized by crates/blastit/tests/critical_dev_and_fail_closed.rs]
  M  crates/blastit/tests/critical_dev_reload.rs  [authorized by crates/blastit/tests/critical_dev_reload.rs]
  M  crates/blastit/tests/critical_static_cli.rs  [authorized by crates/blastit/tests/critical_static_cli.rs]
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.40.5.1
```

`cargo check -p blastit --all-targets --features python --locked`:
`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.31s` — zero
warnings.

## Follow-ups

- None out of scope. The `blastit check` `pg.refused` quieting from this node's
  goal list is carried by the `EventDrain` in `check.rs` (only `warn` and above,
  one pretty line apiece) and needed no separate change here.

---

# Review round 2 — the five findings, fixed

## 1. [BLOCKER] the drain is now stopped on every path
`EventDrain` gained `impl Drop` (idempotent through `handle.take()`), so the
`?` arms and every early return join the thread and forward its bytes; the
success path also stops it explicitly right after `proxy.stop()` and before the
`code != 0` return. The doc comment now claims exactly what happens.

**Root cause found while proving it:** the drain wrapped the *pipe* read end in
a `UnixStream`, whose `Read` calls `recv(2)` — a pipe is not a socket, so every
read failed with `ENOTSOCK` and the thread exited with zero bytes. Probed live:

```
DRAIN-PROBE write fd=13
DRAIN-PROBE start fd=12
DRAIN-PROBE read error: Socket operation on non-socket (os error 88)
```

`Dbproxy::start` now returns the `std::io::PipeReader` itself (no `UnixStream`,
no `OwnedFd` round-trip), and the drain reads it with `read(2)`.

## 2. [MAJOR] one emitter, the run's level, the shared forwarding path
`execute` builds **one** `Emitter` (`Emitter::new(stderr, config.log_format,
config.log_level, colour.stderr)`) and the drain forwards each line through
`Emitter::forward(&Pipe::Events { component: Component::Dbproxy }, line)`. The
hand-rolled `decode_json` + hardcoded `Level::Warn` + `eprintln!` is gone; the
component restamp, the dropped-line warning and the configured level filter are
the emitter's. `check::run`/`execute` take the `Colour` the composition root
already decided (`cli::main`), so nothing re-probes TTY/env.

Real end-to-end `blastit check` (offline fixture: a stub `pytest`, a `postgres`
store pointed at a dead port, the suite's own connection refused by dbproxy):

```
$ uv run --no-project python /tmp/opencode/mb4051/refused_check.py
--- BLASTIT_LOG_LEVEL=warn · exit 1 ---
--- stdout ---
blastit check: 1 requests, 0 statements, 1/1 routes
  route home: 1 grant never used
  suite exercised 0 routes or 0 proxy statements
  proxy db: declared, 0 statements observed
blastit check: 3 problems · `--json` for the machine-readable list
--- stderr ---
03:50:12.658 WARN  pg.refused             *  db  upstream_unreachable
--- pg.refused lines on stderr: 1 ---
```

and the filter the old code ignored:

```
$ uv run --no-project python /tmp/opencode/mb4051/refused_check.py error
--- BLASTIT_LOG_LEVEL=error · exit 1 ---
--- stderr ---

--- pg.refused lines on stderr: 0 ---
```

## 3. [MAJOR] `e2e/dx_replay.py` matcher follows the widened column
`r"INFO  request\s+GET / → \S+  200 "` — matches the real 23-column line and
still matches a narrow one:

```
$ uv run --no-project python -m pytest e2e/ -k dx_replay --collect-only
collected 0 items
$ ... exec_module(e2e/dx_replay.py); re.findall(pattern, line)
imported e2e/dx_replay.py: dx_replay
matches the real 23-column line: ['INFO  request                GET / → home  200 ']
matches the old 12-column line: ['INFO  request     GET / → home  200 ']
```

(`dx_replay.py` is a script, not a pytest module, so the narrowest harness
invocation available is the import + regex proof above.)

## 4 & 5. [MINOR] clean refusal text
The bare `  BLASTIT_EXEC` item (`install.naming()` appended after the Binary
recipe) is gone from both branches, and the internal re-exec plumbing left the
user text in `stage0/mod.rs` and `host.rs` ("the database proxy runs the
capability-free copy"). Captured, on a pty, stderr only:

```
$ script -qec "sh -c '... blastit > prod.out'" /dev/null
blastit: init runs as uid 1000 and is missing cap_chown, cap_dac_override, cap_fowner, cap_kill, cap_setgid, cap_setuid.
  A rootless deployment gives the capabilities to a *dedicated copy* of the binary, never to the shared one:
  cp <the blastit binary> <the binary>-init
  chown <the uid init runs as>:<its gid> <the copy>
  chmod 0500 <the copy>
  setcap 'cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setpcap+ep' <the copy>
  then start <the copy>, and set BLASTIT_EXEC=<the blastit binary> so that the database proxy runs the capability-free copy
  To run the same tree locally without root, use `blastit dev`; it is a single-uid simulation, not a sandbox
=== exit 1
```

## 6. [MINOR] the banner assertion is no longer vacuous
`critical_dev_and_fail_closed.rs` no longer asserts `banner.len() == 3` (the
helper hard-caps at three). It locates the header line in `dev.stdout_text()`
and asserts a `  view  ` row follows it, then keeps the exact host/honesty/count
lines and the two-view table assertion.

## Verification (round 2)

```
$ cargo test -p blastit --features python --test critical_dev_and_fail_closed \
      --test critical_dev_reload --test critical_static_cli
running 20 tests   test result: ok. 20 passed; 0 failed
running 23 tests   test result: ok. 23 passed; 0 failed
running 12 tests   test result: ok. 12 passed; 0 failed

$ cargo test -p blastit --locked --lib check
test result: ok. 17 passed; 0 failed; 0 ignored; 248 filtered out

$ cargo check -p blastit --all-targets --features python --locked
Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.65s   (zero warnings)
```

`NAME_COLUMN` is untouched at 23 and `dev.reload.unavailable` is unchanged.

