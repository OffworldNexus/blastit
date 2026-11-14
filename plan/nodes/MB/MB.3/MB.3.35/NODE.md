---
id: MB.3.35
title: "Name the shipped PHP shims per ADR-0010 s4.1 (libblastit_php<minor>-<arch>-<libc>.so) everywhere"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.3.13.2]
paths:
  - Makefile
  - e2e/php_sapi_probe.py
  - "e2e/php-sapi/**"
  - "crates/blastit-php-sapi/src/toolchain.rs"
  - "crates/blastit-php-sapi/src/lib.rs"
  - "crates/blastit-php-sapi/tests/critical_shim_contract.rs"
  - "crates/blastit-php-sapi/tests/php_native.rs"
  - "crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h"
  - crates/blastit/src/stage0.rs
  - "crates/blastit-view/tests/php_native.rs"
  - "docs/research/php-embed-sapi.md"
  - docs/knowledge/LESSONS.md
  - ".github/workflows/php-sapi.yml"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-14T23:17"
updated: "2026-11-15T00:08"
---

## Goal

Make the shim a build produces the shim a boot selects: `make php-sapi` writes
the ADR-0010 §4.1 name `libblastit_php<major><minor>-<arch>-<libc>.so`
(`libblastit_php84-x86_64-gnu.so`), and `blastit dev` finds it beside the
binary / in `target/php-sapi` with no `BLASTIT_PHP_SHIM_DIR` export.

## Context

- `MB.3.13.2` implemented boot selection exactly as `ADR-0010` §4.1 specifies:
  `(host major.minor, target_arch, target_env gnu|musl)` ->
  `libblastit_php<major><minor>-<arch>-<libc>.so`, looked up in
  `BLASTIT_PHP_SHIM_DIR`, beside the executable, then `target/php-sapi`.
- But `Makefile`'s `php-sapi` rule, `crates/blastit-php-sapi/src/toolchain.rs`'s
  `Minor::shim_file()`, `e2e/php_sapi_probe.py` and `e2e/php-sapi/*` still say
  `libblastit_php84.so`, so the native dev path finds no shim and the only way
  to run it is a manual `BLASTIT_PHP_SHIM_DIR` — the exact two-command DX the
  PHP page promises, broken by a file name.
- One name for one artifact: do not keep the old spelling as an alias. Nothing
  has been released in this series.

## Acceptance

```console
$ make php-sapi
$ ls target/php-sapi
$ grep -rn 'libblastit_php[0-9][0-9]\.so' --include=* Makefile crates e2e docs | grep -v '\.so\.' ; echo "grep=$?"   # 1
$ make check
$ uv run --no-project tools/plan.py validate
```

- `make php-sapi` writes `libblastit_php84-x86_64-gnu.so` (musl targets say
  `-musl`), with the ELF `DT_SONAME` matching the file name.
- `blastit dev` on a `[route.php]` manifest finds that shim with no
  `BLASTIT_PHP_SHIM_DIR` and no extra flag; the boot log names the shim it
  selected.
- The old name survives only in the ADR's history; `grep` above exits 1.
- `crates/blastit-php-sapi/tests/critical_shim_contract.rs` (protected) is
  updated to the new name under `uv run --no-project tools/critical_guard.py
  --node MB.3.35`; record that authorization in RESULT.
- `make check` green (including `make php-sapi`'s consumers and `e2e/`'s probe
  helper, which is not run by `make check` — say so and state how it was
  verified), `plan.py validate` OK.

## Notes

- `MB.3.18` owns the full `{x86_64,aarch64} x {gnu,musl}` build matrix; this
  node only makes today's native build selectable.
- Keep it ≤ 600 changed lines; no new dependency.
