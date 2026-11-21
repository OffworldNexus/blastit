# MB.3.12 — RESULT

**Status:** done (attempt 3) — 26 files (24 new, `Makefile` + `Cargo.lock` edited), ~4.7k lines
(C shim, ABI, security types, probe, seccomp harness, Docker + driver, CI job, Makefile targets).

`crates/blastit-php-sapi` exists: one frozen `blastit_php_abi_v1` C surface
(`c/include/blastit_php_abi_v1.h`, mirrored by `src/abi.rs`), one per-minor C SAPI shim
(`c/blastit_php_sapi.c`, 17,384 bytes, `DT_NEEDED: libc.so.6` only), the Rust types that make
`PHP_VALUE`/`PHP_ADMIN_VALUE`/`REDIRECT_STATUS`/`HTTP_PROXY` unconstructible and a `Proxy:`
header unable to become `HTTP_PROXY`, and the toolchain discovery that refuses a PHP with no
shim **by naming `php-config` and the distro package**.

This host has no PHP toolchain, so the shim is built and **proven** in the container:
`debian:trixie-slim` + `php8.4-dev` + `libphp8.4-embed` + `php8.4-curl` (PHP 8.4.27, Debian's
NTS `/usr/lib/libphp8.4.so`) loads it for real and runs six requests through it. **A green
`make check` here proves the Rust degrades correctly; it never proves the C works.** That is
what `make php-sapi-e2e` is for, and it is deliberately outside `make check` (Docker + a PHP
image), exactly like `e2e/`.

## What attempt 2 changed (review)

**MAJOR 1 — `disable_functions` drift.** `DISABLED_FUNCTIONS` is now ADR-0010 §2 **verbatim**
(16 names, including `pcntl_signal`, `posix_kill`, `posix_getpgid`, which matter because a
denied `posix_setsid` is worth nothing while `posix_kill(-pgid)` still works).
`critical_shim_contract.rs` asserts the rendered directive *equals* the constant and contains
every name by name; the probe asserts the **effective** `ini_get('disable_functions')`.

**MAJOR 2 — `allow_url_fopen` was never pinned, and `IniPolicy` could not express egress.**
`IniPolicy::new` now takes a required `Egress` (`Denied`/`Allowed`, no `Default`): `Denied`
renders `allow_url_fopen=0`, `Allowed` renders `1`, so the effective value never depends on the
host's `php.ini` either way, and a caller that forgets to think about egress gets a compile
error instead of an egress-enabled worker. Tested both ways in the crate and through
`ini_get` in the probe.

**This found a real, silent defect.** `sapi_startup()` **nuls `ini_entries`** on the struct it
is given (`main/SAPI.c:70`), so assigning the lockdown before it — the obvious order — left
*every* directive NULL: no `open_basedir`, `allow_url_fopen=On` (distro default),
`disable_functions` empty. The container caught it the moment `ini_get` was asserted
(`"allow_url_fopen":"1"`, `"disable_functions":""`). The assignment now happens after
`sapi_startup` and the critical test pins the ordering (`startup < lockdown`), so it cannot
silently regress. `expose_php=0` had "passed" in attempt 1 only because Debian's `php.ini`
already sets it.

**The egress evidence is two-layer, and the second layer is the real filter.**
`e2e/php-sapi/seccomp-exec/` is a standalone e2e package (`[workspace]`, path dependency on
`crates/blastit-sys`) that installs `SeccompRole::View { egress: false }` — the same
`install()` a view worker calls — and execs the probe, which inherits it. `blastit-php-sapi`
keeps zero production dependencies. The driver runs the probe three times and asserts:

| run | `socket(AF_INET)` errno | `allow_url_fopen` | fopen(`http://`) | `stream_socket_client` | `curl` |
|---|---|---|---|---|---|
| no filter, via the probe | 0 (succeeds) | 0 | `php_setting` | `network` (errno 111) | `network` (os_errno 111) |
| real filter, via the harness | 1 (`EPERM`) | 0 | `php_setting` | refused | refused |
| real filter, `--egress allowed` | 1 (`EPERM`) | 1 | refused | refused | refused |

Which half proves what: `ini_get` proves the INI layer is pinned; the unfiltered run proves
the attempts *would* reach the network (so the filtered runs' difference is the filter, not
port 9 being closed); the `socket(AF_INET)` errno measured **in the same process** proves the
kernel refused it, independently of PHP's wording — which matters, because `stream_socket_client`
reports `(Unknown error)` and curl reports the same text as the unfiltered run when `socket()`
itself fails. The third row is the "strong layer with the weak one removed" case.

