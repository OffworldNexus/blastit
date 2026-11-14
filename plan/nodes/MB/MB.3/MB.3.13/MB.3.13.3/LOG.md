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

## 2026-11-14T23:24 [lead]

hand-off from MB.3.13.2: step 3 (the MB.3.28 interim refusal) is the last boot step and it is what keeps the dev/prod asymmetry. When you delete it, also delete the Development fall-through (a library that is absent or has no shim must be fatal in dev too, with ADR-0010 s4.1's message) and run the discovery gate from check.rs so 'blastit check' reaches it rather than step 3.

## 2026-11-14T23:32 [lead]

edited fields

## 2026-11-14T23:32 [lead]

lead: paths widened to crates/blastit/tests/** — deleting the MB.3.28 refusal makes the protected critical_php_backend.rs (and no_python_build.rs) red; their rewrite to 'a PHP route boots and serves 200 over view.chan' is authorized with critical_guard --node MB.3.13.3, as stage0.rs's own doc comment already says.

## 2026-11-15T00:00 [lead]

carry from MB.3.35: on successful discovery nothing logs the resolved tuple, so 'found the shim' is invisible without gdb. Emit one line (boot event) naming (PHP X.Y, <arch>-<libc>, libphp path, shim path) when a [route.php] route resolves — it is the DX proof that make php-sapi's artifact is the one the boot picks.

## 2026-11-15T00:09 [lead]

status ready -> active

## 2026-11-15T00:09 [lead]

attempt 1/3 started

## 2026-11-15T00:09 [lead]

base 34d0bb7ade2fac13fa2376936f698c95fb050be2

## 2026-11-15T00:23 [lead]

status active -> review

## 2026-11-15T00:28 [lead]

REVIEW FAIL (attempt 1): MAJOR discovery follows symlinks while stage 1 opens O_NOFOLLOW, so a symlinked libphp passes stage 0 then dies after the dev banner with ELOOP and no s4.1 sentence; MAJOR the protected suite branches on the boot's own exit status, so a discovery regression reads as a legitimate refusal; MINORs: boot_bindings/open_pinned untested, stale php_backend_unavailable comments, the [route.static]+[route.php] shape lost from PHP_MANIFEST.

## 2026-11-15T00:28 [lead]

status review -> ready

## 2026-11-15T00:28 [lead]

status ready -> active

## 2026-11-15T00:28 [lead]

attempt 2/3 started

## 2026-11-15T00:36 [lead]

status active -> review

## 2026-11-15T00:40 [lead]

edited fields

## 2026-11-15T00:40 [lead]

lead: paths widened by crates/blastit-scheduler/src/route.rs + its tests — reviewer MINOR 2: the static_forward rule stage1 now patches belongs in RouteInfo::with_static's only caller, so there is one rule, not two.

## 2026-11-15T00:46 [lead]

status review -> done
