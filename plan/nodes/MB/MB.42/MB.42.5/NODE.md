---
id: MB.42.5
title: "Add PHP 8.6 support end-to-end (shim, discovery, 16-leg matrix, packaging, docs) for PHP 8.3-8.6"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.2]
paths:
  - Cargo.toml
  - Cargo.lock
  - Makefile
  - "crates/**"
  - "e2e/**"
  - "scripts/**"
  - ".github/workflows/**"
  - "docs/**"
  - README.md
  - CHANGELOG.md
  - AGENTS.md
  - ARCHITECTURE.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T15:48"
updated: "2026-11-20T00:27"
---

# Goal
Add **PHP 8.6** to the supported set so `0.2.0` ships **PHP 8.3, 8.4, 8.5 and 8.6** end-to-end: the per-minor shim, the discovery gate, the packaging matrix and the shim/worker proofs.

The owner already verified the exact live distro packages in Docker on this host. **Do not guess; use these verbatim.**

## Verified leg definitions (owner-measured)
**`gnu`, `x86_64` + `aarch64` — `debian:bookworm-slim` + Ondřej Surý's signed APT repo:**
```sh
curl -fsSL https://packages.sury.org/php/apt.gpg -o /etc/apt/keyrings/sury.gpg
echo "deb [signed-by=/etc/apt/keyrings/sury.gpg] https://packages.sury.org/php/ bookworm main" \
    > /etc/apt/sources.list.d/sury.list
apt-get update
apt-get install -y --no-install-recommends \
    libphp8.6-embed php8.6-dev php8.6-curl build-essential pkg-config
```
Verified: `libphp8.6-embed` = `8.6.0`; the library is **`/usr/lib/libphp8.6.so`**.
(`bookworm-slim` is glibc **2.36**, which is the floor this leg states — it is *lower* than the `trixie`/`26.04` legs, so it must not raise `glibc_floor`.)

**`musl`, `x86_64` + `aarch64` — `alpine:edge` + the `testing` repository:**
```sh
echo "https://dl-cdn.alpinelinux.org/alpine/edge/testing" >> /etc/apk/repositories
apk update && apk add --no-cache \
    php86-embed php86-dev php86-curl build-base musl-dev pkgconf
```
Verified: the library is **`/usr/lib/php86/libphp.so`** (per-minor directory, packed `86` spelling).

# Exact Deliverables
1. **`crates/blastit-php-sapi`:**
   - `src/toolchain.rs`: add `Minor::Php86` — `(8, 6) => Some(Self::Php86)`, `php_version_number() == 806`, digits `"86"`, dotted `"8.6"`, config binaries `["php-config8.6", "php-config"]`.
   - `SUPPORTED_MINORS` becomes `[Minor; 4] = [Minor::Php86, Minor::Php85, Minor::Php84, Minor::Php83]` (**newest first**, matching the existing order discipline).
   - `shim_file()` -> `libblastit_php86-<arch>-<libc>.so`; confirm `DT_SONAME` matches.
   - Verify `c/blastit_php_sapi.c` (and `c/include/blastit_php_abi_v1.h`) compiles cleanly with `-Wall -Wextra -Werror` against the PHP 8.6 headers; fix any new `-Werror` diagnostic (PHP 8.6 may add deprecations) **without** weakening the flags.
   - Extend the existing `tests/critical_shim_contract.rs` / `tests/php_native.rs` coverage to the 4th minor.
2. **`crates/blastit-view-php/src/discovery.rs`:**
   - `LIBPHP_CANDIDATES: [&str; 13]` — prepend, newest-first: `"libphp8.6.so"`, `"libphp-8.6.so"`, `"libphp86.so"`.
   - `LIBPHP_PROBES: [LibphpProbe; 17]` — prepend the 8.6 block: `libphp8.6.so` (no dir), `libphp-8.6.so` (no dir), `libphp86.so` (no dir), `libphp.so` with `per_minor_dir: Some("php86")`. Keep `Count`/`PROVEN`-style pins in sync (**13 -> 17**, and every test that pins the probe count or the `LIBPHP_CANDIDATES` length).
   - `library_dirs()` — add `"/usr/lib/php86"` and `"/usr/local/lib/php86"`, in the same relative position as the other per-minor dirs (after `/usr/lib`, **before** `/usr/local/lib`).
   - `SUPPORTED_PHP` -> `"PHP 8.3, 8.4, 8.5 and 8.6"`; update every unit/critical test that pins that exact string, plus `PHP_PACKAGES` if it names a minor.
   - Doc comment on `LIBPHP_CANDIDATES`/`LIBPHP_PROBES` should mention the Alpine `86` spelling.