**MINORs.** (a) `php_module_startup`/`php_module_shutdown` are wrapped in `zend_first_try`/
`zend_try` and refuse with `-2` on a caught longjmp. (b) `clock_gettime` return values are
checked: `cpu_ns` is `0` ("no report") when either read fails, never indeterminate. (c) The
"another minor is refused" predicate test is gone; the real comparison is `MB.3.13`'s boot gate
and the test file says so, while the probe keeps the real half (the loaded shim reports a minor
this build supports). (d) The stale `paths` follow-up is dropped; the node frontmatter was not
touched by this attempt.

### Attempt 3 (four review MINORs, all "fails silently or confusingly")

1. **Only three directives were asserted effectively.** `open_basedir`, `ffi.enable` and
   `phar.require_hash` now come back from `ini_get` in `egress.php` and are asserted per run
   (`open_basedir` against `/fixtures:<the worker's own view_tmp>`, which the probe reports, so
   the expectation is not a duplicated constant). A repeat of attempt 1's dead lockdown for
   those three would now be red.
2. **A NULL `cfg->ini` was silently "use the host's php.ini".** `blastit_php_worker_init` now
   returns `-1` **before** `sapi_startup` when `cfg->ini == NULL`; the probe calls
   `worker_init` with a NULL ini and requires `-1` before the real init, and the critical
   contract pins the check's position in the source (before `sapi_startup`).
3. **`absolute_value` accepted `:`**, so a path with one rendered two `open_basedir` entries
   (one often empty, i.e. "anywhere") and every request would 500 on a warning
   `display_errors=0` hides. It now refuses with `IniError::Separator { key }`, tested for both
   the manifest and the view directory.
4. **The e2e harness had no ignore and no lock policy.** `e2e/php-sapi/.gitignore` ignores
   `target/` and **the harness `Cargo.lock`** (ignored, not committed: a non-published e2e tool
   whose only dependency is `crates/blastit-sys` would churn with every blastit-sys dependency
   move and pin nothing that ships; the image and CI build it unlocked on purpose).

## What changed

| File | Change |
|---|---|
| `crates/blastit-php-sapi/Cargo.toml` | New workspace member: **zero dependencies**; one `probe` feature gates the probe binary (`required-features`), so `make check` never builds it. |
| `build.rs` | `PHP_CONFIG` → `php-config` → `php-config8.4/8.3/8.2`; found ⇒ `cfg(php_toolchain)` + `BLASTIT_PHP_CONFIG`/`_VERSION`; absent ⇒ one `cargo:warning` naming `php-config` and `php8.4-dev`/`libphp8.4-embed` and **no** ABI surface. Never fails a build. |
| `src/lib.rs` | `#![deny(unsafe_code)]` + `deny(missing_docs)`; crate docs carry the "green check ⇏ working C" statement; `toolchain_present()`, `build_toolchain_version()`, `ABI_VERSION`. |
| `src/toolchain.rs` | `Minor` (8.2/8.3/8.4, `code()`, `shim_file()`, `accepts()`), injected-`Env` discovery, `ToolchainError` with actionable messages. |
| `src/request.rs` | `ServerVar`/`ServerVars` (reserved names, `HTTP_` derivation, httpoxy drop, 128/8192 budgets), `Egress`, `IniPolicy` (18 pinned + 6 view-scoped directives incl. `allow_url_fopen`, injection-refusing), ADR-verbatim `DISABLED_FUNCTIONS`. |
| `src/abi.rs` | `#[repr(C)]` mirror of the header, `const` layout assertions; the crate's only `unsafe_code` allowance (types only, **no `unsafe` block in the library**). |
| `c/blastit_php_abi_v1.h` | The frozen ABI: sink (`body`/`head`/`log`), request, config, result; 5 entry points; `_Static_assert`-checked layout. |
| `c/blastit_php_sapi.c` | Custom `sapi_module_struct` (`blastit`): `ub_write`→body frames, `send_headers`→head frames + end-of-head marker, `read_post`, `read_cookies`, `register_server_variables` (re-filters the four names), bounded `log_message`, `php_ini_ignore_cwd=1`, `SAPI_OPTION_NO_CHDIR`, the lockdown assigned **after** `sapi_startup`, `CLOCK_PROCESS_CPUTIME_ID` with checked returns, `zend_first_try`/`zend_catch`/`zend_end_try` around startup, the request and shutdown, and a contained fatal on an unstarted response becomes a 500. |
| `src/probe/{main,ffi}.rs` | The probe: all `unsafe` in `ffi.rs` with `// SAFETY:` on every block; asserts shim-before-libphp refusal, ABI/minor, six requests, frames from a `socketpair`, the `inet_socket_errno` syscall sample, one JSON line. |
| `tests/critical_shim_contract.rs` (new, protected) | 12 tests, allowed **and** denied halves: reserved names, httpoxy, budgets, pinned INI (exact `disable_functions`), `allow_url_fopen` both ways, INI injection refusal, version parsing, fake `php-config` discovery, no-toolchain refusal naming the package, C source self-containment (no libphp link line, no host path, guards present, startup-before-lockdown order), build-world consistency, ABI layout. |
| `e2e/php-sapi/{Dockerfile,Dockerfile.dockerignore,run-probe.sh,fixtures/*.php}` | `ARG PHP_MINOR=8.4`, repo-root context kept small by `Dockerfile.dockerignore`, five fixtures (hello, vars, egress, exit, fatal). |
| `e2e/php-sapi/seccomp-exec/` + `e2e/php-sapi/.gitignore` | Standalone e2e package: installs the real `SeccompRole::View { egress: false }` from `crates/blastit-sys` and execs the probe (or anything else). Its `target/` and `Cargo.lock` are ignored (the lock is not committed — see attempt 3, MINOR 4). |
| `e2e/php_sapi_probe.py` | Stdlib driver: build, `readelf` assertion, three probe runs with per-run expectations, non-zero + logs on mismatch. |
| `Makefile` | `php-sapi` (builds each minor whose `php-config` exists, names the absent ones, `-D_GNU_SOURCE -Wall -Wextra -Werror`) and `php-sapi-e2e`; **not** part of `check`. |
| `.github/workflows/php-sapi.yml` | fmt + clippy (workspace and `--features probe`, plus the standalone harness) + `make php-sapi-e2e`, `timeout-minutes: 30`. |
| `Cargo.lock` | Only the new package, **no dependencies** (`+4` lines). |

