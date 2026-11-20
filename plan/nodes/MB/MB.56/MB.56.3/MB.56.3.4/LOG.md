# Log MB.56.3.4

## 2026-11-20T18:45 [lead]

created (task) Create e2e_prod_refusals.rs (§6 fail-closed startup, env/WSGI refusals)

## 2026-11-20T19:52 [lead]

MUST COVER (from MB.56.3.2.2 review): PHP boot refusals that lost their only tests - unsupported minor (Debian 12 ships 8.2; sentence names the supported set), ZTS libphp refusal, shim-for-another-minor refusal before the listener, relative BLASTIT_LIBPHP refused, and 'no PHP server beside it / no php-fpm or socket between Blastit and PHP' (no second listener).

## 2026-11-20T19:52 [lead]

EXTENDED MUST-COVER list (arch, MB.56.3.2.2): libphp not-a-library/package sentence; ZTS refusal (static .dynsym, zero stage-0 dlopen canary); unsupported-minor sentence naming 8.3-8.6; shim built for another minor refused before the listener; probe hang killed by BLASTIT_PROBE_TIMEOUT_MS with its containment shape (pgid/uid/AF_INET EPERM); libphp/shim replaced between stage 0 and stage 1 (inode pin); no_libphp_is_mapped_above_the_view_worker (/proc maps). blastit-view-php and blastit-php-sapi now carry 0 tests.

## 2026-11-20T20:19 [lead]

MUST COVER (from MB.56.3.2.3): production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it - a Blastfile rewritten between stage-0 approval and stage-1 re-read must still be refused with the documented sentence and never reach ready (deleted with critical_raw_ingress).

## 2026-11-20T20:48 [lead]

status ready -> active

## 2026-11-20T20:48 [lead]

attempt 1/3 started

## 2026-11-20T20:48 [lead]

base 026024a59603d0595407c7019233ba888f567e1d

## 2026-11-20T20:56 [lead]

edited fields

## 2026-11-20T20:56 [lead]

RESUME: finish run_spawned zombie fix, make fast-check green, RESULT.md. ALSO: a symlinked Blastfile is NOT refused today (metadata() follows links) but the MB.56 epic goal lists symlink Blastfile as a 6 refusal - implement the refusal in crates/blastit/src (symlink_metadata, actionable sentence naming the path and 'replace it with a regular file') + a test.

## 2026-11-20T21:00 [lead]

edited fields

## 2026-11-20T21:00 [lead]

REVIEW FIX (ux): document the symlinked-Blastfile production refusal + the dev/check/fmt accept split (blastfile.md, troubleshooting row); document the FIFO 'not a regular file' row; document unknown --preset exit 2 + both presets. TEST GAP: production_refuses_a_symlinked_blastfile only exercises stage_one(...) with a synthesized document - add the real front-door boot assertion (stage0/mod.rs:420).

## 2026-11-20T21:00 [lead]

REVIEW FIX (ux): document the symlinked-Blastfile production refusal and the dev/check/fmt accept split (docs/reference/blastfile.md, docs/troubleshooting.md); document the FIFO 'not a regular file' row; document unknown --preset exit 2 naming both presets (docs/reference/cli.md). TEST GAP: production_refuses_a_symlinked_blastfile only drives stage_one() with a synthesized document - add the real front-door boot assertion for stage0/mod.rs:420.

## 2026-11-20T21:01 [lead]

edited fields

## 2026-11-20T21:01 [lead]

REVIEW FIX 2 (arch): REVERT the symlink refusal - a regular file can be swapped too (stage-1 re-read + TOCTOU test is the real barrier) and it rejects the canonical k8s ConfigMap/overlayfs layout; keep FIFO/device/dir. Makefile:203 --test no_python_build -> e2e_prod_refusals (+ run the #[cfg(not(feature=python))] legs); ci.yml:305 exclusion list names critical_dev_and_fail_closed (EXCLUDED_COUNT=2) -> red. Restore lost 6 legs: an_egress_false_view_cannot_open_inet, an_empty_grant_gets_a_denying_listener, only_a_granted_route_gets_a_pg_socket, json_mode_never_emits_a_port_key, lost-port retry, first-declared-route keeps all queries. no_php_server... must assert real sockets (/proc/<pid>/net/{tcp,unix}); prod refusals must use assert_never_served. SKIP lines invisible on pass -> make them visible.

## 2026-11-20T21:07 [lead]

status active -> done
