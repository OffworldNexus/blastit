# Log MB.3.12

## 2026-11-14T14:39 [lead]

created (task) crates/blastit-php-sapi: per-minor Apache-2.0 C SAPI shims (blastit_php_abi_v1) + critical tests

## 2026-11-14T15:08 [lead]

status ready -> active

## 2026-11-14T15:08 [lead]

attempt 1/3 started

## 2026-11-14T15:08 [lead]

base b2890c4; host has NO PHP toolchain (no php/php-config/libphp/headers) -- build.rs must degrade (cfg no_php_toolchain + a Rust refusal naming php-config + the package), the shim is built and exercised ONLY in the e2e Docker job (debian:trixie-slim + php8.4-dev + libphp8.4-embed), and a green make check here proves the Rust degrades, never that the C works. Carrier: MB.3.15 (R12 protected suite) and MB.3.16 (check on all-static python-less) created; MB.3.13 body now requires exposing blastit-view framing.

## 2026-11-14T15:29 [unknown]

edited fields

## 2026-11-14T15:29 [lead]

status active -> review

## 2026-11-14T15:29 [lead]

attempt 1 report: crate lands zero-dep (17 KiB shim, no DT_NEEDED on libphp, zend_first_try, SAPI_OPTION_NO_CHDIR, startup-stage INI lockdown, CLOCK_PROCESS_CPUTIME_ID), build.rs degrades naming php-config + php8.4-dev/libphp8.4-embed. Docker probe PASSED for 8.4.27 (200 / X-Probe / cpu_ns<=wall / reserved names absent / exit() and fatal contained / next request served / shim-before-libphp refused with undefined symbol: sapi_globals). paths extended to e2e/php-sapi/**, e2e/php_sapi_probe.py, .github/workflows/**. FOLLOW-UPS to route: (a) two LESSONS candidates -- php_execute_script swallows the bailout (false is the signal; the outer zend_first_try never fires for exit()/fatals) and CLOCK_PROCESS_CPUTIME_ID is tick-granular so cpu_ns>0 needs a >=1-tick fixture; (b) only 8.4 is compiled+loaded, 8.2/8.3 source-guarded but unbuilt; (c) MB.3.13 must call the five symbols and check blastit_php_minor() against php_version() before the first request.

## 2026-11-14T15:33 [lead]

REVIEW FAIL (attempt 1). Two MAJORs, both ADR-0010 SS2 deviations: (1) request.rs:23 DISABLED_FUNCTIONS omits pcntl_signal, posix_kill, posix_getpgid which the ADR mandates verbatim, and critical_shim_contract.rs:143 checks only 5 names so nothing catches the drift; (2) request.rs:293,358 allow_url_fopen=0 (ADR SS2, when !egress) is NEVER pinned and IniPolicy::new has no egress input, so MB.3.13 cannot express it and a route worker inherits distro allow_url_fopen=On -> userland egress bypassing the broker until the seccomp leg. MINORs: (a) C:451,470 php_module_startup/shutdown run outside any zend_*_try (not attacker-reachable, matches upstream, still wrap + return -2); (b) C:531,581-584 clock_gettime return ignored, cpu0/cpu1 indeterminate -> check -1 and report 0; (c) the 'another minor is refused' bullet is a pure predicate test, no probe loads a mismatched libphp -> exercise the real refusal or move it to MB.3.13's boot gate; (d) RESULT:153 follow-up is stale and the node frontmatter was self-edited -- never hand-edit frontmatter.

## 2026-11-14T15:33 [lead]

status review -> ready

## 2026-11-14T15:33 [lead]

attempt 2/3 started

## 2026-11-14T15:46 [lead]

status ready -> review

## 2026-11-14T15:46 [lead]

attempt 2: both MAJORs + four MINORs fixed. CRITICAL FINDING: sapi_startup() NULLS ini_entries, so attempt 1's entire INI lockdown was silently absent (open_basedir unset, allow_url_fopen=On, disable_functions empty) while everything looked green -- now assigned AFTER sapi_startup with the ordering pinned by a critical test. Egress proven in two layers: the real SeccompRole::View{egress:false} filter (installed by an e2e harness linking blastit-sys, so the shim crate stays zero-dep) refuses socket(AF_INET) with EPERM in the same process that runs PHP, INCLUDING with allow_url_fopen=1 (strong layer holds with the weak one removed). PHP loses errno on socket() failure, so the sandbox claim is carried by the in-process socket errno and the driver says which half proves what.

## 2026-11-14T15:57 [lead]

status review -> done

## 2026-11-14T15:57 [lead]

commit d990e0e (make check exit 0 on a PHP-free host; Docker probe green with the three new ini_get rows). Four review MINORs cleared: three INI keys now asserted via ini_get not the rendered string, cfg->ini NULL refused before sapi_startup (-1, pinned by a critical case), absolute_value rejects ':' so open_basedir can never render two entries, and e2e/php-sapi/.gitignore covers target/ + the harness lock (lock ignored, not committed: unpublished e2e tool).
