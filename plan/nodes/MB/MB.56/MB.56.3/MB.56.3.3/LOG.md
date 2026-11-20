# Log MB.56.3.3

## 2026-11-20T18:45 [lead]

created (task) Create e2e_check_fmt.rs (§5 check/fmt/learn, exit table) and drop the audit_v1 shim

## 2026-11-20T18:54 [lead]

edited fields

## 2026-11-20T19:52 [lead]

MUST COVER (from MB.56.3.2.2 review): 'blastit check validates the libphp gate/preflight' promised by docs/frameworks/php/{index,laravel,symfony}.md - the old check_reaches_the_same_discovery_gate test was deleted.

## 2026-11-20T19:52 [lead]

ALSO (from MB.56.3.2.2 arch review): critical_dev_reload.rs:2473-2545 keeps its own drifting host-PHP probe (walks only [85,84,83], no libphp8.6) - when absorbing that file, delete the copy and call tests/common/php_host::host_php().

## 2026-11-20T20:19 [lead]

MUST COVER (from MB.56.3.2.3): the 9 critical_static_cli cases lost - a readable root under an owned 0700 ancestor is accepted on check AND dev; a root the caller cannot list is refused; a root that is a file is refused; a blastit app on an all-static manifest is refused; 'an all-static check refuses learn (nothing to learn: static mounts)'; a static mount the suite never requested is a warning not a finding; a missing mount root is named with its prefix before any output; a mixed tree without an app still names blastit_app; check without an app still runs the suite.

## 2026-11-20T20:28 [lead]

status ready -> active

## 2026-11-20T20:28 [lead]

attempt 1/3 started

## 2026-11-20T20:28 [lead]

base 5552b5aedb505225c47a7bcbc885c17f23b779c7

## 2026-11-20T20:40 [lead]

edited fields

## 2026-11-20T20:40 [lead]

REVIEW FIX 2 (ux): raw_ingress+ingress_mismatch kinds + ', N proxies' header; docs contradict --learn widening (blastfile.md:566-567 roadmap apology, cli.md:69, ci.md:117); fmt [PATH]; check -- ARGS positive; --learn after a failing suite leaves the file untouched (exit 3); --learn --dry-run --json wrote:false + no findings key; README:100 static-only clean prints nothing; pytest 2/3 rows or rename; loosen advisory line + add unexercised twin; PHP check exits 2 without pytest

## 2026-11-20T20:48 [lead]

status active -> done
