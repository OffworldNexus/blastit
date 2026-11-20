# Log MB.56.3.3

## 2026-11-20T18:45 [lead]

created (task) Create e2e_check_fmt.rs (§5 check/fmt/learn, exit table) and drop the audit_v1 shim

## 2026-11-20T18:54 [lead]

edited fields

## 2026-11-20T19:52 [lead]

MUST COVER (from MB.56.3.2.2 review): 'blastit check validates the libphp gate/preflight' promised by docs/frameworks/php/{index,laravel,symfony}.md - the old check_reaches_the_same_discovery_gate test was deleted.

## 2026-11-20T19:52 [lead]

ALSO (from MB.56.3.2.2 arch review): critical_dev_reload.rs:2473-2545 keeps its own drifting host-PHP probe (walks only [85,84,83], no libphp8.6) - when absorbing that file, delete the copy and call tests/common/php_host::host_php().
