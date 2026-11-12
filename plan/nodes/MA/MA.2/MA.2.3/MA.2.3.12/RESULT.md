# MA.2.3.12 — `free_port()` and load isolation in the protected `dev` suite

Status: **done** (attempt 1). Exactly one file changed:
`crates/blastit/tests/critical_dev_and_fail_closed.rs` (+376 / −33). No production code, no
timeout constant, no assertion condition, no skip/ignore.

## What actually flaked (three defects behind one symptom)

1. **The documented race.** `free_port()` binds `127.0.0.1:0`, drops the listener, and the child
   `bind`s the number later. Another process/leg wins the port → stage 1 reports
   `binding <host>:<port>: Address already in use (os error 98)` and exits 1.
2. **The readiness wait could not tell that failure from success.** `wait_for_ready()` looked for
   any stdout line *containing* `ready`. Stage 1 prints the failed bind on **stdout** as an
   `app.output` event, and `Address al**ready** in use` matches. The follow-up
   `TcpStream::connect` then succeeded — because the process that stole the port is listening
   there — so the suite talked to a stranger. That is why the *wrong* legs failed (`status 0`,
   empty bodies, foreign routes) and why `json_mode_emits_privilege_and_never_a_port_key` could
   fail in `make check` with no visible EADDRINUSE.
3. **Classification raced the pipe readers.** After `wait(2)` returned, the last lines (including
   the bind message) could still be in the pipe, so an exit could be reported as "unknown failure"
   with an empty transcript.

## The fix (single file)

| Change | Purpose |
|---|---|
| `PORT_ATTEMPTS = 3`; `Dev::start_with(..., ports: impl FnMut() -> u16)`; `Dev::start` delegates with `free_port` | bounded retry with a **fresh** port on `EADDRINUSE`; the port source is injectable so a test can drive the retry deterministically |
| `LaunchFailure { LostThePort, Exited(ExitStatus), Timeout }`, `Dev::settle()` | waits for `ready` *or* an early exit; a lost port is retried, any other early exit panics immediately with the whole transcript (previously: 60 s of silence) |
| `is_ready_event()` used by `settle()` **and** `wait_for_ready()` | the exact `ready` event (`"event":"ready"` or the human column token `ready`), never the substring in `already in use` |
| `Dev.stdout_reader/stderr_reader: JoinHandle`; `drain_output()` | join the readers (bounded 5 s, finished ones only) after the child exits, so classification and the panic message see the bind line |
| `run_dev_to_exit()` for the `python_fail` leg | same bounded retry, so the leg sees the failed `BLASTIT_APP` import it tests, not `EADDRINUSE` |
| sharper messages | `python_fail` assertion now says it tests the import "not `bind_tcp`" and names an `EADDRINUSE` as port theft; the occupied-port leg says it deliberately holds the listener; launch panics name "harness isolation failure, not a `<tag>` behaviour failure" |
| 2 new tests | `only_a_lost_bind_is_classified_as_a_lost_port` (exact stage-1 line, `127.0.0.1` **and** `0.0.0.0`; different port / different errno / the import failure are *not* swallowed) and `a_launch_that_loses_its_port_retries_with_a_fresh_one` (first attempt handed a held port, must retry and really serve) |

No `sleep`-only workaround: the only sleeps are the pre-existing 20 ms poll loops and the 5 ms
reader-drain poll; no `READY_TIMEOUT`/`EVENT_TIMEOUT`/`EXIT_TIMEOUT` was changed.

## Pre-fix rate on this host (base file from git, NOT the working tree)

Base file = `git show HEAD:crates/blastit/tests/critical_dev_and_fail_closed.rs`
(`sha256 fdb3a136…`, 1350 lines, 11 tests), built in place and then the fixed file restored
(`sha256 6f1f680a…`; final delivered file `sha256 fd70f8c1…` after the wording-only edits below).

```console
$ /tmp/opencode/ma2312/load.sh <base test binary> prefix 8 15   # 8 copies × 15 runs
PRE-FIX: total=120 failed=11        # 9.2 %
pre-fix failing tests:
      3 dev_boots_five_unprivileged_roles_and_strips_secrets
      3 learn_writes_routes_and_queries_on_sigterm
      2 fatal_startup_removes_the_ephemeral_dev_root
      2 one_route_banner_singularises
      2 only_a_granted_route_gets_a_pg_socket
      1 json_mode_emits_privilege_and_never_a_port_key
```

`1/120` is exactly the `json_mode_emits_privilege_and_never_a_port_key` failure the node records
from `make check`. Two representative traces:

