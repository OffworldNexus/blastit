---
id: MB.3.12
title: "crates/blastit-php-sapi: per-minor Apache-2.0 C SAPI shims (blastit_php_abi_v1) + critical tests"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.3.11]
paths:
  - "crates/blastit-php-sapi/**"
  - "e2e/php-sapi/**"
  - e2e/php_sapi_probe.py
  - ".github/workflows/**"
  - Cargo.toml
  - Cargo.lock
  - Makefile
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-14T14:39"
updated: "2026-11-14T15:57"
---

# Goal

Build `crates/blastit-php-sapi`: the Apache-2.0 per-PHP-minor C SAPI shims
(`libblastit_php8{2,3,4}.so`) exposing one versioned C vtable, `blastit_php_abi_v1`, to
Rust — with the `exit()`/bailout guard, the INI lockdown, direct `$_SERVER` population and
exact per-request CPU.

# Context

- ADR-0010 §1/§2/§5 and `docs/research/php-embed-sapi.md`: a custom `sapi_module_struct`
  (`sapi_startup` → `php_module_startup` → per request `php_request_startup` /
  `php_execute_script` / `php_request_shutdown`), `ub_write` → `view.chan` body frames,
  `send_headers` → a validated `RESP_HEAD`, `read_post` from the credit stream,
  `read_cookies` from the request's `Cookie` buffer, `register_server_variables` from typed
  head fields (`SCRIPT_FILENAME` pinned to `entry`, `HTTP_PROXY` never populated),
  `log_message` bounded to 2048-byte lines.
- The `zend_first_try { … } zend_catch { … } zend_end_try()` wrapper is mandatory: PHP
  `exit()`/`die()`/fatals `longjmp` through `EG(bailout)` and unwinding across Rust frames
  is UB. The shim returns plain status codes; Rust never sees a jump.
- Per-minor build rule: never link `libphp` into `init`, `http`, `scheduler` or `dbproxy`;
  the shim is `dlopen`ed `RTLD_NOW | RTLD_LOCAL` strictly inside the `view` worker. The
  shim itself must state the PHP minor it was compiled for, so `MB.3.13`'s boot gate can
  refuse a mismatch.
- `CLOCK_PROCESS_CPUTIME_ID` is read around `php_execute_script` in the shim/worker loop,
  outside PHP userland — no side channel, nothing a script can forge.

# Acceptance

```console
$ make php-sapi                      # builds all shims that have headers on this host, or reports which are absent
$ cargo test -p blastit-php-sapi --locked
$ make check
```
- The vtable is versioned (`blastit_php_abi_v1`) and its ABI number check is a test: a
  shim built for one minor refuses a `libphp` of another.
- Critical tests (protected, `crates/blastit-php-sapi/tests/critical_*.rs`): `exit()`,
  `die()` and a fatal error each return a status code and leave the worker able to serve
  the next request; `PHP_VALUE`/`PHP_ADMIN_VALUE`/`REDIRECT_STATUS` are structurally
  unrepresentable (no code path accepts an INI string from a request); `HTTP_PROXY` is never
  populated from a `Proxy:` header; `open_basedir`/`allow_url_include`/`user_ini.filename`
  are set as `INI_SYSTEM` and cannot be relaxed by a request.
- The shim source contains no `libphp` link line and no host path; it is pure Apache-2.0
  and self-contained per minor.

# Notes

- Hosts without `php-dev` headers for a minor: that shim is skipped with a clear Makefile
  line, never a silent half-build. `MB.3.13` decides what a missing shim means at boot.
- No Rust `unsafe` outside the FFI boundary module, each with a `// SAFETY:` comment.

## Toolchain reality (checked by the owner on this host, Debian 13 trixie)

**This machine has no PHP toolchain at all**: no `php`, no `php-config`, no `libphp.so`,
no `/usr/include/php`. Therefore:

