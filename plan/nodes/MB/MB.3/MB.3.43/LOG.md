# Log MB.3.43

## 2026-11-15T02:52 [lead]

created (task) Close MB.3.40 review MINORs: the gate's call site needs a protected witness, and two new unit tests assert nothing

## 2026-11-15T02:55 [lead]

edited fields

## 2026-11-15T02:55 [lead]

reviewer of MB.3.42 left two evidence-hygiene MINORs in its RESULT.md (clippy was recorded red from MB.3.40's in-flight host.rs, now green; make check was skipped by the owner to avoid a wheel-cache race, reviewer ran it green) — fold the corrected evidence in here

## 2026-11-15T03:05 [lead]

ROOT CAUSE of the red php-sapi job (run 43665450916, head c56b1e1): the four MB.3.40 legs panic at critical_php_embed.rs:2070 'no directory on this host is one uid's alone (tried ): a deployment boot could not run on this host either, so the accepted twin has nothing to accept' — note '(tried )' is EMPTY. cargo test -p blastit --test critical_php_backend --test critical_php_embed --no-run   # build here, run in the image
docker run blastit-php-sapi-e2e:local target/debug/build/blastit/3b9e55e09c71c369/out/critical_php_backend-3b9e55e09c71c369 --nocapture

running 5 tests
test a_static_only_manifest_is_not_refused_by_this_gate ... refused: blastit dev: route "app": [route.php] entry "public/index.php": /tmp/blastit-php-backend-1-not-a-library/libphp.so is not an ELF shared object (no ELF magic); this host is (PHP ?, x86_64-gnu), so no shim can be chosen until the object's version is known; install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so

ok
test check_reaches_the_same_discovery_gate ... ok
test a_blastit_libphp_that_is_not_a_library_refuses_the_boot ... ok
served: HTTP/1.1 200 OK  cpu_ns=14932684  wall_ns=33283583  (libphp /usr/lib/libphp8.4.so)
symlinked /tmp/blastit-php-backend-1-symlink/libphp8.so -> /usr/lib/libphp8.4.so  served: HTTP/1.1 200 OK
test a_php_route_is_served_or_the_boot_refuses_naming_the_package ... ok
test a_symlinked_blastit_libphp_is_resolved_before_it_is_served ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s

docker run blastit-php-sapi-e2e:local target/debug/build/blastit/0b784173b4fc9736/out/critical_php_embed-0b784173b4fc9736 --nocapture

running 18 tests
test a_runpath_into_a_writable_directory_refuses_a_production_boot ... FAILED
refused (stage 0; the zero-dlopen proof is the ZTS leg): blastit dev: route "app": [route.php] entry "public/index.php": /tmp/blastit-php-embed-1-not-a-library/libphp8.4.so is not an ELF shared object (no ELF magic); this host is (PHP 8.4, x86_64-gnu), whose embedded shim is libblastit_php84-x86_64-gnu.so; install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
test a_non_library_blastit_libphp_refuses_before_anything_loads ... ok
test a_world_writable_manifest_root_refuses_a_production_boot ... FAILED
`blastit dev` served a 0777 tree the deployment rule refuses: HTTP/1.1 502 Bad Gateway
test a_dev_boot_of_a_world_writable_tree_still_serves ... ok
test a_world_writable_shim_directory_refuses_a_production_boot ... FAILED
blocked 200 ms in usleep: status 200  cpu_ns=141252  wall_ns=216738837  cpu/wall=1/1534  answered in 217.591678ms
test a_blocking_script_is_charged_cpu_time_not_wall_time ... ok
fatal: status 500  outcome=view_died  cpu_ns=571936  wall_ns=18764049  cost_ns=18764049  retired pid 153  respawned pid 154 (ppid 126) uid 1000  live=[154]
test a_fatal_is_contained_and_the_next_request_is_served ... ok
form body reached `$_POST` (x=1, 200); a 8194-byte body was refused with `HTTP/1.1 413 Content Too Large`; the slot still answered x=2
test a_form_body_reaches_the_front_controller ... ok
test the_same_tree_owned_by_init_is_accepted ... FAILED
usleep(): status 500  outcome=view_died  cpu_ns=117009  wall_ns=76331331  cost_ns=76331331  retired pid 212  respawned pid 213 (ppid 185) uid 1000  live=[213]
test a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time ... ok
refused (stage 0, zero dlopen): blastit dev: route "app": [route.php] entry "public/index.php": /tmp/blastit-php-embed-1-zts/libphp8.4.so is a ZTS (thread-safe) libphp, and the embedded worker needs the NTS build of the same version; this host is (PHP 8.4, x86_64-gnu), whose embedded shim is libblastit_php84-x86_64-gnu.so; install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
test a_zts_libphp_refuses_before_anything_loads ... ok
refused (stage 1 inode pin) on attempt 1: {"ts":"2026-11-15T02:05:08.479Z","level":"info","event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"unprivileged-dev","preset":null}
{"ts":"2026-11-15T02:05:08.484Z","level":"error","event":"app.output","component":"init","stream":"stderr","line":"route \"app\": libphp /tmp/blastit-php-embed-1-swap/libphp8.4.so is not the file stage 0 inspected (st_dev/st_ino moved), so the worker would load something nobody verified; restore the package and restart blastit","part":1,"parts":1}
{"ts":"2026-11-15T02:05:08.478Z","level":"warn","event":"manifest.warning","component":"init","message":"the [route.php] runtime is libphp /tmp/blastit-php-embed-1-swap/libphp8.4.so and shim /tmp/blastit-php-embed-1-swap/libblastit_php84-x86_64-gnu.so (PHP 8.4, x86_64-gnu)"}
test a_libphp_replaced_between_the_stages_refuses_the_boot ... ok
killed at BLASTIT_PROBE_TIMEOUT_MS=250 after 759.535814ms; probe pid 224 ppid 221 pgid 224 uid 1000 seccomp EPERM, gone from /proc
route "app": [route.php] entry "public/index.php": the probe child (pid 224) did not load `libphp` and its shim within 250 ms, so it was killed; raise BLASTIT_PROBE_TIMEOUT_MS if this host needs longer; this host is (PHP 8.4, x86_64-gnu), whose embedded shim is libblastit_php84-x86_64-gnu.so; install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
test a_probe_that_hangs_is_killed_and_refuses_the_boot ... ok
refused at boot (stage-1 probe), load in a child only: [(228, 225), (228, 225)]
route "app": [route.php] entry "public/index.php": the probe child (pid 228) exit code 1: the shim is built for PHP 8.3 but the host's libphp reports 8.4.27; install the libblastit_php83 shim for this host; this host is (PHP 8.4, x86_64-gnu), whose embedded shim is libblastit_php84-x86_64-gnu.so; install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
test a_shim_for_another_minor_refuses_the_boot_before_the_listener ... ok
refused (stage 1 shim inode pin) on attempt 1: {"ts":"2026-11-15T02:05:08.779Z","level":"info","event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"unprivileged-dev","preset":null}
{"ts":"2026-11-15T02:05:08.785Z","level":"error","event":"app.output","component":"init","stream":"stderr","line":"route \"app\": shim /tmp/blastit-php-embed-1-swap-shim/libblastit_php84-x86_64-gnu.so is not the file stage 0 inspected (st_dev/st_ino moved), so the worker would load something nobody verified; restore the package and restart blastit","part":1,"parts":1}
{"ts":"2026-11-15T02:05:08.778Z","level":"warn","event":"manifest.warning","component":"init","message":"the [route.php] runtime is libphp /tmp/blastit-php-embed-1-swap-shim/libphp8.4.so and shim /tmp/blastit-php-embed-1-swap-shim/libblastit_php84-x86_64-gnu.so (PHP 8.4, x86_64-gnu)"}
test a_shim_replaced_between_the_stages_refuses_the_boot ... ok
exit(): status 500  outcome=view_died  cpu_ns=692389  wall_ns=19328900  cost_ns=19328900  retired pid 258  respawned pid 259 (ppid 232) uid 1000  live=[259]
test an_exit_is_contained_and_the_next_request_is_served ... ok
init pid 261: no libphp/shim; above the view 0 map(s) content-checked, 2 denied (EACCES), 0 gone, of 3 children; euid 1000
test no_libphp_is_mapped_above_the_view_worker ... ok
killed at BLASTIT_PROBE_TIMEOUT_MS=250 after 478.769914ms; probe pid 292 ppid 289 pgid 292 uid 1000 egress socket allowed (inet 0), gone from /proc
route "app": [route.php] entry "public/index.php": the probe child (pid 292) did not load `libphp` and its shim within 250 ms, so it was killed; raise BLASTIT_PROBE_TIMEOUT_MS if this host needs longer; this host is (PHP 8.4, x86_64-gnu), whose embedded shim is libblastit_php84-x86_64-gnu.so; install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
test an_egress_route_lets_the_probe_open_a_socket ... ok

failures:

failures:
    a_runpath_into_a_writable_directory_refuses_a_production_boot
    a_world_writable_manifest_root_refuses_a_production_boot
    a_world_writable_shim_directory_refuses_a_production_boot
    the_same_tree_owned_by_init_is_accepted

test result: FAILED. 14 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.50s runs the built binaries directly with 'docker run --user $(id -u):$(id -g) -v $CURDIR:$CURDIR -w $CURDIR' (Makefile:571-577), so inside the image the tests run as the runner uid (1001): CARGO_TARGET_TMPDIR is unset (no cargo), HOME is unset or /root (not writable by 1001), and /opt /usr/local /srv are root-owned 0755 so create_dir_all fails — and protected_root's loop s WITHOUT recording those, which is why the panic lists nothing. host::protected refuses any tree with a group/other-writable ancestor, so /tmp cannot host the accepted twin by design. This is exactly MINOR (2) and needs an environment-robust candidate list, not a skip: add the workspace's own target/tmp (runtime current_dir()/target/tmp AND compile-time CARGO_MANIFEST_DIR/../../target/tmp — the image bind-mounts the checkout at the same absolute path, so it is owned by the test's uid with no world-writable ancestor), keep CARGO_TARGET_TMPDIR/HOME/system dirs, and record a reason for EVERY rejected candidate so the panic is never '(tried )'. the_same_tree_owned_by_init_is_accepted must run everywhere — no host-tier demotion, no #[ignore], no filtering of the job.

## 2026-11-15T03:05 [lead]

CORRECTION to the mangled entry above (a backtick in a double-quoted plan.py argument was executed by the shell; ignore the injected text). The diagnosis itself stands, and `make rs-test-php-image` reproduces it locally, so the fix is verifiable before any push.

PANIC (all four, identical): "no directory on this host is one uid's alone (tried ): a deployment boot could not run on this host either, so the accepted twin has nothing to accept" at critical_php_embed.rs protected_root's panic!. The "(tried )" is EMPTY: the loop continues past a candidate that is not a directory or that create_dir_all refuses WITHOUT recording it, so the message names nothing.

ROOT CAUSE, by mechanism: Makefile's rs-test-php-image runs the built binaries directly, "docker run --user <host uid>:<host gid> -v $CURDIR:$CURDIR -w $CURDIR" (Makefile:575-577). Inside that container the tests are not under cargo, so CARGO_TARGET_TMPDIR is unset; HOME is unset or the image's /root, which that uid cannot create in; /opt, /usr/local and /srv are root-owned 0755, so create_dir_all is EACCES. Every candidate dies, and host::protected deliberately refuses any tree with a group- or other-writable ancestor, so /tmp (1777) can never host the accepted twin — by design, not as a workaround.

FIX (environment-robust, no skip, no job filter): add the workspace's own target/tmp as candidates - runtime std::env::current_dir()/target/tmp and compile-time env!("CARGO_MANIFEST_DIR")/../../target/tmp. The image bind-mounts the checkout at the same absolute path the binary was compiled against, so that tree is owned by the test's uid with no world-writable ancestor; targets under /tmp stay excluded because the gate is right about /tmp. Record a reason for EVERY rejected candidate (not a directory / create failed with its errno / host::protected's own sentence) so the panic can never print "(tried )" again.

ACCEPTANCE ADDENDUM: `make rs-test-php-image` must report 18 passed, 0 failed locally (that is the CI witness and it reproduces), and the_same_tree_owned_by_init_is_accepted must keep running everywhere - no host-tier demotion, no #[ignore], no filtering.

## 2026-11-15T03:11 [lead]

MB.3.37 reviewer PASSed (attempt 1) with four MINORs, three of which are this file's honesty and belong in the same pass as this node's own items (they are all in crates/blastit/tests/critical_php_embed.rs, which this node already owns):

(1) :1888-1891 — the leg-1 doc comment claims that comparing the shim against the library's pair "leaves every protected assertion green". The reviewer measured that inversion and leg 1 STAYS GREEN, but the suite goes red through 10 other legs, so the sentence overstates. Reword it to what is true: caught by the legs that require a SERVED boot (hang, egress twin, the real-pair legs), not by leg 1.

(2) :208 vs :227 — BLOCK_MS = 200 documents the blocking fixture's block, but BLOCKING_PHP hardcodes usleep(200000): changing the constant alone reddens the wall_ns >= block_ns guard instead of moving it. Build the fixture from BLOCK_MS (format!) or assert the two agree, so the constant cannot lie.

(3) :1719-1755 — the two probe twin legs (hang, egress=true) share the shape parse but duplicate every containment assertion (pid/ppid/pgid/uid/euid/reap), so they can drift. Extract one assert_probe_containment(&fields, init, route_uid, &shape) helper and call it from both; the assertions must stay identical in number and strength.

None of these may weaken or delete an assertion, and (2)/(3) must keep both legs red under their existing mutation controls.

## 2026-11-15T03:13 [lead]

status ready -> active

## 2026-11-15T03:13 [lead]

attempt 1/3 started

## 2026-11-15T03:13 [lead]

base 4749882

## 2026-11-15T03:26 [lead]

status active -> review

## 2026-11-15T03:29 [lead]

edited fields

## 2026-11-15T03:29 [lead]

status review -> done
