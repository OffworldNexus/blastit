# Result MB.15

## Summary

**Proof + pin.** The report that `SIGTERM` to `blastit dev`'s init PID orphans stage 1 and
leaves the port answering **does not reproduce** against a binary built from this checkout,
and it never could: the two facts below explain the whole observation.

* `blastit dev` `execve`s **in place** into `--stage=1`
  (`stage0::exec_stage1_dev`): the PID a caller starts *is* init, but its command line
  becomes `/proc/self/exe --stage=1 --config-fd=3 --dev-root=… --dev-host=…`, so
  `pgrep -f 'blastit dev --port N'` can **never** match it after boot.
* A probe whose leftover scan runs through `bash -c "… <pattern> …"` matches **its own
  shell's argv** (`pgrep` excludes itself, not its parent), which reads as a survivor that
  never existed; the same probe that lost `$!` to a wrapper then signals something that is
  not the server.

Delivered: three verbatim transcripts (SIGTERM, SIGINT, a second SIGTERM *during* the
drain, each against an all-static manifest served from `target/debug/blastit`), a new
standard-tier regression test (`crates/blastit/tests/dev_signal_reap.rs`, unprivileged,
~0.05 s, `Drop`-guarded) that pins the contract by PID identity rather than by a command
line, the `docs/knowledge/LESSONS.md` bullet, and a confirmation that `make docs-serve-check`
needs **no** change (it never had `setsid` or a process-group kill). `make check` is green.

## Acceptance 1 — formal reproduction