## Acceptance

```console
$ make php-sapi                       # this host: no PHP toolchain, exit 0
php-sapi: skipped 8.4: no php-config for PHP 8.4 (Debian/Ubuntu: apt install php8.4-dev libphp8.4-embed)
php-sapi: skipped 8.3: no php-config for PHP 8.3 (Debian/Ubuntu: apt install php8.3-dev libphp8.3-embed)
php-sapi: skipped 8.2: no php-config for PHP 8.2 (Debian/Ubuntu: apt install php8.2-dev libphp8.2-embed)
php-sapi: 0 shim(s) in …/target/php-sapi; skipped: 8.4 8.3 8.2
```

```console
$ cargo test -p blastit-php-sapi --locked
running 12 tests
test a_missing_toolchain_names_php_config_and_the_package ... ok
test a_proxy_header_never_becomes_http_proxy ... ok
test allow_url_fopen_follows_the_declared_egress_both_ways ... ok
test ini_lockdown_pins_every_system_directive ... ok
test ini_policy_refuses_to_render_an_injection ... ok
test php_versions_parse_only_where_a_shim_exists ... ok
test reserved_server_names_are_unrepresentable ... ok
test server_var_names_values_and_budget_are_bounded ... ok
test the_build_reports_one_consistent_world ... ok
test the_c_shim_is_self_contained_and_guarded ... ok
test an_unsupported_minor_is_refused_by_name ... ok
test a_fake_toolchain_is_discovered_on_path ... ok
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
Doc-tests blastit_php_sapi: 1 passed
```

```console
$ make php-sapi-e2e                   # Docker, exit 0 (/tmp/opencode/php-e2e-a2.log)
php-sapi-e2e: readelf -d libblastit_php84.so
  ok  DT_NEEDED: libc.so.6 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
  ok  php 8.4 abi 1: status 200, cpu_ns 10016071 <= wall_ns 11676568,
      exit()+fatal contained, next request served
      allow_url_fopen=0  socket(AF_INET) errno before/after=0/0  fopen_http=php_setting
      stream_socket=network (Connection refused)
      curl=network (Failed to connect to 127.0.0.1 port 9 after 0 ms: Could not connect to server)
php-sapi-e2e: probe — real View{egress:false} filter, egress denied (both layers)
  ok  php 8.4 abi 1: status 200, cpu_ns 11211633 <= wall_ns 11791707, …
      allow_url_fopen=0  socket(AF_INET) errno before/after=1/1  fopen_http=php_setting
      stream_socket=other (stream_socket_client(): Unable to connect to tcp://127.0.0.1:9 (Unknown error))
      curl=network (Failed to connect to 127.0.0.1 port 9 after 0 ms: Could not connect to server)
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.4 abi 1: status 200, cpu_ns 10905617 <= wall_ns 11837157, …
      allow_url_fopen=1  socket(AF_INET) errno before/after=1/1  fopen_http=other
      stream_socket=other (… (Unknown error))  curl=network (… Could not connect to server)
```

