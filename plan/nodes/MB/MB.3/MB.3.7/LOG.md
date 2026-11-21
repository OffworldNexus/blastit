# Log MB.3.7

## 2026-11-14T01:09 [planner]

created (task) Prove FastCGI, the documented commands and the php2shell suite end-to-end in Docker

## 2026-11-14T03:37 [planner]

edited fields

## 2026-11-14T13:24 [lead]

HELD pending MB.12's embed-vs-pool decision (owner 2026-11-14: 'use the goddamn libphp and bypass php-fpm if you need to'). Do not implement, do not fail, do not delete work: the node is frozen as-designed against the FastCGI/php-fpm model and will be re-read, amended or superseded once the architect decides. If a sub-item reads as 'needed regardless' (e.g. the pure codec crate), it is called out in the MB.3 inventory log.

## 2026-11-14T14:06 [lead]

node body corrected (2026-11-14) after MB.3.9.1's review: the page no longer carries php-fpm.conf/mkdir/absolute-listen and no longer uses uv init/uv add, so the e2e replay clauses naming those are replaced. INI assertions now target the GENERATED pool file, and only if the submitted mechanism keeps a pool (if MB.12 lands embed, the equivalent is that dev starts the PHP worker with the derived config and two routes get two). Still HELD pending MB.12.

## 2026-11-14T14:39 [unknown]

edited fields

## 2026-11-14T14:39 [lead]

ADR-0010 supersedes the pool model for PHP: [route.php] is first-class (embedded NTS libphp.so + per-minor C SAPI shim in the unprivileged view worker); [route.fastcgi] is an external operator-managed fallback (entry + listen, no workers, loud weaker-mode warning); php-fpm pool machinery is dropped, not deferred. Retarget this node per the ADR's Node Changes table; do not implement pool ctl/PoolReady/parentfd.

## 2026-11-14T19:06 [lead]

INPUT from MB.3.25/native probe (2026-11-14): e2e/php-sapi/seccomp-exec installs SeccompRole::View{egress} because the harness predates MB.3.25. The PHP worker now installs EmbeddedView, and EmbeddedView denies execve (R14) BY DESIGN, so no harness can install it and then exec the probe ('Operation not permitted (os error 1)' at execve — itself correct). To make the shipped e2e test the exact PHP role, the probe must accept --role embedded-view and install the filter itself at startup (single-threaded, before php_module_startup), not inherit it across exec. Today the egress proof transfers because View and EmbeddedView share the identical socket/egress branch (MB.3.25's pinned tables); the exact-role proof is native in-process: AF_INET=EPERM(1), AF_UNIX=0, clone3=ENOSYS(38), execve=EPERM(1).

## 2026-11-15T09:37 [lead]

status ready -> active

## 2026-11-15T09:37 [lead]

attempt 1/3 started

## 2026-11-15T09:37 [lead]

base 466a9a1

## 2026-11-15T10:05 [lead]

status active -> review

## 2026-11-15T10:05 [lead]

H-007 answered and closed by the owner (conductor): THE ADR WINS — pin the entry by (st_dev, st_ino) at boot, do not amend ADR-0010 §3. Reasons: the mechanism already exists for libphp/shim (consistency, not new design); the provenance gate reduces exploitability but does not make the ADR true, and this milestone has twice refused 'safe enough that the documentation can lie'; and a process able to write manifest_dir could swap the executed script between requests, a different trust boundary from 'the operator owns the deploy'. Filed as MB.3.56, priority 1, release-blocking, ahead of MB.3.18.

## 2026-11-15T10:11 [lead]

REVIEW FAIL (attempt 1). The suite is real (121 checks, all 24 probes at expected status, P4b control, swap leg, `make e2e` and `make check` green) and the three "unproven" items were honestly disclosed. Two MAJORs and a set of MINORs that decide whether the suite can actually fail:
MAJOR 1: NODE.md:72 names `check_php_entry_0000_refused_for_route_uid` as an `e2e/rootless/**` deliverable; it exists nowhere (`grep` finds only the NODE), and RESULT's "what is unproven" omits it. Either add it (root `chmod 000` the entry; a live `GET /users/42` must answer 5xx with zero body bytes and decoy 0 opens; restore) or list it in §7 with the reason.
MAJOR 2: `e2e/rootless/php_entry_swap.py:222-229` — `server.stop(); server.start(); recycled = get(...)` with no readiness gate, asserting `status != 200`; `entry_probe.py:31` prints `{"error": …}` with no `status` on a refused connection, so `None != 200` reports ok and a dead server passes. Poll for a response head, then require `500 <= status < 600`. Same for the first swap at `:213`.
MINORs that change whether a probe can fail (do these): P4b's control should invoke `php -d auto_prepend_file=/fixture/canaries/prepend.php` (the reviewer verified on this image that it prints PREPEND-RAN) rather than a `data://` URL, so the marker E7 watches is actually shown executable; E11's `absent` must require a dict body (`assert_exchange`), and add `("eq","script_filename", …)`; E14 must assert the WINNING value (the row says first-wins), or label it Rust-only; E24's "proxy-control headers dropped" is unmeasurable from the fixture — assert the response head carries no `X-Accel-Redirect`/`X-Sendfile` or relabel it; E4/E20 need the positive control that `GET /uploads/shell.jpg` is 200, else the 404s pass for a missing file; E9/E10 must say they can never fail on Linux NTS (research §B #13) and that only P4b carries the runtime claim.
THE "UNPATCHED RUNTIME" PREMISE IS NOT MET: the node demanded a pinned UNPATCHED php image precisely so "the runtime is not what saves us", and the fixture is Debian's current 8.4.27. Either pin a genuinely older/unpatched image and say which digest, or state plainly in README/RESULT that this suite proves the surface is ABSENT by construction (no CGI binary, no FastCGI param stream) and does not claim "even an unpatched runtime". Pick one and say which; do not leave the node's claim standing unearned.
Also: `RESULT.md:279` still names H-006 — H-007 supersedes it (H-006 is closed as duplicate, MB.3.56 is the fix); `RESULT.md:170`'s "replays every `$ ` command verbatim" overstates (3 commands: 1 executed, 2 explained) — say so; `e2e/rootless/run.py` has ~780 whitespace-only reformatted lines in a shared file — minimise to the real changes; `:264`'s "restored entry is the same inode" is a property of the `mv` round-trip, not the worker — compare an inode the worker opened or drop it; `:293-306`'s inside-decoy `200 DECOY-RAN` is `info`, not asserted — keep it as the MB.3.56 witness, say so.

## 2026-11-15T10:11 [lead]

status review -> ready

## 2026-11-15T10:11 [lead]

status ready -> active

## 2026-11-15T10:11 [lead]

attempt 2/3 started

## 2026-11-15T10:26 [lead]

status active -> review

## 2026-11-15T10:33 [lead]

status review -> done
