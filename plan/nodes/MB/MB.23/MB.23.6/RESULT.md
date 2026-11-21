# Result MB.23.6

## Summary

The `MB.23` closing audit is `docs/testing/AUDIT-MB.23.md` (371 lines, the `AUDIT-MB.22` house format): **0 BLOCKER, 5 MAJOR, 13 MINOR**. The gates are green (`make check` exit 0 in 266 s with zero `FAIL|FAILED`), the `--no-reload` documentation fence is executed (`executed=True`), both ban guards pass on the standalone binary **and** on the wheel's real `_blastit*.so`, two `strace` transcripts prove `check` and a production boot create no fanotify group, and `dev_reload.rs` kills **84.8 %** of its mutants (123 caught / 145 viable, 22 missed, 35 unviable, 0 timeout) — above the 75 % floor. The dropped behaviour is genuinely gone: no test asserts a single-view restart, an unchanged untouched pid, or a per-route map. Nine child tasks (`MB.23.7`–`MB.23.15`) file every open gap, including the inherited `policy_out_of_step` exec gap, the unwritten `T31`, `T18`'s missing `200` half, `T17`/`T20`/`T23`, the absent container row, the scheduler/frame proof gaps and the unread panic. No test, crate or script was edited.

## Evidence

### 1. The documentation fence, executed (`--no-reload`)

```console
$ make docs-check
No issues found                       # EXIT=0
$ make docs-unit
107 passed, 65 subtests passed in 2.51s
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags  4.3s  each dev flag boots and stops (...)
48 fences: 30 executed, 18 accounted, 0 failed · 38.5s   # EXIT=0
$ UV_OFFLINE=1 uv run --no-project python /tmp/opencode/exec_report.py docs/reference/cli.md
EXECUTED docs/reference/cli.md fence 1 mode=dev-flags ok=True executed=True seconds=4.4
4 fences: 4 executed, 0 accounted, 0 failed · 10.3s
```

`dev-flags` loops over every `$ ` line of the fence, and `docs/reference/cli.md:20` is `$ blastit dev --no-reload`; `executed=True` is that line's verdict (it boots, names its port, stops on `SIGTERM`).

### 2. Both guards, plus the frozen files

```console
$ scripts/check-no-inotify.sh target/debug/blastit
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)
$ scripts/check-no-inotify.sh target/docs-wheel/unpacked/blastit/_blastit.cpython-314-x86_64-linux-gnu.so
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)
$ cargo tree -p blastit -e normal --locked | grep -Ei '\b(notify|inotify)\b'          # empty
$ grep -rniE "inotify|BLASTIT_DEV_RELOAD" docs/local.md docs/reference/ docs/troubleshooting.md   # empty
$ git diff --stat e7b9581 HEAD -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs        # empty
```

The wheel path is the real ELF unpacked from the wheel `make docs-test` built; `make docs-test` itself leaves only the `.whl`, so the acceptance command as written finds nothing on a fresh gate (filed as `MB.23.14`). `strace` 6.13 was installed on this host for this audit.

### 3. Production, the gate host, and the live reload

```console
$ strace -f -e trace=fanotify_init,fanotify_mark -o /tmp/opencode/check.strace target/debug/blastit check -- ./scripts/noop.py
1 passed in 0.01s ; blastit check: 1 problem · `--json` ...   # exit 1, completeness finding
FANOTIFY_LINES=0
$ strace -f -e trace=fanotify_init,fanotify_mark -o /tmp/opencode/prod-boot.strace timeout -s TERM 20 target/debug/blastit
PROD_EXIT=1 (fails closed) ; FANOTIFY_LINES=0
$ target/debug/blastit dev --port 18098
reload: on (1 directories)
$ target/debug/blastit dev --no-reload --port 18097
(no `reload: on` line)
$ cargo test -p blastit --features python --test critical_dev_reload php -- --nocapture
reloaded (php, /usr/lib/libphp8.4.so + target/php-sapi/libblastit_php84-x86_64-gnu.so): 2 view(s), 1 file(s), both routes serve v2; a warm compiled artefact needs no reload
```

The node's literal `check -- ./scripts/noop` cannot run: the repo root has no `Blastfile` and `scripts/noop` does not exist. The substitution (a fixture with `Blastfile`, `app.py`, `scripts/noop.py`) still reaches pytest and the completeness engine; the defect is filed as `MB.23.14`.