The INI half of run 2, unedited (the reserved names, the effective `disable_functions`, and the
outbound errors):

```json
{"allow_url_fopen": "0",
 "disable_functions": "exec,passthru,shell_exec,system,proc_open,popen,pcntl_exec,pcntl_fork,pcntl_rfork,pcntl_forkx,pcntl_signal,posix_kill,posix_getpgid,posix_setsid,posix_setpgid,dl",
 "open_basedir": "/fixtures:/tmp/blastit-php-probe-view", "ffi_enable": "0", "phar_require_hash": "1",
 "attempts": {
   "fopen_http": {"connected": false, "error": "fopen(http://127.0.0.1:9/): Failed to open stream: no suitable wrapper could be found"},
   "stream_socket": {"connected": false, "errno": 0, "error": "stream_socket_client(): Unable to connect to tcp://127.0.0.1:9 (Unknown error)"},
   "curl": {"connected": false, "errno": 7, "os_errno": 0, "error": "Failed to connect to 127.0.0.1 port 9 after 0 ms: Could not connect to server"}}}
```

Run 1, same fixture, unedited — the same three attempts with no filter (this is what makes the
difference attributable to the filter):

```json
{"fopen_http": {"connected": false, "error": "fopen(http://127.0.0.1:9/): Failed to open stream: no suitable wrapper could be found"},
 "stream_socket": {"connected": false, "errno": 111, "error": "Connection refused"},
 "curl": {"connected": false, "errno": 7, "os_errno": 111, "error": "Failed to connect to 127.0.0.1 port 9 after 0 ms: Could not connect to server"}}
```

```console
$ make check                          # exit 0, no PHP installed, whole workspace
…
interpreter-free build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK                               # full log: /tmp/opencode/make-check-a2.log
```

```console
$ uv run --no-project tools/critical_guard.py --base b2890c4 --node MB.3.12
node MB.3.12: paths ['crates/blastit-php-sapi/**', 'e2e/php-sapi/**',
  'e2e/php_sapi_probe.py', '.github/workflows/**', 'Cargo.toml', 'Cargo.lock', 'Makefile']
  no change to a protected critical test
```

## What the container run actually proves

- **Linkage**: `readelf -d` has one `NEEDED` (`libc.so.6`); the shim *before* `libphp` is
  refused with `undefined symbol: sapi_globals`, not a crash; after
  `dlopen(libphp, RTLD_NOW|RTLD_GLOBAL)` it loads `RTLD_LOCAL` and its Zend/SAPI symbols
  resolve against `libphp`.
