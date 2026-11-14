# Log MB.3.13.3

## 2026-11-14T17:04 [lead]

created (task) MB.3.13 part 3: Vtable wiring: the five shim symbols, blastit_php_minor() vs php_version() before the first request, and the tick floor for cpu_ns

## 2026-11-14T18:31 [lead]

edited fields

## 2026-11-14T18:31 [lead]

PATH REPAIR by lead: added crates/blastit-php-sapi/src/** because the five vtable symbols are handed over from there (abi.rs owns the extern declarations); no Cargo.toml change expected in this leaf (.1 added the dep).

## 2026-11-14T19:28 [lead]

HARD REQUIREMENT carried from MB.3.13.1 review (2026-11-14, MAJOR 5): fork_php_view now EXISTS but has NO caller. Today a [route.php] manifest is accepted at boot and then Ctl::Spawn either kills the container (no-python build: spawn_now -> fail('no Python worker runtime')) or forks a WSGI view for the PHP route (python build). This leaf must (a) add SpawnSpec::Php/PhpBinding + the Ctl::Spawn wiring to fork_php_view, (b) add the boot refusal for any php route this build/path cannot serve (naming libphp and BLASTIT_LIBPHP), and (c) reach the native fixture milestone: a [route.php] entry answered through view.chan with cpu_ns <= wall_ns on this host (libphp8.4 + target/php-sapi/libblastit_php84.so). Do not declare [route.php] support before (a)-(c). Also confirm the Stage-1 probe child (design R8) is either here or explicitly deferred with a reason.

## 2026-11-14T19:29 [lead]

edited fields