### 4. Suite and epic gate

```console
$ make test-critical
# 47 `test result: ok` lines, 0 failed; Ran 53 tests in 10.840s — OK (no php_embed probe flake this run)
$ make check
MAKE_CHECK_EXIT=0 SECONDS=266 ; FAIL_LINES=0
test result: ok. 23 passed; 0 failed ... (critical_dev_reload)
Ran 53 tests in 11.830s ; 48 fences: 30 executed, 18 accounted, 0 failed ; zed-check: OK ; plan OK
$ cargo test -p blastit --lib --test critical_dev_reload   # after the mutation run
289 passed; 2 ignored ; 23 passed; 0 failed
```

### 5. Mutation floor and cleanup

```console
$ cargo mutants --in-place --file crates/blastit/src/dev_reload.rs --features python \
    --timeout 120 --build-timeout 600 -o /tmp/opencode/mutants-mb236 -- --locked --lib --test critical_dev_reload
180 mutants tested in 56m: 22 missed, 123 caught, 35 unviable
# 123 / (123 + 22) = 84.8 % killed, 0 timeout
$ make mutants-clean          # clean: freed 0 B
$ git status --short crates scripts Makefile Cargo.toml Cargo.lock   # empty
$ pgrep -af "cargo-mutants|cargo mutants|mutants"                   # empty
```

All 22 survivors are classified in the audit §3.3: 6 equivalents, 3 dead/dead-return, 1 weak accessor, 3 weak oracles, 9 meaningful. The `blastit-sys` pure helpers (mask encoding, errno ladder, handle keying, `parse_records`) were audited by reading plus their own unit/protected tests; the raw syscall bodies are out of the floor as the node allows.

### 6. Determinism and the dropped behaviour

```console
$ grep -n "sleep(" crates/blastit/tests/critical_dev_reload.rs
# 23 hits: 17 bounded `sleep(Duration::from_millis(...))` inside deadline loops, 6 `sleep(SETTLE)` (750 ms)
```

The 6 `SETTLE` windows are negative-verdict windows, documented and followed by a positive control in 4 of 6 rows; filed as MINOR (`MB.23.13`) because the node's acceptance says no fixed sleep may decide a verdict. No test asserts a single-view restart, an unchanged untouched pid, or a per-route map (audit §2.1). The three inherited gaps are recorded as **not fixed** with nodes: (a) `policy_out_of_step` not carried across the exec (`MB.23.7`), (b1/b2) idempotent reload + streaming `0x45` (`MB.23.15`), (c) the unread panic (`MB.23.13`).

## Files

- `docs/testing/AUDIT-MB.23.md` (new)
- `plan/nodes/MB/MB.23/MB.23.6/RESULT.md` (this file)
- `plan/nodes/MB/MB.23/MB.23.7/` … `MB.23.15/` (nine follow-up nodes, via `tools/plan.py`)

No `crates/`, `scripts/`, `Makefile`, `Cargo.toml` or `Cargo.lock` byte changed; `target/docs-wheel/unpacked/` is generated under git-ignored `target/`.

## Follow-ups

| Node | Severity | Gap |
|---|---|---|
| `MB.23.7` | MAJOR | carry the policy channel's out-of-step taint in the adoption document |
| `MB.23.8` | MAJOR | prove `T31` (a save during the cycle) and the `reconcile` window |
| `MB.23.9` | MAJOR | prove `T18`'s `200` half under the real PostgreSQL CI job |
| `MB.23.10` | MAJOR | `T17` production/check fd+hook, `T20`/`T23` keep-alive |
| `MB.23.11` | MAJOR | epic item 4 in a real container (default seccomp) |
| `MB.23.12` | MINOR | the 9 meaningful mutation survivors and the weak/dead accessors |
| `MB.23.13` | MINOR + unknown | six `SETTLE` verdicts, ack bound/duplicate/stale docstring, the unread panic campaign |
| `MB.23.14` | MINOR | the two unrunnable acceptance commands (`scripts/noop`, un-unpacked wheel) |
| `MB.23.15` | MINOR | `MB.23.5.1`'s idempotent-reload and streaming-`0x45` proof gaps |

`uv run --no-project tools/plan.py validate` is green with all nine nodes.
