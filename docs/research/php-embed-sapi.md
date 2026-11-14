# Embedding PHP via `libphp.so` (`sapi/embed` vs Custom `sapi_module_struct`)

- Node: MB.12   - Author: architect (primary-source research)   - Date: 2026-11-14

## TL;DR (<= 8 lines)

- `php_embed_init()` in `sapi/embed/php_embed.c` is a CLI-like one-shot helper (`SG(headers_sent) = 1`, `no_headers = 1`, no-op `send_header`, registers `dl()`): a web server must instead register its own `sapi_module_struct` (`sapi_startup` -> `php_module_startup` -> per-request `php_request_startup` / `php_execute_script` / `php_request_shutdown`) against the exact same `libphp.so` ([`php_embed.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/embed/php_embed.c), [`SAPI.h`](https://github.com/php/php-src/blob/PHP-8.4/main/SAPI.h)).
- **Zend/SAPI has no cross-minor C ABI**: `sapi_globals_struct` grew `request_parse_body_context` in PHP 8.4, `sapi_module_struct.ini_entries` changed signature, and `EG(bailout)` (`zend_first_try` `jmp_buf`) shifts offset on every minor release (`20220829` 8.2, `20230831` 8.3, `20240924` 8.4). Within one `(major, minor, NTS, non-debug)` tuple, patch releases (`8.4.x`) preserve ABI.
- **Stock Linux distro `libphp` packages are NTS** (`apt install libphp-embed` on Debian/Ubuntu, `apk add php84-embed` on Alpine, `dnf install php-embedded` on Fedora/RHEL/Remi) and match 100% of distro PHP C extensions (`pdo_pgsql`, `intl`, `redis`, `opcache`); ZTS builds (used by Go's FrankenPHP because Go cannot `fork()`) cannot load stock distro NTS extensions.
- Because Blastit's `init` is single-threaded and forks dedicated single-threaded `view` worker processes, **NTS `libphp.so` fits Blastit's process topology natively** (the NGINX Unit `nxt_php_sapi.c` model) without needing ZTS.
- Embedding eliminates the entire FastCGI/CGI wire and `fastcgi_ini_parser` surface (`PHP_VALUE`, `PHP_ADMIN_VALUE`, `REDIRECT_STATUS`, `cgi.fix_pathinfo`, CVE-2012-1823, CVE-2024-4577/8926/8927), and gives exact per-request `CLOCK_PROCESS_CPUTIME_ID` timing.
- `libphp.so` is licensed under **PHP-3.01** (plus transitive distro libs such as `libedit` `LGPL-2.0-or-later` on Alpine): Blastit must dynamically `dlopen` the **host's** `libphp.so` (never bundle `libphp` or set `DT_NEEDED` on `blastit`), keeping static/Python/Node/JVM hosts 100% PHP-free.

## Developer Experience Implications

- **Zero `php-fpm.conf`, zero FastCGI sockets, zero `listen` knob.** The developer writes `[route.php] entry = "public/index.php"` and runs `blastit dev`. Blastit discovers the host's `libphp.so` (`BLASTIT_LIBPHP` override) and runs the front controller directly inside isolated per-route worker processes.
- **Stock OS extensions work out of the box.** Installing `libphp-embed` + `php-pgsql` (`apt`) or `php84-embed` + `php84-pdo_pgsql` (`apk`) uses the exact same NTS Zend ABI and `/etc/php/8.x/embed/conf.d/` (or `/etc/php84/conf.d/`) `.ini` scan directory as CLI/FPM — zero custom extension compilation.
- **Official Docker Hub `php:8.4-*` caveat must be documented.** Upstream `docker-library/php` (`php:8.4-cli`, `php:8.4-fpm-alpine`) compiles PHP without `--enable-embed`, so Dockerfiles for Blastit PHP apps should start from `debian:trixie-slim` (`apt-get install -y libphp8.4-embed php8.4-pgsql ...`) or `alpine:3.21` (`apk add php84-embed php84-pdo_pgsql ...`), which installs binary extensions in ~1 s without `docker-php-ext-install` C builds.

## Technical Findings

### 1. Why `php_embed_init()` is NOT the web-request entry point

Inspection of [`sapi/embed/php_embed.c` (PHP 8.4)](https://github.com/php/php-src/blob/PHP-8.4/sapi/embed/php_embed.c) reveals four behaviours unsuitable for a multi-request HTTP worker:

1. `php_embed_init()` calls `php_request_startup()` once inside module startup and immediately sets `SG(headers_sent) = 1; SG(request_info).no_headers = 1;` — suppressing `header()` and `http_response_code()`.
2. `php_embed_send_header` is an empty function (`{}`), `read_post` is `NULL`, and `php_embed_read_cookies` returns `NULL`.
3. `php_embed_single_write` writes directly to `STDOUT_FILENO` / `stdout` instead of framing HTTP response chunks.
4. `additional_functions` registers `ZEND_FE(dl, arginfo_dl)`, exposing runtime `dl()` arbitrary `.so` loading unless explicitly disabled.

**Correct SAPI host pattern (proven by NGINX Unit `nxt_php_sapi.c` and FrankenPHP `frankenphp.c`):**
`libphp.so` compiled with `--enable-embed=shared` exports the full `SAPI_API` and `PHPAPI` C surface (`sapi_startup`, `sapi_shutdown`, `php_module_startup`, `php_module_shutdown`, `php_request_startup`, `php_execute_script`, `php_request_shutdown`, `php_register_variable_safe`, `sapi_globals`). A ~400-line C SAPI shim defines a custom `sapi_module_struct`:

- `name = "blastit"`, `pretty_name = "Blastit PHP Worker"`
- `ub_write`: streams body chunks into the Rust worker's `view.chan` `RESP_BODY` sink (respecting backpressure credit).
- `send_headers` / `header_handler`: reads `SG(sapi_headers).http_response_code` (default `200`) and walks `SG(sapi_headers).headers` (`zend_llist` of `sapi_header_struct`), validating RFC 9110 header tokens/values and emitting `RESP_HEAD` before the first body chunk.
- `read_post`: pulls request body bytes from the Rust worker's `REQ_BODY` stream (enabling `php://input` and `$_POST` / `$_FILES` without pre-buffering raw uploads on `Stream`-capable routes).
- `read_cookies`: returns the request's `Cookie` header buffer (`$_COOKIE` parsed natively by Zend).
- `register_server_variables`: populates `$_SERVER` via `php_register_variable_safe` from Blastit's canonicalized request metadata (`SCRIPT_FILENAME`, `DOCUMENT_ROOT`, `SCRIPT_NAME`, `PATH_INFO`, `REQUEST_URI`, `QUERY_STRING`, `REQUEST_METHOD`, `SERVER_PROTOCOL`, `SERVER_NAME`, `SERVER_PORT`, `REMOTE_ADDR`, `REMOTE_PORT`, `REQUEST_SCHEME`, `HTTPS`, `CONTENT_TYPE`, `CONTENT_LENGTH`, plus filtered `HTTP_*` headers — never `HTTP_PROXY`), and imports sanitized Stage-1 environment variables when `variables_order` includes `E`.
- `log_message` / `sapi_error`: forwards PHP warnings/errors line-by-line to stderr / `app.output` (2048-byte line cap).
- `ini_entries`: pins `ZEND_INI_SYSTEM` directives at module startup (`allow_url_include=0`, `auto_prepend_file=`, `auto_append_file=`, `user_ini.filename=`, `expose_php=0`, `enable_dl=0`, `open_basedir=<manifest_dir>:<view_tmp>`, `max_execution_time=<timeout_s>`). Unlike FastCGI, there is **no** `fastcgi_ini_parser`, so request bytes can never mutate INI state.
- `additional_functions`: registers `fastcgi_finish_request()` (flushes headers + body `RESP_END` so Symfony `kernel.terminate` / Laravel terminable middleware runs post-response before `DONE` releases the scheduler slot) plus `getallheaders()` / `apache_request_headers()`. **`dl()` is omitted.**

### 2. C ABI Coupling Across PHP Minor Versions (8.2, 8.3, 8.4, 8.5)

Comparing [`PHP-8.2/main/SAPI.h`](https://github.com/php/php-src/blob/PHP-8.2/main/SAPI.h) against [`PHP-8.4/main/SAPI.h`](https://github.com/php/php-src/blob/PHP-8.4/main/SAPI.h):

1. `sapi_globals_struct` (`extern SAPI_API sapi_globals_struct sapi_globals;` under NTS) appended `sapi_request_parse_body_context request_parse_body_context` in PHP 8.4.
2. `sapi_module_struct.ini_entries` changed from `char *` (8.2) to `const char *` (8.4).
3. `zend_executor_globals` (`executor_globals`, accessed by `zend_first_try` / `zend_catch` around `setjmp(*EG(bailout))`) changes field layout on **every** PHP minor release (`ZEND_MODULE_API_NO`: `20220829` for 8.2, `20230831` for 8.3, `20240924` for 8.4, `20250925` for 8.5). `zend_first_try` is mandatory: any PHP call to `exit()` / `die()`, uncaught fatal error, or `memory_limit` exhaustion executes `zend_bailout()` (`longjmp`), which crashes the worker if `EG(bailout)` was read from the wrong struct offset.
4. **Conclusion on Rust FFI vs C SAPI Shims:**
   - Declaring Zend/SAPI structs by hand in pure Rust and calling `dlopen("libphp.so")` across 8.2–8.5 **will segfault across minor versions**.
   - Instead, mirroring NGINX Unit (`php82.unit.so`, `php83.unit.so`, `php84.unit.so`), Blastit compiles one tiny (~25 KiB, Apache-2.0) C shim per supported PHP minor (`libblastit_php{82,83,84}-<arch>-<libc>.so`) against that minor's official `php-src` headers, exposing a **single frozen 5-function Blastit C ABI** (`blastit_php_abi_version`, `blastit_php_minor`, `blastit_php_worker_init`, `blastit_php_worker_shutdown`, `blastit_php_request` in `c/include/blastit_php_abi_v1.h`) to the Rust worker!
   - At Stage 0 (before `execve`), Blastit checks `open`/`fstat` + static ELF `.dynsym` (`executor_globals` present && `tsrm_startup` absent -> NTS, zero Stage-0 `dlopen`). After the Stage-0→Stage-1 secret-stripping `execve` barrier, a short-lived unprivileged Stage-1 probe child `dlopen`s fd-verified `/proc/self/fd/<libphp_fd>` (`RTLD_NOW | RTLD_GLOBAL`), calls `const char *php_version(void)` ([`main/php_main.h`](https://github.com/php/php-src/blob/PHP-8.4/main/php_main.h)) to pick `libblastit_php<major><minor>-<arch>-<libc>.so`, `dlopen`s the shim (`RTLD_NOW | RTLD_LOCAL`), and runs `blastit_php_worker_init` + `blastit_php_worker_shutdown`. A host with an unsupported minor (e.g. PHP 7.4) or a ZTS-only `libphp` fails cleanly before `$PORT` is bound, naming the version found!

### 3. NTS vs ZTS & Process Model

- FrankenPHP embeds PHP inside a multi-goroutine Go binary; because Go's runtime cannot safely `fork()` after initialization, FrankenPHP requires `--enable-zts` (`libphp-zts.so`), forcing users onto custom Homebrew/static-php repositories because Debian, Ubuntu, Alpine, and RHEL ship **NTS** extensions by default ([php-src#18568](https://github.com/php/php-src/issues/18568), [FrankenPHP docs](https://frankenphp.dev/docs/static)).
- Blastit's supervisor (`init`) is single-threaded Rust and forks dedicated per-route `view` processes (`UID_BASE + 1000 + i`). Each PHP worker process is **single-threaded NTS**, handling one in-flight request at a time over its `view.chan` socketpair.
- **Concurrency & CoW sharing:** Per-route concurrency up to `BLASTIT_VIEW_MAX_WORKERS` is served by forking multiple NTS worker processes under the route's UID on demand (lazy scale-out, idle scale-in). Even better, a per-route unprivileged PHP zygote (`UID_BASE + 1000 + i`) can call `php_module_startup` (initializing Zend + OPcache anonymous `mmap` SHM) once and `fork()` sibling workers on demand, sharing `libphp.so`, extensions, and OPcache bytecode across workers copy-on-write (~2–4 MiB marginal RSS per concurrent worker).

### 4. Distribution & Licensing Reality

| Platform / Distro | Package Providing NTS `libphp.so` | Shared Object Path | Size | Extensions Compatible? |
|---|---|---|---|---|
| Debian 12/13 & Ondřej Surý PPA | `libphp-embed` (`libphp8.2-embed`, `libphp8.3-embed`, `libphp8.4-embed`) | `/usr/lib/libphp.so`, `/usr/lib/libphp8.4.so` | ~1.9 MiB (5.8 MiB installed) | Yes — all `php8.X-*` packages |
| Ubuntu 22.04 / 24.04 / 26.04 | `libphp-embed` (`universe`) | `/usr/lib/libphp.so`, `/usr/lib/libphp8.X.so` | ~1.9 MiB | Yes — all `php8.X-*` packages |
| Alpine 3.20 / 3.21 / 3.23 | `php83-embed`, `php84-embed` (`community`) | `/usr/lib/libphp84.so` | ~1.9 MiB (4.8 MiB installed) | Yes — all `php84-*` packages |
| Fedora / RHEL / Alma / Remi | `php-embedded` | `/usr/lib64/libphp-8.4.so`, `/usr/lib64/libphp.so` | ~2.1 MiB | Yes — all `php-*` packages |
| Official `php:8.4-cli` / `php:8.4-fpm` Docker | **Not included** (`--enable-embed` omitted upstream) | *(absent — use `debian:*-slim` or `alpine` + `*-embed`)* | — | — |

- **License ([`PHP-3.01`](https://raw.githubusercontent.com/php/php-src/PHP-8.4/LICENSE)):** Clause 4 forbids naming derived products `"PHP*"` (`blastit` complies); Clause 6 requires attribution only when **redistributing** PHP software; Alpine's `php84-embed` also dynamically links `libedit` (`LGPL-2.0-or-later` / BSD). Because Blastit **never bundles `libphp.so`** and only `dlopen`s the host's installed `libphp` at runtime (while our tiny `libblastit_php8X.so` shims contain solely our own Apache-2.0 C code compiled against header declarations), Blastit distributes zero PHP-licensed or LGPL binaries.

## Comparison Table

| Option | SPDX License | `php2shell` Surface | CPU Accounting | Host Prerequisite | Extension Compatibility | Verdict |
|---|---|---|---|---|---|---|
| **A. Host NTS `libphp.so` + per-minor C SAPI shim (`libblastit_php8X.so`) in forked `view` worker** | Blastit shim: `Apache-2.0`; Host `libphp`: host-installed (`PHP-3.01`) | **Structurally absent** (no FastCGI/CGI wire, no `PHP_VALUE` parser, no `fix_pathinfo`) | **Exact** (`CLOCK_PROCESS_CPUTIME_ID` around `php_execute_script`) | `libphp-embed` / `php84-embed` / `php-embedded` (`BLASTIT_LIBPHP` override) | **100%** of stock distro NTS extensions | **RECOMMENDED (Primary & Sole PHP Backend)** |
| **B. Bundled static/shared ZTS `libphp` inside `blastit`** | Contaminates distribution (`PHP-3.01` + transitive C libs) | Structurally absent | Exact | None, ~50 MiB binary | **Broken** for stock distro NTS extensions (`pdo_pgsql`, `intl`, `redis`) | **Rejected** (breaks extensions, bloats static/Node/Python installs, couples PHP CVEs to Blastit) |
| **C. Pure-Rust `dlopen("libphp.so")` without per-minor C shim** | `Apache-2.0` | Structurally absent | Exact | `libphp-embed` | 100% | **Rejected** (`SG(...)` & `EG(bailout)` offsets change per PHP minor → segfaults on `exit()`) |
| **D. External `php-fpm` over FastCGI** | `Apache-2.0` client | Present on wire; requires 25-row param lockdown + bind-window freeze | Requires external side-channel / `getrusage` hook + `SO_PEERCRED` verification | `php-fpm` binary | 100% | **Superseded** (extra daemon, socket dance, buffered raw bodies, side-channel CPU complexity) |

## Concrete Recommendations for Blastit

1. Make **Embedded NTS `libphp` in dedicated per-route `view` workers (`[route.php] entry = "public/index.php"`)** the sole PHP backend for `0.2.1`.
2. Ship tiny (~25 KiB) Apache-2.0 `libblastit_php82.so`, `libblastit_php83.so`, `libblastit_php84.so` SAPI bridge modules alongside `blastit`, dynamically `dlopen`ing the host's NTS `libphp` (`BLASTIT_LIBPHP` override) strictly inside unprivileged per-route worker processes — **never** linking `libphp` into `blastit` (`init`/`http`/`scheduler`/`dbproxy`).
3. Wrap every request in `zend_first_try` / `zend_catch` + `setitimer(ITIMER_PROF)` / scheduler timeout backstop so `exit()`, `die()`, fatal errors, and `memory_limit` cleanly terminate the current request (or recycle the worker on hard bailout) without breaking `view.chan` framing.
4. Measure exact worker CPU via `clock_gettime(CLOCK_PROCESS_CPUTIME_ID)` before `php_request_startup` and after `php_request_shutdown` (including `fastcgi_finish_request` post-response work) and report it via `RESP_END_COST` on `view.chan`.

## Unverified / Open Questions

- **PHP 8.5 SAPI struct:** Ubuntu 26.04 (`resolute`) ships `libphp8.5-embed`; adding `libblastit_php85.so` requires checking `PHP-8.5/main/SAPI.h` once 8.5 headers are targeted in CI.
- **OPcache SHM across per-route PHP zygote forks:** `opcache.enable=1` mmaps shared memory during `php_module_startup`; a quick Linux + gVisor probe during `MB.3` implementation should verify `mmap(MAP_SHARED | MAP_ANONYMOUS)` behavior under `SeccompRole::View` when a per-route PHP zygote forks sibling workers.

## Sources

- PHP 8.4 `sapi/embed/php_embed.h` & `php_embed.c`: <https://github.com/php/php-src/blob/PHP-8.4/sapi/embed/php_embed.c>
- PHP 8.4 `main/SAPI.h`: <https://github.com/php/php-src/blob/PHP-8.4/main/SAPI.h>
- PHP 8.2 `main/SAPI.h` (ABI comparison): <https://github.com/php/php-src/blob/PHP-8.2/main/SAPI.h>
- PHP 8.4 `main/php_main.h`: <https://github.com/php/php-src/blob/PHP-8.4/main/php_main.h>
- PHP License v3.01: <https://raw.githubusercontent.com/php/php-src/PHP-8.4/LICENSE>
- Debian `libphp-embed`: <https://packages.debian.org/sid/libphp-embed>
- Ubuntu `libphp-embed` / `libphp8.4-embed`: <https://packages.ubuntu.com/noble/libphp-embed>
- Alpine `php84-embed`: <https://pkgs.alpinelinux.org/package/v3.21/community/x86_64/php84-embed>
- NGINX Unit `auto/modules/php` & `src/nxt_php_sapi.c` (Apache-2.0 NTS SAPI precedent): <https://github.com/nginx/unit/issues/854>, <https://github.com/php/php-src/issues/18568>
- FrankenPHP internals & ZTS vs NTS extension trade-offs: <https://frankenphp.dev/docs/internals>, <https://frankenphp.dev/docs/static>, <https://github.com/php/frankenphp/issues/1602>
