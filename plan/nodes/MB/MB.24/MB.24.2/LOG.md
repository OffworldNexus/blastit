# Log MB.24.2

## 2026-11-17T01:26 [planner]

created (task) Design the per-route view pool: per_process_cap, spawn-on-demand, LRU and reload (ADR-0015)

## 2026-11-17T05:40 [lead]

status ready -> active

## 2026-11-17T05:40 [lead]

attempt 1/3 started

## 2026-11-17T05:40 [lead]

base bc0b492569015729e4fd69ae96a54386f23aaad3

## 2026-11-17T06:05 [lead]

status active -> review

## 2026-11-17T06:09 [lead]

REVIEW FAIL (attempt 1): MAJOR parallel-views.md:100-103 + :1187-1193 — "on a preloaded image each fork is milliseconds" is false for PHP: the child dlopens libphp and runs php_module_startup post-fork (php_view.rs:34-35, ADR-0010 dec.2), so §4.3's fork→READY→fork chain puts two PHP startups inside the 450 ms window and the PHP leg never warms the route. Fix: set prefork in the MB.24.4 PHP fixture and/or one warmup request before t0, assert the measured startup cost in MB.24.4/RESULT.md, delete the preloaded-image sentence. MINORs: (1) review-implementability.md:20 still says M3 NOT RESOLVED and quotes `other != route` while §4.4:702 has `other < route` and §9 says accepted — rewrite that row as resolved; (2) the frame paths amendment must also cover MB.23.5 or say MB.24.3 lands first; (3) no docs/decisions/README.md + docs/design/README.md index rows for ADR-0015/parallel-views — add a plan request; (4) §7:1169 R14 calls isolation unmodified although R10/§8.6 widen isolation.rs:62; §4.7:962-965 uses .map() as a statement (unused Option under -D warnings); §4.3 cites :1832 for the none-keyed branch; queued_for/queued_for_streams used but unspecified.

## 2026-11-17T06:09 [lead]

status review -> ready

## 2026-11-17T06:09 [lead]

status ready -> active

## 2026-11-17T06:09 [lead]

attempt 2/3 started

## 2026-11-17T06:16 [lead]

status active -> review

## 2026-11-17T06:19 [lead]

REVIEW FAIL (attempt 2): MAJOR parallel-views.md:1031 — the printed view_gone body iterates queued_for_streams(key.route) (&self borrow) while calling self.queue.remove/fail_request/release inside the loop: E0502, reproduced with rustc (edition 2024), so MB.24.3 cannot transcribe it; fix = collect to Vec first, mirroring today's scheduler.rs:3733-3744. MINORs: §4.6:920-923 still offers 'MB.23.5 lands first / either order compiles' against §1's committed order (delete: MB.23.5 lacks frame.rs, frame/src/lib.rs, frame/src/decoder.rs so TypeCode::Reload cannot exist); §1:148 + RESULT.md:104 wrongly say MB.23.5 paths list only frame/src/ctl.rs (11 paths incl. scheduler.rs/supervise.rs); review-implementability.md:37-53 reports stale nits m5/m6/m7/m10 already fixed on disk and misses the E0502; §7:1246 PHP leg's '<= control - 400 ms' names no control (the only control is the WSGI leg in another function) — measure a PHP MAX_WORKERS=1 control in the same function or drop the clause. Attempt 3 stays cheap because each round's findings are new and mechanical (no repeated finding, attempts not exhausted); expert escalation reserved for an attempt-3 failure.

## 2026-11-17T06:19 [lead]

status review -> ready

## 2026-11-17T06:19 [lead]

status ready -> active

## 2026-11-17T06:19 [lead]

attempt 3/3 started

## 2026-11-17T06:22 [lead]

status active -> review

## 2026-11-17T06:31 [lead]

status review -> done

## 2026-11-17T06:31 [lead]

reviewer PASS (attempt 3). Residual MINORs for the epic gate: (1) review-implementability.md:37 heading still says the residual MINORs are 'unchanged' although the round-5 annex marks m5/m6/m7/m10 fixed; (2) review-testability.md:41 still calls R14's isolation legs 'unmodified' while parallel-views.md:1241 says 'except the one widened leg R10 owns'; (3) the MB.23.5 order request should also ask that MB.23.5/NODE.md's Goal lose the sentence claiming Ctl::Reload 0x45 lives in blastit-frame.

## 2026-11-17T06:31 [lead]

committed 29fe7b4 on release/0.2.0