- **Response path**: `header('X-Probe: 1')` reaches the head frames (so `send_headers` is a
  real callback, not `php_embed_init`'s no-op), body frames carry `hello from php`, status 200.
- **INI lockdown is applied** (and now actually is): `ini_get` reports the pinned
  `allow_url_fopen` and the full `disable_functions`; no `X-Powered-By` head.
- **Exact CPU**: `cpu_ns` from `CLOCK_PROCESS_CPUTIME_ID` inside the shim, `0 < cpu_ns <= wall_ns`.
- **`php2shell` names**: `PHP_VALUE`/`PHP_ADMIN_VALUE`/`REDIRECT_STATUS`/`HTTP_PROXY` are
  force-injected into the request's server-var array *past* the Rust types; `vars.php` reports
  all four `absent` — the C filter refused them. (PHP 8.4's own `check_http_proxy` is a second
  gate, and it only deletes `HTTP_PROXY` when the *process* env has none — which stage 0
  guarantees. Belt, braces, and the ADR's strip.)
- **Containment**: `exit(3)` after an `echo` returns 200 with its body and `contained: 1`;
  an undefined function returns 500 with `contained: 1`; the next request still returns 200
  with its body — `php_request_shutdown` runs on the bailout path and the executor survives.
- **The cwd never moves** (`cwd_preserved: true`): `SAPI_OPTION_NO_CHDIR` stops
  `php_execute_script` from chdir'ing into the application tree, which would otherwise move
  `/proc/self/cwd` — the worker's private directory of granted proxy sockets — out from under
  every cwd-relative proxy connection.
- **Egress, both layers**: the table in *What attempt 2 changed*.

## Honest limits (recorded, not hidden)

- **Only 8.4 is compiled and loaded anywhere.** 8.2/8.3 are source-compatible (`send_header`
  is `void` in all three; `ini_entries` is `char *` in 8.2 and guarded with
  `#if PHP_VERSION_ID >= 80300`), but no 8.2/8.3 image was built. A wrong assumption there
  fails the C compile loudly rather than half-building.
- **The body is pre-buffered.** `blastit_php_request` takes the bytes the worker already read
  from `view.chan` under its credit (`req->body`), rather than pulling a credit stream from
  inside the shim. The credit loop and `php://input` chunking belong to `MB.3.13`; this ABI
  keeps the shim free of sockets.
- **`cpu_ns` granularity**: `CLOCK_PROCESS_CPUTIME_ID` advances in scheduler ticks (~4 ms at
  HZ=250), so a request that costs less than one tick honestly measures `0`. `hello.php` burns
  ~4 MB of userland CPU so `cpu_ns > 0` is an assertion about the clock, not about HZ.
- **PHP loses the errno when `socket()` itself fails**: `stream_socket_client` says
  `(Unknown error)` and curl's message is identical to the unfiltered run. That is why the
  sandbox claim is carried by the `socket(AF_INET)` errno measured in the same process, and the
  driver's comments say so per attempt.
- **The egress harness is a replica of the worker's *call*, not of the worker.** It installs the
  same `SeccompRole::View { egress: false }` filter a view worker installs, but it is an e2e
  package: the real install site is `MB.3.13`'s post-fork worker, which still has to prove it.

## Budget: declared waiver

**Measured** (`git diff --numstat` for the tracked edits, `wc -l` for the new files):
**4 691 insertions, 1 deletion = 4 692 changed lines across 26 files** (24 new, 2 edited) —
`Makefile` +50/−1, `Cargo.lock` +4/−0 — against the node's ~8-file / ~600-line guard
(≈3.2× the files, ≈7.8× the lines): crate Rust ~2 850 (incl. probe ~1 200 and the protected
test ~470), C shim + ABI header ~800, `e2e/php-sapi/**` + driver + harness ~650, workflow 62,
edits 54.

**Reason.** A **new workspace crate** + a C SAPI shim compiled per minor + an e2e probe that
`dlopen`s the real `libphp` + a seccomp harness + a Docker job is inherently larger than one
leaf: there is no subset of it that is both load-bearing and smaller, because the crate, the
shim and the probe are one interface seen from three sides. The node was landed **whole**
because Docker was available and the probe ran green here; splitting it *after* the probe
passes would add a re-dispatch and a second integration surface without adding proof.

**Not dropped to fit.** Every acceptance row landed and is evidenced above: the degrade path
(`build.rs` cfg + warning + `Toolchain::discover` refusal naming `php-config` and the package),
the native build (`make php-sapi` inside the image → 17,384-byte shim), no `NEEDED libphp`
(`readelf`), the probe rows (200 / `X-Probe: 1` / body / `cpu_ns ≤ wall_ns` / reserved names
absent / `cwd_preserved: true`), containment (`exit()` and fatal both return a status, next
request served), and both egress layers. No rule, no allowed/denied twin and no protected test
was weakened.

**Verdict so far.** Attempt 1 failed for two MAJORs (the ADR-0010 §2 `disable_functions` list
and the missing egress input) and attempt 2 **passed** with four MINORs, all fixed in attempt 3
(effective `ini_get` for three more directives, a NULL ini refused before `sapi_startup`, `:`
refused in a path, and the e2e harness's ignore/lock policy).

## Follow-ups (out of this node's scope)

1. Three `docs/knowledge/LESSONS.md` candidates (docs were out of scope here):
   (a) `php_execute_script()` has its own `zend_try`/`zend_catch`, so a SAPI host must treat
   `false` as the bailout signal — an outer `zend_first_try` never fires for `exit()`/fatals;
   (b) `sapi_startup()` nuls `ini_entries`, so a SAPI lockdown assigned before it is silently
   absent (`ini_get` is the only honest check); and
   (c) `CLOCK_PROCESS_CPUTIME_ID` is tick-granular, so `cpu_ns > 0` needs a fixture that burns
   at least one tick.
2. `MB.3.13` must call the five symbols, check `blastit_php_minor()` against `php_version()`
   before the first request, install `SeccompRole::View { egress }` in the forked worker (the
   e2e harness proves the filter, not the install site), and pass `Egress` from the manifest to
   `IniPolicy::new`.
3. A second CI matrix leg (`--minor 8.3`) is a `build-arg` away once an image ships
   `php8.3-dev` + `libphp8.3-embed`.