```console
# prefix-w1-i6: the python_fail leg failed on the *bind*, and the message blamed the import
thread 'fatal_startup_removes_the_ephemeral_dev_root' panicked at …:955:
stage 1 reported the failed BLASTIT_APP import:
21:09:13.786 ERROR app.output  stderr  binding 127.0.0.1:39975: Address already in use (os error 98)

# prefix-w4-i1: a *victim* — this dev received other legs' requests through its stolen port
21:09:08.904 INFO  request     GET /sql → sql  200  62.2ms
21:09:08.906 INFO  request     GET /granted/sql → (unmatched)  200  79.6ms   # only_a_granted_route…
21:09:08.970 INFO  request     GET /extra/7 → (unmatched)  200  65.3ms       # learn_writes…
learn: 3 routes, 1 queries added                                             # assertion wants 1
```

## Post-fix acceptance

```console
$ for i in $(seq 20); do cargo test -p blastit --test critical_dev_and_fail_closed --locked; done
FINAL SEQ: GREEN=20 RED=0
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s

$ # 8 concurrent copies of the suite (cargo test, exactly as accepted)
wall=3s
      8 × test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
failed: 0/8

$ # same load as the pre-fix measurement: 8 copies × 15 runs, test binary directly
FINAL LOAD: total=120 failed=0
120 logs contain `13 passed; 0 failed`

$ # 8 copies × 10 runs with --nocapture, to prove the retry *fires* in the wild
NOCAP: total=80 failed=0 retries_observed=87 final_bind_failures=0
wild retries (excluding the deterministic lost-port test): 6
      1 bad-import
      2 json-no-port      <- the leg that failed in `make check`
      2 learn-multi-view
      1 one-route
```

So the race still happens on this host (≈7.5 % of loaded launches) — it is now absorbed by a
bounded fresh-port retry and reported as such, instead of failing an unrelated assertion.

```console
$ make check
exit=0
… test result: ok. 13 passed; 0 failed; …
cargo build --locked -p blastit-py -p blastit-dbproxy           Finished …
python -m unittest discover -s python/tests/critical
Ran 50 tests in 3.537s
OK
uvx --offline ruff check python tools                           All checks passed!
uvx --offline ruff format --check python tools                  43 files already formatted
uvx --offline zensical build --strict                           No issues found
uv run --no-project tools/plan.py validate                      plan OK
```

`critical_guard` in `make check` (report mode, `--base HEAD` without `--node`) still prints its
informational `[VIOLATION: not covered by any node path]`; the enforcing form passes:

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MA.2.3.12
critical-guard: OK: all 1 protected change(s) are authorized by node MA.2.3.12   (exit 0)
```

## Assertions / timeouts unchanged (proof)

`git diff -U0` removes exactly 33 lines. Every one is implementation or an **assertion message**:

```console
$ git diff -U0 crates/blastit/tests/critical_dev_and_fail_closed.rs | grep '^-' | grep -v '^---'
 1 -use std::process::{Child, Command, ExitStatus, Stdio};
 2 -        let dir = fixture(tag, manifest);
 3 -        let port = free_port();
 4 -        if let Some(out) = child.stdout.take() { … }          # spawn_reader moved/returned
 …
15 -        "stage 1 reported the occupied bind address:\n{stdout}"                    # message only
…
32 -        "failed import must exit 1\nstdout:\n{stdout}\nstderr:\n{stderr}"          # message only
33 -        "stage 1 reported the failed BLASTIT_APP import:\n{stdout}"                 # message only
$ git diff … | grep TIMEOUT     # only doc/message references to READY_TIMEOUT, no constant
```

The three condition expressions those messages belong to are byte-identical to HEAD:

```rust
stdout.contains(&format!("binding 127.0.0.1:{occupied_port}"))
stdout.contains("BLASTIT_APP (missing_app_module:application)")
output.status.code(), Some(1)
```

`wait_for_line`/`wait_for_event` keep `READY_TIMEOUT` and the 20 ms poll; only the panic wording and
the predicate plumbing changed.

## Follow-ups

- `docs/knowledge/LESSONS.md` is outside this node's `paths`, so the lead may want to add:
  *a readiness probe that matches the substring `ready` is satisfied by `Address already in use`
  (stage 1 prints it to stdout as an `app.output` event); match the event, and drain the child's
  pipes after `wait(2)` before classifying an exit (MA.2.3.12).*
- No other suite in `crates/*/tests/` uses a substring readiness probe (checked with grep), so this
  class is confined to this file.
- Pre-existing flakes unrelated to this node were not observed post-fix: the `learn` failures in the
  pre-fix logs were victims of a stolen port (foreign routes/requests), not the MA.2.3.8 `ECONNRESET`
  path.
