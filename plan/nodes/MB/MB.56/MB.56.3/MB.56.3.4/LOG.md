# Log MB.56.3.4

## 2026-11-20T18:45 [lead]

created (task) Create e2e_prod_refusals.rs (§6 fail-closed startup, env/WSGI refusals)

## 2026-11-20T19:52 [lead]

MUST COVER (from MB.56.3.2.2 review): PHP boot refusals that lost their only tests - unsupported minor (Debian 12 ships 8.2; sentence names the supported set), ZTS libphp refusal, shim-for-another-minor refusal before the listener, relative BLASTIT_LIBPHP refused, and 'no PHP server beside it / no php-fpm or socket between Blastit and PHP' (no second listener).

## 2026-11-20T19:52 [lead]

EXTENDED MUST-COVER list (arch, MB.56.3.2.2): libphp not-a-library/package sentence; ZTS refusal (static .dynsym, zero stage-0 dlopen canary); unsupported-minor sentence naming 8.3-8.6; shim built for another minor refused before the listener; probe hang killed by BLASTIT_PROBE_TIMEOUT_MS with its containment shape (pgid/uid/AF_INET EPERM); libphp/shim replaced between stage 0 and stage 1 (inode pin); no_libphp_is_mapped_above_the_view_worker (/proc maps). blastit-view-php and blastit-php-sapi now carry 0 tests.

## 2026-11-20T20:19 [lead]

MUST COVER (from MB.56.3.2.3): production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it - a Blastfile rewritten between stage-0 approval and stage-1 re-read must still be refused with the documented sentence and never reach ready (deleted with critical_raw_ingress).
