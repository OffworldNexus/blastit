# Log MB.42.2.4

## 2026-11-19T17:18 [unknown]

created (task) Integration glue: make install staging, test-critical staging, and one-binary four-runtime E2E

## 2026-11-19T21:30 [lead]

status ready -> active

## 2026-11-19T21:30 [lead]

attempt 1/3 started

## 2026-11-19T21:30 [lead]

base 4dc0c2f569d60195bf6b682b0175294ab4d156f7

## 2026-11-19T22:54 [lead]

status active -> review

## 2026-11-19T22:58 [lead]

REVIEW PENDING FIXES (filtered, to apply after MB.42.7): MAJOR PHP leg proves a build-tree shim not the bundle (BLASTIT_PHP_SHIM_DIR=target/php-sapi = Development scope) so an empty PHP bundle still passes -> assert libblastit_php* beside the binary, drop the env, export BUNDLE_REQUIRE_PHP_SHIMS=1; MAJOR blastit-lsp ships unchecked (rs-no-python ELF-checks only blastit); MAJOR glibc-floor probe greps prose (grep --max-glibc) not the option parser; BLOCKER SKIP exits 0 so make check passes without proving PHP (--allow-skip never in check); BLOCKER '1 PHP shim(s)' pluralisation; MAJOR no next command + no why-share/blastit/python sentence; minors: tautological assert_version, e2e prefix under target/ defeats no-dev-fallback claim, bundle layout not closed for stray dirs, weak fold assertion, universal-smoke duplication, unconditional capabilities line, closing banner on failure, FAIL-vs-SKIP inconsistency, two fail texts.
