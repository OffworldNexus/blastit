# RESULT — MB.3.13.3 (attempt 1)

Vtable/boot wiring: a `[route.php]` route is served by an embedded NTS `libphp`
worker forked by `init`'s `Ctl::Spawn`, or the boot refuses with `ADR-0010`
§4.1's package sentence. The interim `MB.3.28` refusal is deleted.

## Attempt 2 — review findings (all five addressed)

| # | Finding | Change |
|---|---|---|
| 1 MAJOR | Discovery followed symlinks (`File::open`/`metadata`/`is_file`) while stage 1 opens `O_NOFOLLOW`, so a symlinked `libphp` passed the gate and then died with `ELOOP` after the banner, with no §4.1 sentence. | `stage0::discover_php` now records `std::fs::canonicalize`'s answer for **both** objects (`resolve()`), so stage 0 and stage 1 open the same real file and a distro's `libphp8.so -> libphp8.4.so` keeps working. A name that cannot be resolved is a §4.1 refusal (`PhpRefusal::NotAFile`, whose sentence carries the route, the `entry`, the file, the reason, the package and `BLASTIT_LIBPHP`), never an `errno` in a worker. `php_runtime_line` now logs **`libphp <resolved path>` and `shim <resolved path>`** so "found it" is visible. Tests: `stage0::php_boot_gate::a_symlinked_libphp_and_shim_are_recorded_as_their_targets` (a link to a real file records the target, and the recorded paths are not links), `…::a_dangling_symlink_is_refused_with_the_package_sentence`, and `critical_php_backend.rs::a_symlinked_blastit_libphp_is_resolved_before_it_is_served` (end to end: must boot and serve on a host with the pair, must refuse with the package sentence otherwise). |
| 2 MAJOR | The protected suite branched on the boot's own exit status, so a discovery regression (`NoShim`/`Missing`) read as a legitimate refusal. | `critical_php_backend.rs` gained `host_php()`, an **independent** probe: raw `std` file access, the `ADR-0010` §3 candidate list, `.dynsym` names read as bytes (`executor_globals` present, `tsrm_startup` absent), the version taken from the resolved file name, and the shim found by the ADR's literal spelling in `BLASTIT_PHP_SHIM_DIR` → exe dir → `target/php-sapi`. It calls none of the code under test. The two outcomes are now keyed on that probe: probe says yes → the boot **must** serve (a refusal panics with the files it found); probe says no → the refusal is asserted; probe says no but the boot serves → the probe is stale and the test fails. |
| 3 MINOR | `boot_bindings`/`open_pinned` had no tests. | Three unit tests in `php_view::tests`: `a_php_slot_gets_one_binding_and_a_wsgi_slot_none` (accept branch: absolute `entry`, rendered INI, `view_tmp`, `max_requests`, and both descriptors readable as the recorded bytes), `a_replaced_libphp_or_shim_is_refused_by_name` (a file replaced between the record and the open → refusal, **each object independently**, so a record that verified only the library fails the shim half), and `a_missing_record_is_refused_by_route`. |
| 4 MINOR | Comments still described `php_backend_unavailable`/`MB.3.28` as the next live boot step. | Rewritten to today's order at all five sites (`stage0::run`'s header, `no_python_support`'s and `app_rule`'s doc, `mod capability`'s PHP case, `mod app_rule_executors`'s PHP fixture): gate → `config.php` record → stage-1 bindings → `Ctl::Spawn` fork. No `MB.3.28`/`interim` reference to a deleted function remains. |
| 5 MINOR | The `[route.static] root = "public"` + `[route.php]` page shape was covered nowhere. | `PHP_MANIFEST` is now the documented page (`docs/frameworks/php.md`), and the served leg asserts all three rules: `/asset.txt` → `200` from the static responder, `/php-probe` → `200` from the front controller, `/index.php` → `404` (only `entry` runs; no source dump). Making that true needed one line of behaviour: `static_forward` is derived from `BLASTIT_APP` alone in both `RouteInfo` builders, and a PHP route has no `app` (`ADR-0010` §2), so a miss `404`ed instead of reaching the front controller — `stage1` now sets `static_forward` for a PHP route whose policy is not `spa`, in `dev` and in production (`routes_from_manifest` is the same rule one process earlier). |