- **Never `apt install` a system package** to make this build pass: that needs `sudo` the
  loop does not have, and it would make the build depend on a machine mutation.
- **`build.rs` degrades honestly.** Find `php-config` via `PHP_CONFIG`, then `PATH`
  (`php-config`, `php-config8.4`, `8.3`, `8.2`, in that order). When none exists, emit a
  warning, define a cfg (e.g. `no_php_toolchain`), compile the crate **without** the ABI
  surface, and let the Rust side refuse the PHP backend with a message naming the missing
  toolchain and the package. A C dependency that breaks `make check` on a machine without
  PHP would make the whole repo unbuildable for contributors — that is forbidden.
- **The shim is built and exercised only in the container, by design.** A green
  `make check` here proves the *Rust* degrades correctly, never that the C works. Say that
  in the node, the crate docs and the RESULT in those words.
- **CI carries the real proof**: a Docker job (image + `--locked` build + the probe below),
  kept **out of `make check`** exactly like `e2e/` (Docker + a PHP image), so the shim
  cannot rot unnoticed.

### Linkage rule to verify, not assume

The shim must not carry a `DT_NEEDED` on `libphp` (that would link PHP into whatever loads
it). The worker `dlopen`s `libphp.so` `RTLD_NOW | RTLD_GLOBAL` **inside the view worker
only**, then `dlopen`s the shim `RTLD_NOW | RTLD_LOCAL`, whose undefined Zend/SAPI symbols
resolve against the global handle. Prove the shim's `readelf -d` has no `NEEDED libphp`,
and that loading the shim *before* libphp fails cleanly instead of crashing.

## Container + probe (the acceptance that counts)

- `e2e/php-sapi/`: a `Dockerfile` on `debian:trixie-slim` installing
  `php8.4-dev libphp8.4-embed gcc make pkg-config libc6-dev` (no external PHP server), copying the
  repo read-only, running `make php-sapi` inside, then building and running the probe.
  `ARG PHP_MINOR=8.4` so a second minor is a build arg, not a rewrite.
- `e2e/php_sapi_probe.py` (stdlib only, same shape as the other e2e drivers): builds the
  image, runs it, parses one JSON line, fails non-zero with the container log on mismatch.
- **The probe is a real load, not a header-only compile.** A small Rust binary (in the
  image, `crates/blastit-php-sapi/tests/probe` or a `--bin` behind the cfg) that:
  1. `dlopen`s `libphp.so` (`BLASTIT_LIBPHP`/arg), then the shim;
  2. creates a `socketpair(AF_UNIX)` to stand in for `view.chan`;
  3. for a fixture `public/index.php` that `echo "hello from php"` and
     `header('X-Probe: 1')`, calls shim `worker_init` → `request(entry, server_vars, sink)`;
  4. asserts status `200`, head contains `X-Probe: 1`, body contains `hello from php`,
     and `cpu_ns` is present, `> 0` and `<= wall_ns`;
  5. asserts a `PHP_VALUE` entry placed in `server_vars` is **structurally dropped**;
  6. runs a fixture that `exit()`s and one that raises a fatal: both return a status code
     (no unwind, no process death) and the **next** request still serves — re-entrancy
     proved by two consecutive requests;
  7. prints one JSON line (`{status, head, body, cpu_ns, wall_ns, contained}`).
- CI: a job running `make php-sapi-e2e` (`--locked`), separate from `make check`.

## Acceptance (this node)

```console
$ make check                      # degrades honestly here: no PHP toolchain, crate compiles, Rust refuses with the package name
$ make php-sapi-e2e               # Docker: builds libblastit_php8.4.so and runs the probe above
$ cargo test -p blastit-php-sapi --locked
```
- On this host: `make check` green **without** PHP installed, and the refusal message names
  `php-config` and the distro package.
- In the container: the seven probe assertions pass, and the reported `cpu_ns` is non-zero.
- `readelf -d libblastit_php8.4.so | grep NEEDED` shows no `libphp`.
