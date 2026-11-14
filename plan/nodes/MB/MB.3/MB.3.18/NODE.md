---
id: MB.3.18
title: "Build and publish the Linux packaging matrix: x86_64/aarch64 x glibc/musl, wheels + standalone binaries + PHP shims"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.3.13, MB.3.17]
paths: [".github/workflows/**", "scripts/**", "e2e/**", Cargo.toml, Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T15:17"
updated: "2026-11-14T15:17"
---

# Goal

Build and publish the Linux packaging matrix the owner settled on 2026-11-14:
**x86_64/aarch64 × glibc/musl**, wheels + standalone binaries + PHP shims, with the boot
selection rule proven and the SAPI probe run on every leg.

**Scope:** Linux only. `aarch64` in scope. Alpine/musl in scope.

**Not started on purpose.** This node touches `.github/workflows/**` and `Cargo.toml` and
would collide with `MB.3.12` (shims, `Cargo.toml`/`Cargo.lock`) and `MB.3.17` (removes a
workspace member). It runs **after** both; do not open it earlier.

# Context

- Distribution table of record: `ADR-0010` §4 (standalone archive ships `php` with `python`
  off, zero `libpython`; the wheel ships `python, php`), and `ADR-0009` for the
  capability-gated Python story the Python-free build depends on.
- `MB.3.11` made the Python-free build real (`rs-no-python`, `0 pyo3 dependencies`), so the
  standalone artifact has something to build. `MB.3.12` produces the shims and the probe;
  `MB.3.13` owns the *boot* selection rule. This node owns the *packaging* of those
  artifacts, not their design.
- **musl + embedded PHP is the riskiest leg** and is owner-visible: Alpine packaging
  (`libphp-embed` availability, extension ABI), `dlopen` behaviour against a musl-linked
  `libphp.so`, `/proc` guarantees under Alpine's defaults, and shim linkage are all less
  exercised than glibc. **If musl cannot be made to work, it is dropped with a written
  reason recorded in this node and reported up** — never silently skipped, never shipped
  "best effort".

# Acceptance

1. **Wheels** — CPython **3.13 + 3.14** × `manylinux_2_28` (x86_64, aarch64) and
   `musllinux` (x86_64, aarch64): 8 wheel artifacts, each installed and smoke-tested in its
   own container (import, `--version`, one static route served).
2. **Standalone binaries** — `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
   `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, each built with
   `--no-default-features` (Python-free), shipped with a checksum file, and each proven to
   serve a static route and to refuse a WSGI manifest at exit 1.
3. **PHP shims** — the matrix target is 3 minors (8.2/8.3/8.4) × 2 archs × 2 libcs, each
   file named with arch + libc + minor (e.g. `libblastit_php84-aarch64-musl.so`), built
   **natively** on the target (arm runners, Alpine containers) — never cross-compiled
   against a foreign `php_config.h`.
   **Proven-count rule:** `MB.3.12` built and probed **8.4 only**; 8.2/8.3 are
   source-guarded but unbuilt. So the first job here is **first build + probe of 8.2 and
   8.3** (the Dockerfile already takes `PHP_MINOR`). Until a minor has been built *and*
   probed on a leg, **no document may claim "12 shims"** — it is **4 proven** (8.4 × 4
   legs) and the rest unproven. A minor that cannot be built and probed is **dropped from
   the matrix with a reason**, exactly the musl rule, and the count in the docs/install
   table drops with it.
4. **Boot selection rule implemented and tested** (with `MB.3.13`): pick the shim by host PHP
   minor + arch + libc; no match ⇒ fail closed naming the host combination and
   `BLASTIT_LIBPHP`. Each of the 12 combinations has a test; a mismatch row and an
   accepted row per combination.
5. **The SAPI probe runs on every platform leg** — arm runner or qemu for aarch64, Alpine
   container for musl — asserting the `MB.3.12` probe's seven rows on each. A shim that
   only compiles is unproven and does not count.
5b. **Every leg also passes the security checklist below**, evidenced per leg (glibc **and**
   musl — musl is not held to a higher standard than the leg we already trust).
6. **Publish pipeline updated**: a tag publishes everything — PyPI for the wheels, GitHub
   Releases for the binaries + shims with checksums — and `scripts/check-release-tag.sh`
   plus `scripts/release-notes.sh --require-dated` still pass.

## Musl gate: security, not "it runs"

Owner confirmation 2026-11-14: **"drop musl if it cannot be made secure"**. Musl ships only
if **all six** hold on the Alpine leg. Any failure that cannot be fixed ⇒ **drop musl**
(remove it from the matrix, CI, docs and the install table) with the failing item recorded
in **one line**, e.g. *"musl dropped: seccomp filter refuses syscall X required by Alpine's
libphp; adding it would weaken glibc"*. A failed musl leg is **reported to the conductor as
an owner-visible decision**, not absorbed into a green run.

1. **Secrets.** Stage-0 `execve` strip and `ALWAYS_STRIP` are effective; nothing reaches the
   PHP worker's environment. Assert by reading the worker's `/proc/<pid>/environ`
   in the Alpine container, not by trusting the config.
2. **Isolation.** Per-route uid and `SeccompRole::View` are installed, and the filter is
   **not silently widened for musl**: a syscall musl needs is either justified for every
   platform (and added everywhere, with a test) or musl loses the leg.
3. **Egress.** No network beyond the granted proxy sockets (`AF_INET`/`AF_INET6` refused),
   asserted from inside the worker.
4. **CPU accounting real.** `CLOCK_PROCESS_CPUTIME_ID` reports, and `/proc` reconciliation
   is available. If only the degraded path works, that is a **documented fairness
   degradation reported up** — never quietly counted as a pass.
5. **Provenance.** `libphp` comes from Alpine's **signed** package, `dlopen` resolves
   without a compatibility shim, and the shim is built natively against musl headers.
6. **Lockdown effective.** `ffi.enable=0`, `phar.require_hash=1`, `memory_limit`, and a
   protected `extension_dir` hold **against Alpine's own INI defaults** — assert the
   effective values via PHP reflection/`ini_get`, not the generated file.

**A leg never attempted is a finding, not a pass** — every skipped platform is listed in the
RESULT with its reason. Record the same six items per glibc leg, so the comparison is honest.

# Notes

- Keep the matrix in one place (a workflow matrix or `scripts/` table), not copied per job;
  a leg added once must appear everywhere.
- The container/e2e jobs stay out of `make check` (Docker + PHP images), same rule as
  `e2e/`.
- Record every skipped leg with its reason in the RESULT — "not attempted" is a finding,
  not a pass.