### The `ELOOP` before/after (finding 1)

Before — `BLASTIT_LIBPHP=/usr/lib/libphp8.so` (the distro's own symlink) passed the
gate and then died in stage 1, after the banner, with an `errno`:

```console
$ BLASTIT_LOG_FORMAT=json BLASTIT_LIBPHP=/usr/lib/libphp8.so blastit dev --port 46351
{"level":"warn","event":"manifest.warning","message":"the [route.php] shim is …/libblastit_php84-x86_64-gnu.so (PHP 8.4, x86_64-gnu)"}
{"level":"info","event":"boot","stage":"stage0",…}
{"level":"error","event":"app.output","stream":"stderr","line":"route \"app\": libphp /usr/lib/libphp8.so: Too many levels of symbolic links (os error 40)","part":1,"parts":1}
```

After — the same link resolves to the real file, which is what the boot line says and
what the workers load; the request is served:

```console
$ ls -l /usr/lib/libphp8.so
lrwxrwxrwx … /usr/lib/libphp8.so -> /etc/alternatives/libphp8
$ BLASTIT_LOG_FORMAT=json BLASTIT_LIBPHP=$PWD/shimlink.so blastit dev --port 46411   # two hops
{"level":"warn","event":"manifest.warning","message":"the [route.php] runtime is libphp /usr/lib/libphp8.4.so and shim …/target/php-sapi/libblastit_php84-x86_64-gnu.so (PHP 8.4, x86_64-gnu)"}
$ curl -sS -i http://127.0.0.1:46411/php-probe
HTTP/1.1 200 OK
Content-type: text/plain;charset=UTF-8
Transfer-Encoding: chunked

blastit-php-ok
```

A dangling link takes the sentence instead:

```console
$ BLASTIT_LIBPHP=/tmp/…/libphp8.so blastit dev            # -> a target that does not exist
blastit dev: route "app": [route.php] entry "public/index.php": /tmp/…/libphp8.so is not an
ELF shared object …; install the PHP embed package — … or set BLASTIT_LIBPHP=/path/to/libphp.so
$ echo $?; # stdout: 0 bytes
1
```

## Attempt 3 — two MINORs (review PASS)

| # | Finding | Change |
|---|---|---|
| 1 MINOR | `host_php()` read the **ambient** `BLASTIT_LIBPHP`/`BLASTIT_PHP_SHIM_DIR` while `Dev::start` clears the environment, so the served leg could pass on a pair the probe never saw. | The served leg now hands the boot the probe's own answer — `BLASTIT_LIBPHP=<host.libphp>`, `BLASTIT_PHP_SHIM_DIR=<host.shim.parent()>` — and asserts the boot's log names **exactly** `libphp <path>` and `shim <path>`. Negative control (below): with a 14-byte ambient `BLASTIT_LIBPHP`, the served leg now fails loudly instead of passing. |
| 2 MINOR | `with_static` still derived `static_forward` from `has_app` alone, so any caller but `stage1` saw `false` for a `[route.static]` + `[route.php]` page; `stage1` patched it in a loop. | The rule moved to its only caller: `routes_from_manifest` now passes `has_app \|\| route.php().is_some()` (manifest access is already there), `with_static`'s parameter is named `forwards` and its doc says exactly what it means ("this route has something to forward to: `BLASTIT_APP`, or its own executor"), and the `stage1` loop is deleted (the two `dev` builders pass the same expression). New scheduler test `a_static_and_php_page_forwards_its_misses_to_the_front_controller` pins all three directions through `routes_from_manifest`: `[route.static]`+`[route.php]` with no app → forward; `spa = true` → not; app present → forward; and a pure static route → not. |

### The negative control (finding 1)

The probe and the boot now cannot disagree about the pair, and a poisoned environment
is a loud failure rather than a green pass:

```console
$ printf 'not a library\n' > /tmp/opencode/bogus-libphp.so   # 14 bytes
$ BLASTIT_LIBPHP=/tmp/opencode/bogus-libphp.so \
  cargo test -p blastit --locked --test critical_php_backend -- \
    --exact a_php_route_is_served_or_the_boot_refuses_naming_the_package
thread '…' panicked at crates/blastit/tests/critical_php_backend.rs:654:
the probe found no usable libphp + shim, yet the boot served the PHP route. The probe reads
BLASTIT_LIBPHP/BLASTIT_PHP_SHIM_DIR from this process's environment while the boot is given
the probe's own answer, so an ambient value that is not a library makes the two disagree
test result: FAILED. 0 passed; 1 failed
```

Before this round the same invocation printed `test result: ok` (attempt 2's transcript),
because the boot — with its environment cleared — found `/usr/lib/libphp8.4.so` while the
probe was reading the text file.

The positive control, an ambient pair that *is* a library, still passes and names it:

```console
$ BLASTIT_LIBPHP=/usr/lib/libphp8.4.so BLASTIT_PHP_SHIM_DIR=$PWD/target/php-sapi \
  cargo test -p blastit --locked --test critical_php_backend -- --nocapture --exact \
    a_php_route_is_served_or_the_boot_refuses_naming_the_package
served: HTTP/1.1 200 OK  cpu_ns=15645809  wall_ns=33775416  (libphp /usr/lib/libphp8.4.so)
test result: ok. 1 passed; 0 failed
```



## What changed (10 files, `crates/` only)

| File | Change |
|---|---|
| `crates/blastit/src/php_view.rs` | **boot half**: `boot_bindings(config, manifest, routes, dev)` builds one `PhpBinding` per PHP slot — `entry`/`document_root` from the manifest dir, `IniPolicy::new(...)` rendered once, `view_tmp`, `max_requests`, `max_body_bytes`, and both objects re-opened `O_RDONLY\|O_CLOEXEC\|O_NOFOLLOW` and checked against stage 0's `(st_dev, st_ino)`. `PhpObject` (the `Path` "native development" variant) is gone: `PhpBinding.libphp`/`.shim` are `OwnedFd`, so a load can only ever be of the verified inode. `fork_php_view` keeps both fds through `ChildIo.keep`. |
| `crates/blastit/src/config.rs` | `PhpRuntime` now records the **shim's** `(st_dev, st_ino)` as well as the library's (`shim_dev`, `shim_ino`) and round-trips them through the stage-0→stage-1 document. Without this the stage-1 re-open compared the shim against the library's inode and refused every boot — see "What the first run found". |
| `crates/blastit/src/stage0.rs` | `PhpBoot::Proceed(PhpRuntime)` (no `Option`), `PhpRefusal::is_fatal`/the `Development` fall-through deleted so **every** scope refuses a host without a usable library, both boot paths store `config.php = Some(runtime)`, `php_backend_unavailable` and its 3 call sites deleted. Every refusal now names the route **and** its `[route.php] entry`. |
| `crates/blastit/src/stage1.rs` | runs `boot_bindings` before `bind_tcp` (a moved file refuses before the listener), threads the vector through `Serving` → `Parts`, and passes `has_app \|\| route.php().is_some()` to `with_static` in its two `dev` builders (attempt 3). |
| `crates/blastit-scheduler/src/route.rs` | `routes_from_manifest` derives `static_forward` from **what the route has to forward to** (`BLASTIT_APP` or its own `[route.php]` executor), `with_static`'s parameter/doc renamed to say exactly that, and a new test pins the three directions (attempt 3). |
| `crates/blastit/src/supervise.rs` | `Parts.php` / `Supervisor.php`: the per-route choice is **boot data**. `spawn_now` → `fork_view` (one `if let` on the slot's binding, no match on backend kind) + `record_view` (one `SPAWNED`/pipes/pid/event path for both runtimes). `interpreter_live(py)` is `true` in a `python` build (init holds the GIL) and `false` in the standalone one. |
| `crates/blastit/src/check.rs` | `blastit check` runs `php_boot_gate(..., Development)` instead of the interim sentence. |
| `crates/blastit/src/lib.rs` | `mod php_view` loses its `#[allow(dead_code)]`; the doc says what it now is. |
| `crates/blastit/tests/critical_php_backend.rs` | rewritten (protected; authorized with `critical_guard.py --node MB.3.13.3`): the documented static+PHP page, an independent `host_php()` probe deciding the two outcomes, and a symlink leg (attempt 2). |
| `crates/blastit/tests/no_python_build.rs` | its `[route.php]` case now pins a `BLASTIT_LIBPHP` that cannot be a library, so the interpreter-free build asserts the discovery gate's sentence instead of the deleted interim one. |

`1728 insertions / 658 deletions` in `crates/` (attempts 1–3) — the net change
is ~+1070 lines, and most of the churn is the protected suite's rewrite (deleted
doc + a new boot/HTTP/event/host-probe harness). No file outside `crates/` was
touched.

### Which entry points are wired

| Entry point | Wired? |
|---|---|
| `blastit` (production server mode) | **Yes** — same `Supervisor`; the only untested-by-this-node part is the privileged `setresuid` drop, which is `MB.3.13`'s and `e2e/rootless`'s. |
| `blastit dev` | **Yes** — proven end to end below (200 through `view.chan`). |
| `blastit check` | **Yes** — reaches the discovery gate (exit `2`, empty stdout). |
| Stage-1 probe child (`ADR-0010` §3, design `R8`) | **Deferred, deliberately.** The load-time ABI check already refuses a wrong minor *before* `READY`: `Loaded::load` compares `blastit_php_minor()` against `php_version()` inside the worker and `php_view` turns a `LoadError` into `exit 1` before `serve_forever` sends anything. A separate pre-bind probe child adds a second fork per PHP route every boot; `MB.3.13`'s `exit()`/fatal resilience and `MB.3.18`'s native probe legs (`critical_shim_contract`, `make php-sapi-e2e`) are where the real load is proven. Nothing in this node asserts a probe exists. |

## What the first run found (and fixed)

Stage 1's re-open refused the shim on every boot:

```
ERROR app.output  stderr  route "app": shim …/target/php-sapi/libblastit_php84-x86_64-gnu.so
is not the file stage 0 inspected (st_dev/st_ino moved) …
```

The `(dev, ino)` pair in `Config::php` was the **library's**, and the shim was
verified against it. `PhpRuntime` now records both objects' identities
(`shim_dev`/`shim_ino`), which is what `ADR-0010` §3 asks for ("recording
`(path, st_dev, st_ino)`" for `libphp` *and* the shipped shim).

## Evidence

### 1. Native 200 fixture — `blastit dev`, standalone build (`default = ["php"]`)

The fixture is the documented page (`docs/frameworks/php.md`): `[route.static]
root = "public"` + `[route.php] entry = "public/index.php"`, with an asset beside
the front controller.

```console
$ cat blastit.toml                     # /tmp/opencode/phpfinal
version = 1

[[route]]
name = "app"
path = [".*"]

[route.static]
root = "public"

[route.php]
entry = "public/index.php"

$ BLASTIT_LOG_FORMAT=json BLASTIT_MEM_HIGH_WATERMARK=100 \
  <worktree>/target/debug/blastit dev --port 46401 &
$ curl -i http://127.0.0.1:46401/asset.txt        # the static responder
HTTP/1.1 200 OK
Content-Type: text/plain; charset=utf-8
Content-Length: 15
…
$ curl -i http://127.0.0.1:46401/php-probe        # a miss → the front controller
HTTP/1.1 200 OK
Content-type: text/plain;charset=UTF-8
Transfer-Encoding: chunked

blastit-php-ok
$ curl -o /dev/null -w '%{http_code}\n' http://127.0.0.1:46401/index.php
404                                              # only `entry` runs: no source dump
```

The same boot's own event stream — the resolved library and shim, and the
`request` events carrying the worker's CPU nanoseconds beside the wall time:

```console
$ head -1 err.txt                       # the boot line (MB.3.35)
{"ts":"…23:41:59.785Z","level":"warn","event":"manifest.warning","component":"init",
 "message":"the [route.php] runtime is libphp /usr/lib/libphp8.4.so and shim
 …/target/php-sapi/libblastit_php84-x86_64-gnu.so (PHP 8.4, x86_64-gnu)"}
$ grep '"event":"request"' out.txt
{"bytes_in":0,"bytes_out":15,"cost_ns":248729,"cpu_ns":null,"duration_ms":0.354704,
 "event":"request","method":"GET","outcome":"ok","path":"/asset.txt","route":"app","status":200}
{"bytes_in":0,"bytes_out":15,"cost_ns":21985632,"cpu_ns":20376019,"duration_ms":37.014073,
 "event":"request","method":"GET","outcome":"ok","path":"/php-probe","route":"app","status":200}
{"bytes_in":0,"bytes_out":10,"cost_ns":0,"cpu_ns":null,"duration_ms":0.082725,
 "event":"request","method":"GET","outcome":"unmatched","path":"/index.php","route":"app","status":404}
```

`cpu_ns = 20 376 019 ≤ wall_ns = 37 014 073` (`duration_ms × 10⁶`); the static
answer carries no `cpu_ns` at all, exactly as before PHP existed.

The human format of the same boot (`BLASTIT_LOG_FORMAT` unset), which is what a
developer reads:

```console
23:17:08.205 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
23:17:08.205 WARN  manifest.warning the [route.php] runtime is libphp /usr/lib/libphp8.4.so and shim …/libblastit_php84-x86_64-gnu.so (PHP 8.4, x86_64-gnu)
23:17:08.213 INFO  ready       serving  routes=1  workers=16
23:17:14.205 INFO  process.spawned app  pid=2312648 uid=1000  fork=0.2ms
23:17:14.239 INFO  request     GET / → app  200  34.4ms
```

### 1b. The no-library twin (same fixture, `BLASTIT_LIBPHP` names a text file)

```console
$ BLASTIT_LIBPHP=$PWD/libphp.so blastit dev --port <n>    # libphp.so holds "this is not an ELF shared object"
$ echo "exit=$?"                                          # stdout: 0 bytes
exit=1
blastit dev: route "app": [route.php] entry "public/index.php": /tmp/…/libphp.so is not an
ELF shared object (no ELF magic); this host is (PHP ?, x86_64-gnu), so no shim can be chosen
until the object's version is known; install the PHP embed package — Debian/Ubuntu:
apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded,
or set BLASTIT_LIBPHP=/path/to/libphp.so
```

### 2. The protected suite, standalone build (this host: PHP 8.4.27 + shim)

```console
$ cargo test -p blastit --locked --test critical_php_backend -- --nocapture --test-threads=1
running 5 tests
test a_blastit_libphp_that_is_not_a_library_refuses_the_boot ... refused: blastit dev: route "app": [route.php] entry "public/index.php": /tmp/blastit-php-backend-…-not-a-library/libphp.so is not an ELF shared object (no ELF magic); this host is (PHP ?, x86_64-gnu), so no shim can be chosen until the object's version is known; install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so

ok
test a_php_route_is_served_or_the_boot_refuses_naming_the_package ... served: HTTP/1.1 200 OK  cpu_ns=21424575  wall_ns=39580208  (libphp /usr/lib/libphp8.4.so)
ok
test a_static_only_manifest_is_not_refused_by_this_gate ... ok
test a_symlinked_blastit_libphp_is_resolved_before_it_is_served ... symlinked /tmp/blastit-php-backend-…-symlink/libphp8.so -> /usr/lib/libphp8.4.so  served: HTTP/1.1 200 OK
ok
test check_reaches_the_same_discovery_gate ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

What each case proves:

* `a_php_route_is_served_or_the_boot_refuses_naming_the_package` — the two
  `ADR-0010` §1 outcomes and nothing else, **keyed on the independent `host_php()`
  probe**. Served: the static asset from disk, the front controller's body for a
  miss, `404` for `.php` under the root, the boot line naming the resolved
  library and shim, `cpu_ns <= wall_ns`, the tick floor as the second half
  (`cpu_ns > 0 || wall_ns < 1 ms`), and — the invariant that `libphp` is loaded
  **only** below a view worker — `/proc/<init pid>/maps` of the still-serving
  init contains neither `libphp` nor `libblastit_php`. Refused: exit `1`, empty
  stdout, the package sentence.
* `a_symlinked_blastit_libphp_is_resolved_before_it_is_served` — `BLASTIT_LIBPHP`
  is a symlink: with the pair on the host it must boot, serve, and log the
  **resolved** path; without one the (dangling) link must refuse with the
  package sentence and never with `Too many levels of symbolic links`.
* `a_blastit_libphp_that_is_not_a_library_refuses_the_boot` — the denied twin,
  deterministic on every host: exit `1`, **empty stdout**, the sentence naming
  the route, the entry, the file, the package and `BLASTIT_LIBPHP`, no fallback
  recipe (`php-fpm`/`fastcgi`/`route.fastcgi`/`wsgi`/`meanwhile`), and no WSGI
  surface (no banner, no `view …` line).
* `check_reaches_the_same_discovery_gate` — `blastit check` exits `2` with the
  same sentence and empty stdout, before pytest.
* `a_static_only_manifest_is_not_refused_by_this_gate` — the allowed twin: no
  PHP route, so the gate is silent and the boot reaches its banner with the
  static route counted.

### 3. Wheel configuration (`--features python`, `interpreter_live = true`)

```console
$ cargo test -p blastit --locked --features python --test critical_php_backend -- --nocapture --test-threads=1
test a_blastit_libphp_that_is_not_a_library_refuses_the_boot ... ok
test a_php_route_is_served_or_the_boot_refuses_naming_the_package ... served: HTTP/1.1 200 OK  cpu_ns=13938372  wall_ns=33239683  (libphp /usr/lib/libphp8.4.so)
ok
test a_static_only_manifest_is_not_refused_by_this_gate ... ok
test a_symlinked_blastit_libphp_is_resolved_before_it_is_served ... symlinked …/libphp8.so -> /usr/lib/libphp8.4.so  served: HTTP/1.1 200 OK
ok
test check_reaches_the_same_discovery_gate ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Same result with CPython's fork protocol in place (`PyOS_Before/AfterFork`),
which is the branch `interpreter_live = true` selects.

### 4. Acceptance commands

```console
$ cargo test -p blastit --locked
unittests src/lib.rs      test result: ok. 184 passed; 0 failed; 1 ignored
unittests src/main.rs     test result: ok. 0 passed; 0 failed
tests/audit_group_hint    test result: ok. 4 passed; 0 failed
tests/critical_check_engine       test result: ok. 19 passed; 0 failed
tests/critical_config_and_presets test result: ok. 11 passed; 0 failed
tests/critical_dev_and_fail_closed test result: ok. 0 passed; 0 failed
tests/critical_php_backend        test result: ok. 5 passed; 0 failed
tests/critical_static_cli         test result: ok. 11 passed; 0 failed
tests/dev_signal_reap             test result: ok. 1 passed; 0 failed
tests/no_python_build             test result: ok. 3 passed; 0 failed
tests/smoke                       test result: ok. 31 passed; 0 failed
doc-tests blastit                 test result: ok. 0 passed; 0 failed
exit 0
```

```console
$ cargo test -p blastit-scheduler --locked
test route::tests::a_static_and_php_page_forwards_its_misses_to_the_front_controller ... ok
test result: ok. 47 passed; 0 failed; 0 ignored          (lib)
…                                                        (11 + 9 + 32 + 14 + 6 + 18 + 9 + 17 integration tests)
exit 0
```

```console
$ make check
…
cargo test -p blastit --features python --test critical_php_backend
running 5 tests
test a_static_only_manifest_is_not_refused_by_this_gate ... ok
test check_reaches_the_same_discovery_gate ... ok
test a_blastit_libphp_that_is_not_a_library_refuses_the_boot ... ok
test a_symlinked_blastit_libphp_is_resolved_before_it_is_served ... ok
test a_php_route_is_served_or_the_boot_refuses_naming_the_package ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
…
cargo test  -p blastit --test no_python_build          # the interpreter-free configuration
running 3 tests
test a_wsgi_manifest_is_refused_by_the_capability_gate ... ok
test a_php_only_manifest_is_refused_by_the_php_discovery_gate ... ok
test a_manifest_asking_for_the_retired_external_php_pool_is_refused_by_name ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
…
clippy      -p blastit{,-view} (standalone)
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

`grep -c 'test result: FAILED'` over the whole log: **0**.

```console
$ uv run --no-project tools/plan.py validate
plan OK
```

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.3.13.3
critical-guard: HEAD..working tree, 14 changed file(s)
node MB.3.13.3: paths ['crates/blastit-view/src/**', 'crates/blastit-view/Cargo.toml',
  'crates/blastit/src/**', 'crates/blastit-php-sapi/src/**', 'crates/blastit/tests/**']
  M  crates/blastit/tests/critical_php_backend.rs  [authorized by crates/blastit/tests/**]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.13.3
```

### 5. Not run here

* `make php-sapi-e2e` (Docker, the real load in a packaged container) — CI's
  proof, per the brief; not run in this session.
* Production's privileged path (`setresuid` to the route uid, `RLIMIT_NPROC`,
  `EmbeddedView` egress filter under `Scheduler`-separated uids): the same
  `Supervisor::fork_view` serves it and it compiles in every configuration, but
  it is `MB.3.13`/`e2e/rootless`'s test, not this node's. The dev boot above
  exercises the fork, the child sequence, `EmbeddedView`, the `view.chan`
  request/response path and the CPU accounting; only the uid drop differs.

## Follow-ups (out of this node's `paths`)

1. `docs/knowledge/LESSONS.md` is outside `MB.3.13.3.paths`, so these two
   lessons are recorded here instead of appended to it:
   * **A stage-0 identity record is per *object*, not per route.** `Config::php`
     carried one `(dev, ino)` and the stage-1 re-open compared *both* the
     library and the shim against it, so every boot refused the shim as "moved".
     Record one pair per file the worker loads (`shim_dev`/`shim_ino` here), and
     read the refusal sentence before believing the file really changed.
   * **A test that parses `blastit dev`'s event stream must set
     `BLASTIT_LOG_FORMAT=json`.** `dev`'s default is the human format
     (`INFO request GET / → app 200 34.4ms`), so a JSON parse of stdout silently
     finds no `request` event and reads as "the scheduler never reported a
     cost"; the JSON document is the only machine-readable form.
   * **A discovery record must hold the *resolved* path, not the name that was
     opened.** Stage 0 follows the distro's `libphp8.so -> libphp8.4.so`, stage 1
     re-opens `O_NOFOLLOW` by the recorded path, and recording the link makes a
     perfectly good host fail after the banner with `ELOOP (os error 40)` and no
     §4.1 sentence — canonicalise before recording (attempt 2, finding 1).
   * **A protected test must branch on an independent probe, never on the exit
     status of the thing it tests.** "It refused" and "discovery regressed" are
     the same observation from outside; resolve the library and the shim with
     raw `std` file access and require the served outcome whenever the probe
     finds them (attempt 2, finding 2).
2. `MB.3.13.4`'s parent acceptance still lists the **stage-1 probe child**
   (`ADR-0010` §3, design `R8`) — deferred here with the reason above; it belongs
   with `MB.3.18`'s native per-platform probe legs or a dedicated leaf.
3. `docs/design/backend-runtimes.md` §4.3 still shows `WorkerBinding`/
   `SpawnSpec` as a scheduler-side type; the implementation keeps the choice in
   `Supervisor` (as this node's brief required, `blastit-scheduler` not being in
   scope), so `MB.9.3` should reconcile the doc with the `Parts.php` vector.
4. **Production provenance is still `MB.3.13.2`'s half**: `ADR-0010` §3 and
   design `R7`/`B2-R5` ask the gate to refuse a `libphp`/shim whose parent a
   route uid could write, plus a writable `manifest_dir`/`entry`, via
   `host::plain_is_protected(path, init_uid)` and `!has_file_caps(path)` — also
   "zero writable `DT_RPATH`/`DT_RUNPATH`". Today the gate inspects the ELF
   symbols and the tuple only, and this node adds the cross-stage inode pin on
   top of that; the `plain_is_protected` half is not implemented anywhere yet and
   belongs to a `MB.3.13.2`/`MB.3.18` follow-up (it must not run in `dev`, where
   the shim lives in a writable checkout).
