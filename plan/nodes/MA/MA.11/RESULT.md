# Result MA.11

## Summary

`pretty_renders_a_forwarded_event_as_the_human_line` no longer assumes an ordering the product does
not provide. The ingress ` request ` line and the forwarded `sql.denied` event reach stdout from
**different writers**, so `wait_for_event("sql.denied")` returning said nothing about whether the
request line was already there; the old code then snapped `stdout_text()` once and failed if the
line had not been flushed yet.

The test now polls for its own precondition, with the same 20 ms cadence and `EXIT_TIMEOUT` budget
as `wait_for_event`, through a new reusable `Server::wait_for_line(&self, needles: &[&str]) ->
Option<String>` placed next to `wait_for_event`. It returns the matching line, so every existing
assertion is applied to it unchanged: `→ data`, `200`, `trim_end().ends_with("ms")`, no `method=`,
not raw JSON (`"event":` absent, not starting with `{`). On timeout it panics with
`panic!("no request line in:\n{}", server.stdout_text())` — the same diagnosability as today. The
`log` used for the `sql.denied` and "only stage 0 lines may be JSON" assertions is now read **after**
the poll, so it is a superset of everything asserted below it, and both earlier `sql.denied` lines
are kept verbatim.

No timeout constant was raised, no assertion relaxed, reordered away or deleted, no dependency or
`unsafe` added, and no other test touched.

Measured, not merely argued: instrumented once to reproduce the old read (then reverted), the
single snapshot **missed** the request line on **10 of 40** runs on a quiet machine; with the poll,
`40/40` consecutive single-test runs passed (20 quiet + 20 under 2×`nproc` saturation), and the whole
`smoke` binary passed **31/31** as root exactly as `smoke (root)` runs it.

## Evidence

### Environment

```console
$ docker exec blastit-ma11-pg pg_isready -U blastit
/var/run/postgresql:5432 - accepting connections
$ docker port blastit-ma11-pg
5432/tcp -> 0.0.0.0:55432
5432/tcp -> [::]:55432
$ sudo --non-interactive id -u
0        # unprivileged caller is uid 1000; the test binary is built by uid 1000
```

Host gotcha that had to be handled before any run could work (see Follow-up 1): the test binary's
`RUNPATH` (from `crates/blastit/build.rs`, driven by `PYO3_PYTHON`) points into
`/home/remy/.local/share/uv/python/...`, but `$HOME` here is `drwx------`, so the dropped-uid
`dbproxy` child could not traverse to `libpython3.14.so.1.0`:

```console
11:48:59.867 ERROR app.output  stderr  /proc/self/exe: error while loading shared libraries: libpython3.14.so.1.0: cannot open shared object file
11:48:59.867 ERROR app.output  stderr  dbproxy exited before it was ready
```

Proof it was traversal and not the test: `chmod o+x /home/remy /home/remy/.local /home/remy/.local/share`
(recorded as `700 700 700`, restored to `700 700 700` afterwards, verified) made the identical
command pass:

```console
$ sudo --non-interactive env PYO3_PYTHON=.../bin/python3.14 BLASTIT_TEST_PG_URL=... <binary> pretty_renders_a_forwarded_event_as_the_human_line --exact --nocapture
test pretty_renders_a_forwarded_event_as_the_human_line ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 30 filtered out; finished in 0.22s
```

Build (as uid 1000, never root; `executable` for stem `smoke`):

```console
$ env PYO3_PYTHON=.../python3.14 LD_LIBRARY_PATH=.../lib cargo test -p blastit --locked --test smoke --no-run --message-format=json > /tmp/opencode/ma11-artifacts.json
$ readelf -d <binary> | grep RUNPATH
 0x000000000000001d (RUNPATH)  Library runpath: [/home/remy/.local/share/uv/python/cpython-3.14.10-linux-x86_64-gnu/lib]
$ # artifact: /home/remy/offworld-nexus/blastit/target/debug/build/blastit/98997e28a8a88429/out/smoke-98997e28a8a88429
```

### Acceptance 1 — the test waits for its own precondition (bounded poll, log preserved)