Built exactly as the acceptance asks (`$(RUST_ENV)` read out of the `Makefile` itself, so
the interpreter/loader settings are the gate's own):

```
$ eval "$(make --no-print-directory --eval 'print-rust-env: ; @echo "$(RUST_ENV)"' print-rust-env) cargo build -p blastit --locked"
   Compiling blastit v0.2.0 (/home/remy/offworld-nexus/blastit/crates/blastit)
   Compiling blastit-view v0.2.0 (/home/remy/offworld-nexus/blastit/crates/blastit-view)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.28s
```

The serve root is the all-static manifest `MB.14` dogfooded (`version = 1`, one
`[[route]]` with `path = ['.*']`, `[route.static] root = "site"`), with an `index.html` and
one subpage. The launcher is a **plain background launch** (`&`, no `setsid`, no wrapper);
`$!` is recorded and every later check is anchored on it. Leftover probes run from the
transcript script itself (`argv = bash transcript.sh`), never through
`bash -c "<pattern>"`: the self-match trap is real and produced a phantom in the *first*
draft of this very transcript (a `pgrep -f -- '--dev-root=.*-<pid>-'` launched via
`bash -c` printed the harness's own pid before the probe was moved inline).

### 1a. `SIGTERM` to the init PID only

```
# the all-static manifest the three runs serve:
$ cat /tmp/opencode/mb15/stage/blastit.toml
version = 1

[[route]]
name = "site"
path = ['.*']

[route.static]
root = "site"

=== sigterm ===
$  BLASTIT_MANIFEST=/tmp/opencode/mb15/stage/blastit.toml /home/remy/offworld-nexus/blastit/target/debug/blastit dev --port 9101 >/tmp/opencode/mb15/stage/sigterm.log 2>&1 &
$ echo $!
2236264
$ ps -o pid,pgid,cmd -p 2236264
    PID    PGID CMD
2236264 2236248 /proc/self/exe --stage=1 --config-fd=3 --dev-root=/run/user/1000/blastit-dev-1000-2236264-1979b14e3719859f --dev-host=127.0.0.1
$ curl -sS -o /dev/null -w 'GET /               -> %{http_code}\n' http://127.0.0.1:9101/
GET /               -> 200
$ curl -sS -o /dev/null -w 'GET /sub/page.html -> %{http_code}\n' http://127.0.0.1:9101/sub/page.html
GET /sub/page.html -> 200
$ # what the pid of a plain background launch is now: the argv of stage 1
$ tr '\0' '\n' </proc/2236264/cmdline
/proc/self/exe
--stage=1
--config-fd=3
--dev-root=/run/user/1000/blastit-dev-1000-2236264-1979b14e3719859f
--dev-host=127.0.0.1
$ ls -d /run/user/1000/blastit-dev-1000-2236264-1979b14e3719859f
/run/user/1000/blastit-dev-1000-2236264-1979b14e3719859f
$ # the children of the init pid before the signal
$ cat /proc/2236264/task/2236264/children
2236267 2236268
$ # the MB.14 trap, in situ: the launch cmdline is gone while the server serves
$ pgrep -af -- '/home/remy/offworld-nexus/blastit/target/debug/blastit dev --port 9101'
   (nothing matches: the process execve-d into --stage=1)
$ kill -TERM 2236264
=> port 9101 refuses connections after 103 ms
$ wait 2236264; echo "exit status: $?"
exit status: 0   (gone 104 ms after the signal)
$ ps -o pid,cmd -p 2236264
    PID CMD
   no such process
$ grep -lF -- '--dev-root=/run/user/1000/blastit-dev-1000-2236264-1979b14e3719859f' /proc/[0-9]*/cmdline
   no process names this dev root
$ pgrep -af -- 'blastit-dev-[0-9]*-2236264-'
   no process of this dev root is left
$ ls -d /run/user/1000/blastit-dev-*-2236264-*
ls: cannot access '/run/user/1000/blastit-dev-*-2236264-*': No such file or directory
   the dev root directory is gone
$ # the server log, whole
$ cat /tmp/opencode/mb15/stage/sigterm.log
  blastit dev · single-uid simulation · 127.0.0.1:9101
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
13:23:03.423 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
13:23:03.442 INFO  ready       serving  routes=1  workers=16
13:23:03.536 INFO  request     GET / → site  200  0.3ms
13:23:03.544 INFO  request     GET /sub/page.html → site  200  0.2ms
13:23:03.565 INFO  shutdown    drain
13:23:03.566 INFO  process.exited -  pid=2236267 uid=0  signal=15
13:23:03.566 INFO  shutdown    stopped
$ # the children recorded before the signal, each one now
$ ps -o pid=,cmd= -p 2236267
   2236267 gone
$ ps -o pid=,cmd= -p 2236268
   2236268 gone
```

`PGID 2236248` is the transcript's own process group: no `setsid`, no new group — the PID
that was signalled is the one `$!` named, and that same PID is the stage-1 init whose
`--dev-root` carries the PID in its name.

(The child's `signal=15` in the logs below is not a copy-paste slip: the drain
`SIGTERM`s its children whichever signal init itself received — `supervise::progress_shutdown`
→ `signal_all(Signal::TERM)` — and init's own exit status is `0`.)

### 1b. `SIGINT`

```
=== sigint ===
$  BLASTIT_MANIFEST=/tmp/opencode/mb15/stage/blastit.toml /home/remy/offworld-nexus/blastit/target/debug/blastit dev --port 9101 >/tmp/opencode/mb15/stage/sigint.log 2>&1 &
$ echo $!
2236319
$ ps -o pid,pgid,cmd -p 2236319
    PID    PGID CMD
2236319 2236248 /proc/self/exe --stage=1 --config-fd=3 --dev-root=/run/user/1000/blastit-dev-1000-2236319-70942eb8102fbd06 --dev-host=127.0.0.1
$ curl -sS -o /dev/null -w 'GET /               -> %{http_code}\n' http://127.0.0.1:9101/
GET /               -> 200
$ curl -sS -o /dev/null -w 'GET /sub/page.html -> %{http_code}\n' http://127.0.0.1:9101/sub/page.html
GET /sub/page.html -> 200
$ # what the pid of a plain background launch is now: the argv of stage 1
$ tr '\0' '\n' </proc/2236319/cmdline
/proc/self/exe
--stage=1
--config-fd=3
--dev-root=/run/user/1000/blastit-dev-1000-2236319-70942eb8102fbd06
--dev-host=127.0.0.1
$ ls -d /run/user/1000/blastit-dev-1000-2236319-70942eb8102fbd06
/run/user/1000/blastit-dev-1000-2236319-70942eb8102fbd06
$ # the children of the init pid before the signal
$ cat /proc/2236319/task/2236319/children
2236322 2236323
$ # the MB.14 trap, in situ: the launch cmdline is gone while the server serves
$ pgrep -af -- '/home/remy/offworld-nexus/blastit/target/debug/blastit dev --port 9101'
   (nothing matches: the process execve-d into --stage=1)
$ kill -INT 2236319
=> port 9101 refuses connections after 104 ms
$ wait 2236319; echo "exit status: $?"
exit status: 0   (gone 105 ms after the signal)
$ ps -o pid,cmd -p 2236319
    PID CMD
   no such process
$ grep -lF -- '--dev-root=/run/user/1000/blastit-dev-1000-2236319-70942eb8102fbd06' /proc/[0-9]*/cmdline
   no process names this dev root
$ pgrep -af -- 'blastit-dev-[0-9]*-2236319-'
   no process of this dev root is left
$ ls -d /run/user/1000/blastit-dev-*-2236319-*
ls: cannot access '/run/user/1000/blastit-dev-*-2236319-*': No such file or directory
   the dev root directory is gone
$ # the server log, whole
$ cat /tmp/opencode/mb15/stage/sigint.log
  blastit dev · single-uid simulation · 127.0.0.1:9101
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
13:23:03.731 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
13:23:03.749 INFO  ready       serving  routes=1  workers=16
13:23:03.843 INFO  request     GET / → site  200  0.4ms
13:23:03.851 INFO  request     GET /sub/page.html → site  200  0.2ms
13:23:03.872 INFO  shutdown    drain
13:23:03.873 INFO  process.exited -  pid=2236322 uid=0  signal=15
13:23:03.873 INFO  shutdown    stopped
$ # the children recorded before the signal, each one now
$ ps -o pid=,cmd= -p 2236322
   2236322 gone
$ ps -o pid=,cmd= -p 2236323
   2236323 gone
```

### 1c. A second `SIGTERM` **during** the drain

An idle dev boot drains in ~2 ms, so the first attempt at this leg signalled a process that
was already gone (`No such process`). The window is widened honestly instead of by racing:
a client asks for an 8 MiB body and reads none of it, so one response is in flight and the
drain waits for it (`BLASTIT_GRACEFUL_TIMEOUT=5` is the budget, and the drain still ends
early — when the client goes away — rather than waiting it out).

```
=== second-sigterm-during-drain ===
$ BLASTIT_GRACEFUL_TIMEOUT=5 BLASTIT_MANIFEST=/tmp/opencode/mb15/stage/blastit.toml /home/remy/offworld-nexus/blastit/target/debug/blastit dev --port 9101 >/tmp/opencode/mb15/stage/second-sigterm-during-drain.log 2>&1 &
$ echo $!
2236374
$ ps -o pid,pgid,cmd -p 2236374
    PID    PGID CMD
2236374 2236248 /proc/self/exe --stage=1 --config-fd=3 --dev-root=/run/user/1000/blastit-dev-1000-2236374-8f28df2ddbfef51a --dev-host=127.0.0.1
$ curl -sS -o /dev/null -w 'GET /               -> %{http_code}\n' http://127.0.0.1:9101/
GET /               -> 200
$ curl -sS -o /dev/null -w 'GET /sub/page.html -> %{http_code}\n' http://127.0.0.1:9101/sub/page.html
GET /sub/page.html -> 200
$ # what the pid of a plain background launch is now: the argv of stage 1
$ tr '\0' '\n' </proc/2236374/cmdline
/proc/self/exe
--stage=1
--config-fd=3
--dev-root=/run/user/1000/blastit-dev-1000-2236374-8f28df2ddbfef51a
--dev-host=127.0.0.1
$ ls -d /run/user/1000/blastit-dev-1000-2236374-8f28df2ddbfef51a
/run/user/1000/blastit-dev-1000-2236374-8f28df2ddbfef51a
$ # the children of the init pid before the signal
$ cat /proc/2236374/task/2236374/children
2236377 2236378
$ # the MB.14 trap, in situ: the launch cmdline is gone while the server serves
$ pgrep -af -- '/home/remy/offworld-nexus/blastit/target/debug/blastit dev --port 9101'
   (nothing matches: the process execve-d into --stage=1)
$ head -c 8388608 /dev/zero > /tmp/opencode/mb15/stage/site/big.bin; ls -l /tmp/opencode/mb15/stage/site/big.bin
-rw-r--r-- 1 remy remy 8388608 Nov 14 14:23 /tmp/opencode/mb15/stage/site/big.bin
$ exec 9<>/dev/tcp/127.0.0.1/9101; printf 'GET /big.bin HTTP/1.1\r\nHost: mb15\r\n\r\n' >&9
$ # a response is now in flight and unread; signal, then signal again mid-drain
$ kill -TERM 2236374; sleep 0.3
$ ps -o pid=,stat=,cmd= -p 2236374
2236374 S    /proc/self/exe --stage=1 --config-fd=3 --dev-root=/run/user/1000/blastit-dev-1000-2236374-8f28df2ddbfef51a --dev-host=127.0.0.1
$ grep -c 'shutdown    drain' /tmp/opencode/mb15/stage/second-sigterm-during-drain.log
1
$ kill -TERM 2236374 && echo 'second SIGTERM sent while the first drain is in flight'
second SIGTERM sent while the first drain is in flight
$ sleep 0.3; ps -o pid=,stat= -p 2236374 && echo 'still draining, still alive: the second SIGTERM neither wedged it nor skipped the reap'
2236374 S
still draining, still alive: the second SIGTERM neither wedged it nor skipped the reap
$ grep -c 'shutdown    stopped' /tmp/opencode/mb15/stage/second-sigterm-during-drain.log || true
0
$ # let the in-flight response finish: the drain then completes on its own
$ exec 9<&-; exec 9>&-
=> port 9101 refuses connections after 737 ms
$ wait 2236374; echo "exit status: $?"
exit status: 0   (gone 738 ms after the signal)
$ ps -o pid,cmd -p 2236374
    PID CMD
   no such process
$ grep -lF -- '--dev-root=/run/user/1000/blastit-dev-1000-2236374-8f28df2ddbfef51a' /proc/[0-9]*/cmdline
   no process names this dev root
$ pgrep -af -- 'blastit-dev-[0-9]*-2236374-'
   no process of this dev root is left
$ ls -d /run/user/1000/blastit-dev-*-2236374-*
ls: cannot access '/run/user/1000/blastit-dev-*-2236374-*': No such file or directory
   the dev root directory is gone
$ # the server log, whole
$ cat /tmp/opencode/mb15/stage/second-sigterm-during-drain.log
  blastit dev · single-uid simulation · 127.0.0.1:9101
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
13:23:04.037 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
13:23:04.056 INFO  ready       serving  routes=1  workers=16
13:23:04.150 INFO  request     GET / → site  200  0.3ms
13:23:04.159 INFO  request     GET /sub/page.html -> site  200  0.2ms
13:23:04.188 INFO  shutdown    drain
13:23:04.815 INFO  request     GET /big.bin → site  200 client_gone  628.0ms
13:23:04.817 INFO  process.exited -  pid=2236377 uid=0  signal=15
13:23:04.817 INFO  shutdown    stopped
$ # the children recorded before the signal, each one now
$ ps -o pid=,cmd= -p 2236377
   2236377 gone
$ ps -o pid=,cmd= -p 2236378
   2236378 gone
```

Read the log timestamps: `drain` at `04.188`, the second `SIGTERM` at ~`04.5` with the
process alive and `stopped` still absent (`grep -c` → `0`), and `stopped` at `04.817` — so
the second signal landed **inside** the drain, where `supervise::begin_shutdown` returns
early (`if self.shutting_down { return; }`) instead of restarting or escalating it. The exit
stayed `0`, the port closed, the children were reaped, and the root was removed.

### The original observation was an artifact (one paragraph)

The reported orphan was not the product: it was an artifact of (a) a probe wrapper that
lost `$!` — the PID it later signalled was a shell, not the server — and (b) the in-place
`execve` that makes `blastit dev` become `--stage=1` under the *same* PID, so a
`pgrep -f`/`pkill -f` on the launch command line (`blastit dev --port N`) can never match
the server after boot and returned "no process" for a process that was in fact running and
never signalled. Add the third trap this session walked into itself: `pgrep -f` run through
`bash -c "… <pattern> …"` matches the shell that carries the pattern in its own `argv`, so a
leftover scan can report a survivor that never existed. Signalled directly by PID, `blastit
dev` drains in ~2 ms, exits `0`, closes the port, reaps http and the scheduler, and removes
its `--dev-root` — on `SIGTERM`, on `SIGINT`, and on a second `SIGTERM` sent mid-drain.

## Acceptance 2 — the regression test

**Picked: a new file, `crates/blastit/tests/dev_signal_reap.rs`** (standard tier), not an
added `#[test]` in `smoke.rs`, for two reasons. `smoke.rs` is the *root production* harness
— every test in it starts with `if !needs_root(…) { return; }`, i.e. it declares "skipped:
needs root" and passes, which is precisely the pattern the protected tier forbids and the
wrong shape for a contract that must hold for every unprivileged caller. And the code is
not cheaper there: the fixture, launcher, HTTP client and pid identity checks are ~200
lines that would bloat a 2 800-line file about a different mode. The existing protected
suite (`critical_dev_and_fail_closed.rs`) already asserts *part* of this (SIGTERM → exit
`0`, root removed) for a proxy manifest; the new contract points — the port refusing, no
descendant surviving, SIGTERM to the PID you started being the init PID — belong in a new
standard-tier file, and no protected file had to change.

What it does (no `sudo`, no fixed port, no sleeps-as-assertions, `Drop`-guarded):

1. spawns `env!("CARGO_BIN_EXE_blastit") dev --port <free_port>` with a temp all-static
   manifest (`BLASTIT_MANIFEST`), `BLASTIT_GRACEFUL_TIMEOUT=5`, `BLASTIT_LOG_FORMAT=json`,
   `env_clear()` + the two loader/interpreter variables, and
   `BLASTIT_MEM_HIGH_WATERMARK=100` (a sizing knob: a host above its watermark refuses to
   boot at all, which is not this behaviour — the same knob the protected dev harness sets);
2. waits for `GET /` to answer `200` with a deadline, then asserts the PID it started is
   stage 1 (`--stage=1` in that PID's `/proc/<pid>/cmdline`) and reads the run's
   `--dev-root` from it;
3. records every descendant recursively from `/proc/<pid>/task/*/children` **with each one's
   `starttime`** (field 22 of `/proc/<pid>/stat`) — identity, not a pid alone, so a recycled
   pid cannot pass for a survivor;
4. `SIGTERM` to that PID only, then asserts: the port refuses within
   `BLASTIT_GRACEFUL_TIMEOUT` + 10 s, `waitpid` returns within the same bound with exit `0`,
   the log holds `"stage":"drain"` and `"stage":"stopped"`, no recorded descendant is alive
   (same pid *and* same start time), and the `--dev-root` directory is gone.

```
$ eval "$(make --no-print-directory --eval 'print-rust-env: ; @echo "$(RUST_ENV)"' print-rust-env) cargo test -p blastit --test dev_signal_reap --locked"
   Compiling blastit v0.2.0 (/home/remy/offworld-nexus/blastit/crates/blastit)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.35s
     Running tests/dev_signal_reap.rs (target/debug/build/blastit/…/out/dev_signal_reap-…)

running 1 test
test sigterm_to_the_dev_init_pid_reaps_the_tree_and_closes_the_port ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

Zero skips: the file contains no `#[ignore]`, no `needs_root`, no early `return` — a
failure to boot fails the test with the boot's own JSON log.

### Negative control — the test fails when the contract is broken

Run against a **scratch copy** (`/tmp/opencode/mb15/mutate/proj`, a throwaway cargo project
that compiles the same file, with `CARGO_BIN_EXE_blastit` pointing at this checkout's
binary; nothing in the repo was mutated), with two mutations:

```
$ cp ../nosignal.rs tests/signal.rs   # `dev.signal(libc::SIGTERM)` → `dev.signal(0)`
$ CARGO_BIN_EXE_blastit=…/target/debug/blastit cargo test --offline --test signal
thread 'sigterm_to_the_dev_init_pid_reaps_the_tree_and_closes_the_port' panicked at tests/signal.rs:101:9:
127.0.0.1:42543 still answers 15.002639207s after SIGTERM
{"ts":"…","event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"unprivileged-dev","preset":null}
{"ts":"…","event":"ready","component":"init","stage":"serving","routes":1,"workers":16}
{"bytes_in":0,"bytes_out":49,…,"event":"request",…,"method":"GET",…,"path":"/",…,"status":200,…}
test result: FAILED. 0 passed; 1 failed
```

```
$ cp ../wrongexit.rs tests/signal.rs  # `Some(0)` → `Some(1)` for the expected status
thread 'sigterm_to_the_dev_init_pid_reaps_the_tree_and_closes_the_port' panicked at tests/signal.rs:118:5:
assertion `left == right` failed: a drained dev boot exits 0, not ExitStatus(unix_wait_status(0))
  left: Some(0)
 right: Some(1)
test result: FAILED. 0 passed; 1 failed
```

The unmutated copy in the same scratch project passes (`1 passed`), so the two failures are
the assertions biting, not the scratch harness.

## Acceptance 3 — `docs/knowledge/LESSONS.md`

One bullet, in the file's own style (bolded lead-in, node id at the end):

```
$ git diff -- docs/knowledge/LESSONS.md
+- **`blastit dev` execve's in place into stage 1 — the PID is stable but the command line is not:** so `pkill -f 'blastit dev …'` finds nothing after boot; signal the PID you started, and do not wrap the launch in a way that loses it. The process is `/proc/self/exe --stage=1 … --dev-root=<run/user|tmp>/blastit-dev-<uid>-<init pid>-<nonce>`, so a leftover is identified by that root (the nonce follows the pid, so a `-<pid>`-anchored-to-end pattern never matches), and a `pgrep -f` whose pattern reaches the *checking* shell's own `bash -c` argv matches itself and reads as an orphan that never existed. (`MB.15`.)
```

## Acceptance 4 — Makefile: **no change needed**, and why

`make docs-serve-check` already tears the server down the way this node asks for, and it
never contained the `setsid`/process-group kill the node's context attributed to `MB.14`:

```
$ grep -n "setsid\|process group\|process-group" Makefile
(no output)
$ git show 9fab605 -- Makefile | grep -n "setsid\|kill -TERM\|kill -KILL\|pgrep"
128:+			kill -TERM "$$pid" 2>/dev/null || true; \
133:+				kill -KILL "$$pid" 2>/dev/null || true; \
144:+			leftovers="$$(pgrep -f -- "blastit-dev-[0-9]*-$$pid-" 2>/dev/null | tr '\n' ' ' || true)"; \
```

So the recipe is (from `Makefile:235-262`) `kill -TERM "$pid"` → a 5 s bounded `kill -0`
wait → `SIGKILL` only as a last resort → `wait` → "the port must refuse" → "no process of
this run may survive", with the leftover scan anchored on `$pid`
(`blastit-dev-[0-9]*-<init pid>-`). That is exactly acceptance 4's target state, and the
transcripts above plus the new test confirm it is sufficient: nothing had to be relaxed or
added, and the "no process left behind" assertion is untouched. **The Makefile is
unmodified.**

One detail worth recording: that leftover scan is *not* the self-match trap. The recipe's
script text keeps `$pid` unexpanded in the shell's own `argv` (make turns `$$pid` into
`$pid`), so `pgrep -f` cannot match the shell that runs it — unlike the transcript's first
draft, which substituted the raw number and did match itself.

## Acceptance 5 — no leftover claim that `SIGTERM` orphans stage 1

```
$ grep -rniE "orphan" --include=*.md --include=*.rs --include=Makefile . | grep -v "^./target/"
… (only unrelated hits: HTTP frame fixtures, manifest group names, `<pid>/children` "orphan
PID 1 inherited", e2e `docker compose --remove-orphans`, e2e droplet cleanup, and this
node's own new test/LESSONS text)
$ grep -rniE "blastit dev.*(sigterm|signal)|sigterm.*blastit dev" --include=*.md --include=Makefile . | grep -v "^./target/" | grep -v "^./plan/"
./docs/design/ma-lean-dx.md:28  … `0` clean `SIGINT`/`SIGTERM` …          (correct, and now pinned)
./docs/testing/AUDIT-MA.2.3.md:32  `--learn` not reliably exit `0` …       (a different, fixed MA.2.3 defect)
./Makefile:242  "warning: blastit dev ignored SIGTERM for 5 s; sending SIGKILL"   (the last resort)
```

Nothing inside this node's paths claims an orphan. `MB.14`'s `RESULT.md` follow-up (in
`plan/`, outside `paths`) already frames it correctly — "`blastit dev` execve's into
`--stage=1` (tooling lesson, not a product bug)" — so it needs no correction either.

## `make check`

```
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
uv run --no-project tools/guard.py
guard: OK (14 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 12 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
… cargo clippy --workspace --all-targets --locked -- -D warnings   (Finished, no warnings)
… cargo doc --workspace --no-deps --locked                          (Finished, `-D warnings`)
… cargo test --workspace --all-targets --locked
     Running tests/dev_signal_reap.rs (…)
running 1 test
test sigterm_to_the_dev_init_pid_reaps_the_tree_and_closes_the_port ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
…
56 fences: 29 executed, 27 accounted, 0 failed · 21.7s
uv run --no-project tools/plan.py validate
plan OK
$ echo $?
0
```

### Runs of the gate, and one intermittent failure that is not this diff

Four `make check` invocations in total: the first stopped at `rs-fmt` on the **new** file
(mine — `cargo fmt --all` fixed it and every run after it is clean), the second failed one
pre-existing unit test in a file this node does not touch, and the last two are green on the
final tree (the excerpt above is the last one).

```
---- supervise::tests::a_late_ctl_write_to_a_gone_scheduler_is_best_effort stdout ----
thread '…' panicked at crates/blastit/src/supervise.rs:1045:14:
a closed peer cannot take the message: ()
test result: FAILED. 177 passed; 1 failed
make: *** [Makefile:124: rs-test] Error 101
```

`crates/blastit/src/**` is untouched by this diff (`git status` above) and the case is
"deterministic on purpose" per its own doc comment. Re-run in isolation and three times as
a whole suite it is green:

```
$ cargo test -p blastit --lib --locked supervise::tests::a_late_ctl_write_to_a_gone_scheduler_is_best_effort
test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 178 filtered out; finished in 0.00s
$ for i in 1 2 3; do cargo test -p blastit --lib --locked; done
test result: ok. 178 passed; 0 failed; 1 ignored  (×3)
```

Filed as a follow-up (a `write(2)` on a `UnixStream::pair()` whose peer was just dropped
returning `Ok` under load — `write_ctl`'s `Ok(0) => Ok(())` arm is the only way that test can
see a success).

## Files

* `crates/blastit/tests/dev_signal_reap.rs` — new standard-tier suite (one test), described
  above.
* `docs/knowledge/LESSONS.md` — one bullet (`MB.15`).
* `Makefile` — **unchanged** (see acceptance 4).
* `crates/blastit/src/**`, protected `critical_*` suites, `plan/`, other crates — untouched.

## Follow-ups

* **`supervise::tests::a_late_ctl_write_to_a_gone_scheduler_is_best_effort` is intermittently
  red** (`crates/blastit/src/supervise.rs`, out of this node's `paths`): the second of four
  `make check` runs in this session failed it with `a closed peer cannot take the message: ()`,
  i.e. the second
  `write_ctl` returned `Ok` after the peer was dropped. Either the `Ok(0) => Ok(())` arm is
  being hit (a zero-byte write treated as success) or a `write(2)` on a just-closed
  `UnixStream::pair()` can succeed under load; the case should distinguish "peer gone" from
  "wrote nothing", and it belongs to a `crates/blastit/src/**` node.
* **`NODE.md`'s account of `MB.14` is wrong on one point** (`plan/`, not my `paths`):
  `MB.14` never shipped `setsid` or a process-group kill — the committed teardown is already
  a plain `kill -TERM "$pid"` plus a bounded wait. The "therefore" in MB.15's context
  describes a plan that was described in prose but not in the Makefile. Nothing to undo.
* **`pgrep`/`pkill` in a `bash -c` recipe matches its own shell** — worth a line in the
  vetted-command checklist for future nodes that write teardown/leftover probes; the fix is
  to keep the pattern out of the checker's `argv` (a variable in a script, or an anchored
  pattern that the wrapper's own text cannot contain).
* A stale worktree `.worktrees/docs-do-cluster/` holds a duplicate of `docs/` (including
  `docs/knowledge/LESSONS.md` and `docs/testing/AUDIT-MA.2.3.md`); a repo-wide grep for the
  old wording sees two copies. Out of `paths`; `make mutants-clean`-style hygiene for
  worktrees is a `MB.1.11` concern.

## Review (goldfish: PASS with three MINORs)

The third finding (`NODE.md`'s Context still describing the `setsid` workaround) was fixed by
the lead; the other two are in `crates/blastit/tests/dev_signal_reap.rs` only.

1. **`Drop for Dev` now cleans up after a failure path that `SIGKILL`s init.** `Dev` records
   the run's `--dev-root` (read out of the child's own argv by `dev_root`) and the
   descendants it counted (`descendants`), and `Drop` now `SIGKILL`s each recorded descendant
   *whose `starttime` still matches* (so a recycled pid is never signalled, and ESRCH is
   irrelevant), then `fs::remove_dir_all`s the root when it is still a directory — best
   effort, no panic in `Drop`, no `unwrap`.

   The leak was real and is visible on this host from the **pre-fix** runs:

   ```
   $ ls -ld --time-style=+%Y-%m-%dT%H:%M /tmp/blastit-dev-1000-2240662-* /tmp/blastit-dev-1000-2280555-*
   drwx------ 3 remy remy 60 2026-11-14T14:24 /tmp/blastit-dev-1000-2240662-63d8fede392ce6fc
   drwx------ 3 remy remy 60 2026-11-14T14:31 /tmp/blastit-dev-1000-2280555-1d141430c5f31a77
   ```

   Those are the two pre-fix `signal(0)` mutant runs. With the fix in place, two more failing
   mutant runs leave the filesystem identical:

   ```
   $ ls -d /tmp/blastit-dev-* /run/user/1000/blastit-dev-* | sort > before.txt   # 25 entries
   $ … failing `signal(0)` mutant run (panics at "still answers 15.00s after SIGTERM") …
   $ ls -d … | sort > after.txt
   $ diff before.txt after.txt && echo identical
   identical: 25 dev roots before, 25 after two failing mutant runs
   $ bash scan.sh        # a script, so the pattern is not in the checker's own argv
   (nothing: no process holds a --dev-root)
   ```

2. **A lost ephemeral port is retried, not reported as a slow boot.** `Dev::start` mirrors
   `critical_dev_and_fail_closed.rs::run_dev_to_exit`: `PORT_ATTEMPTS = 3`, a
   `lost_the_port(port, log)` matcher for stage 1's `binding <host>:<port>: Address already in
   use (os error 98)` line, and a fresh `free_port()` per attempt — with the failure named as
   a harness isolation problem, not this contract, when every attempt loses. The retry path is
   deterministic to prove: a scratch mutation whose `free_port` keeps the listener bound
   (`mem::forget`) makes every launch lose, and the test then says exactly that:

   ```
   blastit dev: port 44571 was taken between free_port() and the child's bind (EADDRINUSE); retrying with a fresh port (1/3)
   blastit dev: port 44573 was taken between free_port() and the child's bind (EADDRINUSE); retrying with a fresh port (2/3)
   panicked: blastit dev could not bind 3 ports in a row: another process kept taking the port between free_port() and the child's `bind`. A harness isolation failure, not this contract.
   {"event":"app.output",…,"line":"binding 127.0.0.1:36795: Address already in use (os error 98)"}   # the log the matcher reads
   ```

Post-fix: `cargo fmt --all -- --check` clean, `cargo clippy -p blastit --all-targets -- -D
warnings` clean, `cargo test -p blastit --test dev_signal_reap` → `1 passed … 0.04s` three
times in a row, and `make check` at `4cd727b` → exit `0`.
