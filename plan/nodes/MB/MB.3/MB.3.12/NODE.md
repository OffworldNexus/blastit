---
id: MB.3.12
title: "crates/blastit-php-sapi: per-minor Apache-2.0 C SAPI shims (blastit_php_abi_v1) + critical tests"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.3.11]
paths: ["crates/blastit-php-sapi/**", Cargo.toml, Cargo.lock, Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T14:39"
updated: "2026-11-14T14:39"
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