```console
$ git diff -- crates/blastit/tests/smoke.rs | grep -E '^\+' | head -30
+    fn wait_for_line(&self, needles: &[&str]) -> Option<String> {
+        let deadline = Instant::now() + EXIT_TIMEOUT;
+        while Instant::now() < deadline {
+            if let Some(line) = self
+                .stdout_lines()
+                .into_iter()
+                .find(|line| needles.iter().all(|needle| line.contains(*needle)))
+            { return Some(line); }
+            std::thread::sleep(Duration::from_millis(20));
+        }
+        None
+    }
+    let request = server
+        .wait_for_line(&[" request ", "GET /query"])
+        .unwrap_or_else(|| panic!("no request line in:\n{}", server.stdout_text()));
```

- Cadence/budget identical to `wait_for_event`: `Duration::from_millis(20)` and `EXIT_TIMEOUT`
  (30 s, untouched). No new constant, no raised timeout, `&self`, no deps, no `unsafe`.
- Timeout panics with the **full captured log** (`server.stdout_text()`), same wording as before.
- All six assertions are the originals, applied to `request` (now `String` from the helper rather
  than `&str` from a one-shot `log.lines()`); nothing was relaxed, deleted or reordered away.

### Acceptance 2 — the race is gone, measured not argued

**(a) The old read really was the bug** — temporary local instrumentation (reproduced the old
single snapshot at the same instant, printed whether it had the line, then reverted; `grep -c MA11`
on the delivered file is `0`):

```console
$ # instrumented build, 40 runs as root, CI env
run 03 old snapshot MISSED the line
run 09 old snapshot MISSED the line
run 10 old snapshot MISSED the line
run 15 old snapshot MISSED the line
run 17 old snapshot MISSED the line
run 20 old snapshot MISSED the line
run 23 old snapshot MISSED the line
run 24 old snapshot MISSED the line
run 28 old snapshot MISSED the line
run 37 old snapshot MISSED the line
quiet machine: old-snapshot present=30 missed=10 of 40
```

So the product does *not* order the two lines, and this is not a rare CI coincidence: 25 % of runs
here. (This also settles the stop-and-report question — there is no encoder flush guarantee to
respect, so no product fix is warranted.)

**(b) The delivered test, 20 consecutive runs as root, CI job's env exactly** (`PYO3_PYTHON` +
`BLASTIT_TEST_PG_URL`; the binary derives `LD_LIBRARY_PATH` itself, as the file header says):

```console
$ /tmp/opencode/ma11-loop-ci.sh <binary> 20 /tmp/opencode/ma11-runs-ci
run 01: PASS
...
run 20: PASS
20/20 runs: test result: ok. 1 passed (pass=20 fail=0)
real 0m4.570s
```

**(c) The same 20 runs with the machine saturated** (2 × `nproc` = 20 busy loops on 10 CPUs):

```console
$ ... spawn 20 busy loops ...; uptime
 12:50:29 up 1 day, 11:53,  2 users,  load average: 7.11, 4.09, 3.07
run 19: PASS
run 20: PASS
20/20 runs: test result: ok. 1 passed (pass=20 fail=0)
 12:50:43 up 1 day, 11:53,  2 users,  load average: 10.70, 5.03, 3.39
```

Headline: **20/20** plain consecutive runs; **20/20** more under load average 10.7 on 10 CPUs —
40/40 overall after the fix, versus 10/40 misses before it.

**(d) The whole `smoke` binary as root, exactly how `smoke (root)` invokes it**
(`--test-threads=1 --nocapture`, same env):

```console
$ sudo --non-interactive env PYO3_PYTHON=... BLASTIT_TEST_PG_URL=... <binary> --test-threads=1 --nocapture
test pretty_renders_a_forwarded_event_as_the_human_line ... ok
...
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.51s
```

### Acceptance 3 — is an unprivileged run meaningful?

No. `needs_root` short-circuits the test before the PostgreSQL branch is even reached:

