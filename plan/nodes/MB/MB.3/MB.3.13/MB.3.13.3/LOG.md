# Log MB.3.13.3

## 2026-11-14T17:04 [lead]

created (task) MB.3.13 part 3: Vtable wiring: the five shim symbols, blastit_php_minor() vs php_version() before the first request, and the tick floor for cpu_ns

## 2026-11-14T18:31 [lead]

edited fields

## 2026-11-14T18:31 [lead]

PATH REPAIR by lead: added crates/blastit-php-sapi/src/** because the five vtable symbols are handed over from there (abi.rs owns the extern declarations); no Cargo.toml change expected in this leaf (.1 added the dep).