3. **`crates/blastit/src/stage0/`** and `crates/blastit/tests/common/libphp_probes.rs`: update any pinned supported-minor list / probe fixture.
4. **Build + matrix:**
   - `Makefile`: `PHP_MINORS ?= 8.6 8.5 8.4 8.3`.
   - `scripts/platform_matrix.py`:
     * `PHP_MINORS` -> `{"8.6": "86", "8.5": "85", "8.4": "84", "8.3": "83"}`.
     * `PHP_BUILDS` gains `("8.6", "gnu")` = `{base: debian:bookworm-slim, dev: php8.6-dev, embed: libphp8.6-embed, curl: php8.6-curl, libphp: /usr/lib/libphp8.6.so, suites: True}` and `("8.6", "musl")` = `{base: alpine:edge, dev: php86-dev, embed: php86-embed, curl: php86-curl, libphp: /usr/lib/php86/libphp.so, suites: True}`.
     * Add a declarative repo-setup field per leg (e.g. `"extra_repo": "sury"` for gnu-8.6 and `"extra_repo": "alpine-edge-testing"` for musl-8.6) so `e2e/php-sapi/Dockerfile` and `.github/workflows/php-sapi.yml` perform the setup **from the matrix**, never from a second hard-coded list here.
     * Total PHP shim legs become **16 = 4 minors x 4 (arch, libc) targets**; fix any `--count`/documented-count assumption.
5. **`e2e/php-sapi/Dockerfile` + `e2e/php_sapi_probe.py`:** accept and perform the `extra_repo` setup (sury keyring+sources for Debian bookworm; `edge/testing` appended to `/etc/apk/repositories` for Alpine edge) before installing packages, and pass `PHP_MINOR=8.6`, `PHP_DEV`, `PHP_EMBED`, `PHP_CURL`, `LIBPHP` through as today.
6. **`.github/workflows/php-sapi.yml`:** the new 8.6 legs must appear from `platform_matrix.py --matrix shims --minor 8.6` with **no per-leg edits**; `make rs-test-php-image` must run `critical_php_backend` + `critical_php_embed` on the real 8.6 pair (gnu leg at minimum), and `php_sapi_probe.py` must real-load `libphp8.6.so` + `libblastit_php86-*.so` and serve a request.
7. **Packaging:** `.deb`, `.rpm`, `.tar.gz` bundles and every `py3-none-*` wheel carry `libblastit_php86-<arch>-<libc>.so` (coordinate with `MB.42.3`; do not duplicate the builder — just make sure the shim glob the builder uses is the same one the matrix declares).
8. **Docs statements:** every user-visible support sentence says `PHP 8.3, 8.4, 8.5 and 8.6` (`8.3–8.6`). Files that must be updated (grep for the old three-minor sentence): `docs/frameworks/php.md`, `docs/getting-started.md` (if it names minors), `docs/deploy/*`, `README.md`, `CHANGELOG.md`, `AGENTS.md`, `ARCHITECTURE.md`, `docs/design/backend-runtimes.md` if it states the set, and `scripts/platform_matrix.py`'s `--check` document scan must stay green. Do **not** touch `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` unless an amendment section is genuinely required — if it is, add a dated `## Amendment` paragraph rather than rewriting history.

# Notes
- No `libphp`/`libpython` `DT_NEEDED` change anywhere: 8.6 is another `dlopen` shim, exactly like 8.3/8.4/8.5 (`scripts/check-portable-elf.sh` must stay green).
- Container work only: build/probe inside `debian:bookworm-slim` and `alpine:edge`; **never** install PHP packages onto this host and never touch `/usr/bin/blastit` or `~/.local`.
- If PHP 8.6's headers break the C shim with `-Werror`, fixing the shim is in scope; weakening `-Werror` or adding `#pragma GCC diagnostic ignored` is not.

# Acceptance
- `make fast-check`
- `make php-sapi PHP_MINOR=8.6` produces `target/php-sapi/libblastit_php86-x86_64-gnu.so` in the `debian:bookworm-slim` container and the probe real-loads it
- `make rs-test-php` green (8.4/8.5 legs unchanged) and `make rs-test-php-image` green on the PHP 8.6 gnu leg
- `uv run --no-project scripts/platform_matrix.py --check` green (16 shims declared)