```console
$ env PYO3_PYTHON=... BLASTIT_TEST_PG_URL=... <binary> pretty_renders_a_forwarded_event_as_the_human_line --exact --nocapture   # uid 1000
pretty format: skipped: needs root
test pretty_renders_a_forwarded_event_as_the_human_line ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 30 filtered out; finished in 0.00s
```

An unprivileged pass proves only that the guard works; every run quoted above is uid 0.

### Acceptance 4 — gates

```console
$ make check
guard: OK (2 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 3 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check                       # clean
cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.43s
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.55s
cargo test --workspace --all-targets --locked    # 83 "test result: ok" blocks, 0 failed
cargo test --workspace --doc --locked
38 fences: 26 executed, 12 accounted, 0 failed · 25.4s
uv run --no-project tools/plan.py validate
plan OK
exit=0

$ uv run --no-project tools/plan.py validate
plan OK
validate_exit=0

$ make critical-guard BASE=0f926da
critical-guard: 0f926da..working tree, 3 changed file(s)
  no change to a protected critical test
guard_exit=0
```

(`smoke` also runs unprivileged inside `make check`: `test result: ok. 31 passed` — all 31 skip
their bodies, as designed.)

### Acceptance 5 — what the next CI run must show

`smoke (root)` green, and inside it:

```text
test pretty_renders_a_forwarded_event_as_the_human_line ... ok
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

with `crates/blastit/tests/smoke.rs:1628` (the old `no request line in:` panic from PR #62) no
longer reachable except by a genuine 30 s absence of the line, which would still print the log.

## Files

- `crates/blastit/tests/smoke.rs` — `Server::wait_for_line` added next to `wait_for_event`; the test
  polls for its request line and reads `log` after that poll. Nothing else.

```
 crates/blastit/tests/smoke.rs | 43 ++++++++++++++++++++++++++++++++++++++-----
 1 file changed, 38 insertions(+), 5 deletions(-)
```

## Follow-ups

1. **Host gotcha (not a diff):** a local root run of `smoke` needs the interpreter reachable by the
   dropped uids. Here `$HOME` is `0700`, so `build.rs`'s `RUNPATH` (`$HOME/.local/share/uv/python/…`)
   is unreachable for `dbproxy`, which dies with "libpython3.14.so.1.0: cannot open shared object
   file" and fails *any* database smoke test before the assertion under study. CI is unaffected
   (`/opt/hostedtoolcache` is world-readable). I relaxed `o+x` on `/home/remy`, `/home/remy/.local`
   and `/home/remy/.local/share` for the duration (modes recorded `700`/`700`/`700`) and restored
   them to `700`/`700`/`700`, verified with `stat`. A one-line `docs/knowledge/LESSONS.md` entry
   would save the next person the diagnosis, but `LESSONS.md` is outside this node's `paths`, so I
   did not write it — worth a small follow-up node.
2. **`wait_for_line` is now generally useful** and unused elsewhere only because this node's scope is
   one test; other tests that snapshot `stdout_text()` after `wait_for_event(<different writer>)`
   likely carry the same latent race (e.g. any test pairing a forwarded component event with an
   ingress or lifecycle line). A `test-curator` sweep of `smoke.rs` for "`wait_for_event` then
   `stdout_text` then `find`" would be cheap insurance. Not changed here: out of scope.
3. `make check`'s `guard: OK (2 staged/untracked file(s) checked)` refers to the plan-tree files
   this session's orchestrator touched (`plan/BUDGET.md`, `plan/JOURNAL.md`,
   `plan/nodes/MA/MA.11/`), not to this node's diff, which is the single test file. `plan.py
   validate` reports `plan OK`, so MA.10's earlier "milestone done but has open children" blocker is
   no longer present.
4. Reviewer MINOR findings (both documentation only, addressed as comment rewordings after PASS, no
   logic change): the comments wrongly said "separate writers" — init is the only process that
   writes stdout (`src/logs.rs`), so they now say the two lines arrive over separate *pipes* and init
   writes whichever `poll(2)` reports ready first; and `wait_for_line` now states it returns the
   *first* matching line, so a caller needing a later distinct line must not reuse it as-is.
