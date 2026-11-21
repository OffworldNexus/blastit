# Log MB.3.13.1

## 2026-11-14T17:04 [lead]

created (task) MB.3.13 part 1: PhpEmbedRuntime worker loop over view.chan: route_uid, EmbeddedView filter, max_requests recycling

## 2026-11-14T18:18 [lead]

status ready -> active

## 2026-11-14T18:18 [lead]

attempt 1/3 started

## 2026-11-14T18:18 [lead]

base 9e83cc0; milestone: a PHP fixture answers through view.chan. Prereqs: MB.3.11, MB.3.12, MB.3.5.4, MB.3.23, MB.3.25.

## 2026-11-14T18:20 [lead]

BLOCKED correctly (attempt 1): my brief claimed prereqs had landed, but MB.3.23 (WorkerRuntime), MB.3.24, MB.3.25 (EmbeddedView) are all 'ready', not done -- grep finds no WorkerRuntime/EmbeddedView in crates/. Second: the worktree lacked the architect's accepted docs and the post-merge plan commits because they were committed on release/0.2.1 AFTER the fold; merged now (ff8aba3), so ADR-0010/backend-runtimes.md are present. Corrected order: MB.3.23 (trait) -> MB.3.25 (EmbeddedView) -> MB.3.13.1 -> MB.3.13.2/.3 -> MB.3.13.4. Also: this leaf's paths need crates/blastit-view/Cargo.toml (php feature + blastit-php-sapi dep) which it does not have. Not re-dispatched until those land; no code was touched.

## 2026-11-14T18:20 [unknown]

edited fields

## 2026-11-14T18:20 [lead]

OWNERSHIP RESOLVED (owner, 2026-11-14): PhpEmbedRuntime + the php feature wiring are THIS leaf's; MB.3.13.4 owns only the protected suite. backend-runtimes.md §5 row 7 amended to say so, MB.3.13.4 retitled, and this leaf is the Cargo.toml/Cargo.lock lock owner for the split (paths now include crates/blastit-view/Cargo.toml).

## 2026-11-14T18:31 [lead]

edited fields

## 2026-11-14T18:31 [lead]

PATH REPAIR by lead (2026-11-14): the PhpEmbedRuntime worker cannot dlopen from blastit-view (#![forbid(unsafe_code)]), so the safe loader lands in blastit-php-sapi (as abi.rs:7 already says MB.3.13 owns the dlopen handle); added crates/blastit-php-sapi/src/**, crates/blastit/Cargo.toml (php = [blastit-view/php, dep:blastit-php-sapi]) and Cargo.lock (adding a dep edge rewrites blastit-view's [[package]] entry; --locked would otherwise fail). crates/blastit-view/tests/** added for the non-critical loop tests.

## 2026-11-14T19:04 [lead]

status active -> ready

## 2026-11-14T19:04 [lead]

edited fields

## 2026-11-14T19:04 [lead]

status ready -> active

## 2026-11-14T19:04 [lead]

attempt 1/3 started

## 2026-11-14T19:04 [lead]

base ecd797d; attempt 1 reset because the previous 'attempt' was a lead sequencing error (prereqs not landed), no code was touched. HOST NOW HAS php-config 8.4.27 + libphp8.4-embed: native iteration is possible (probe proves dlopen+ABI+hello/vars/exit/fatal natively; only php8.4-curl is absent, so egress.php stays a Docker row). Prereqs in code: MB.3.23 reviewed PASS (59e7999), MB.3.25 done (9e52b34).

## 2026-11-14T19:28 [lead]

REVIEW FAIL (attempt 1, fresh eyes, base 9e52b34). Acceptance all green, but 6 MAJOR + 12 MINOR. MAJOR: (1) php_view ChildIo.keep misses libphp/shim fds -> production SharedObject::Fd dlopen /proc/self/fd/N ENOENT (native harness uses Path, so it hid); (2) loader sink_body from_raw_parts(NULL,0) is UB; (3) worker_init sets initialized only on code==0, so a -2 (startup ran and failed) lets Drop dlclose libphp; (4) php child never pins HOME/TMPDIR to <runtime>/views/<slot>/tmp (R12); (5) fork_php_view has no caller: [route.php] is accepted at boot then Ctl::Spawn kills the container (no-python build) or forks a WSGI view (python build) -- .3 owns the wiring, so this is carried as a hard .3 blocker, not fixed here; (6) 4.6x leaf size -- LEAD DECISION: accepted as a recorded deviation (loader+runtime+fork are one coherent end-to-end path; retro-split would re-review 2.7k lines to no benefit), to be noted at the epic gate. MINORs being fixed incl. write_body-before-head, view_tmp validation, SCRIPT_NAME/PATH_INFO contract, body/header caps, loader symbol-size/lifetime and php_version NULL, credit-denial/bailout tests, and the new tests/ harness path added to this node.

## 2026-11-14T19:28 [lead]

edited fields

## 2026-11-14T19:28 [lead]

status active -> ready

## 2026-11-14T19:28 [lead]

attempt 2/3 started

## 2026-11-14T19:28 [lead]

attempt 2/3 started; dispatch carries the reviewer's exact MAJOR/MINOR list. Paths add crates/blastit-php-sapi/tests/** for the native harness the leaf genuinely needs.

## 2026-11-14T19:46 [lead]

REVIEW FAIL (attempt 2, fresh eyes). All 17 pass-1 items genuinely FIXED (M1 fd keep verified by a real re-exec'd ChildIo::prepare test that runs in make check; M2/M3/M4/m5-m12 fixed; independent invariants green). One NEW MAJOR: CANCEL is view-fatal -> no RESP_END_COST/DONE (scheduler keeps the slot), view exits 1, and max_inflight==1 means a fork+php_module_startup per client disconnect (DoS amplifier). Attempt 3 fixes it: cancelled state, no-op writes, forced Aborted, DONE(Aborted)+continue loop, and a second-request-after-abort test. 5 MINORs too. Toolchain flake named as MB.3.12's, not this node's.

## 2026-11-14T19:54 [lead]

REVIEW FAIL (attempt 3). Delta verified: all 6 items fixed except the CANCEL class is INCOMPLETE: an idle Frame::Cancel (no request in flight) still hits the Protocol arm and kills the worker; the scheduler can send a stale CANCEL for an already-DONE request (scheduler.rs:1923/3457/2781 + select at 4121), and the WSGI twin no-ops it (lib.rs:588-596). One-line fix: Some((_, Frame::Cancel)) => continue beside the stale-WINDOW arm, + extend the test (id 9 DONE, then CANCEL(9), then id 10 served). MINOR: the cancel test does not assert RESP_END_COST before DONE for id 9. Attempts exhausted (3/3) -> escalate to expert per the ladder, max_attempts 4.

## 2026-11-14T19:54 [lead]

edited fields

## 2026-11-14T19:54 [lead]

attempt 3/4 started

## 2026-11-14T20:21 [lead]

status ready -> done
