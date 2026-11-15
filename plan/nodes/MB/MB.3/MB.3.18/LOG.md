# Log MB.3.18

## 2026-11-14T15:17 [lead]

created (task) Build and publish the Linux packaging matrix: x86_64/aarch64 x glibc/musl, wheels + standalone binaries + PHP shims

## 2026-11-14T15:17 [lead]

QUEUED, not started (owner 2026-11-14): Linux only; aarch64 and Alpine/musl in scope. Runs AFTER MB.3.12 (Cargo.toml/lock + shims) and MB.3.17 (removes a workspace member), and after MB.3.13 (boot selection rule). musl + embedded PHP is the riskiest leg (Alpine packaging, dlopen against musl-linked libphp.so, /proc guarantees) -- if it fails it is DROPPED WITH A WRITTEN REASON, reported up, not silently skipped and never shipped best-effort. 12 shims = 3 minors x 2 archs x 2 libcs, built NATIVELY, named with arch+libc+minor.

## 2026-11-14T15:19 [lead]

ACCEPTANCE TIGHTENED (owner 2026-11-14, 'drop musl if it cannot be made secure'): the musl leg's gate is SECURITY, not 'it runs'. Six items must hold on the Alpine leg -- secrets (stage-0 execve strip effective, worker /proc/<pid>/environ asserted), isolation (per-route uid + SeccompRole::View, filter NOT silently widened for musl: a needed syscall is justified everywhere or musl loses), egress (nothing beyond granted proxy sockets), CPU accounting (CLOCK_PROCESS_CPUTIME_ID + /proc reconciliation; degraded-only is a REPORTED fairness degradation, not a pass), provenance (signed Alpine libphp, dlopen without a compat shim, native musl build), lockdown (ffi.enable=0, phar.require_hash=1, memory_limit, protected extension_dir hold against Alpine INI defaults, asserted via ini_get). Any unfixable failure => DROP musl from matrix/CI/docs/install table with ONE line naming the failing item, and report it to the conductor as an owner-visible decision. A leg never attempted is a FINDING, not a pass. The same six items are recorded per glibc leg so musl is not held to a higher standard. Queue unchanged: MB.3.12 -> MB.3.17 -> MB.3.13 -> MB.3.18.

## 2026-11-14T15:30 [lead]

PROVEN-COUNT RULE (owner 2026-11-14): MB.3.12 built and probed 8.4 only; 8.2/8.3 are source-guarded but UNBUILT. This node's first job is first build + probe of 8.2 and 8.3 (Dockerfile takes PHP_MINOR); a minor that cannot be built AND probed is dropped from the matrix with a reason (the musl rule). Until then NO document may claim '12 shims' -- it is 4 proven (8.4 x 4 legs). ACTION ROUTED UP: the ADR-0010 SS4 distribution matrix row still implies 12; that sentence must become '4 proven (8.4 x 4 legs) until 8.2/8.3 are built and probed'. I did NOT edit docs/decisions/** myself: the architect owns that file and the last two-writer attempt on a design doc cost a 65 KB document. Route it to the architect (one line; no other change).

## 2026-11-15T11:33 [lead]

status ready -> active

## 2026-11-15T11:33 [lead]

attempt 1/3 started

## 2026-11-15T11:33 [lead]

base a86fd31

## 2026-11-15T11:55 [lead]

status active -> review

## 2026-11-15T11:56 [lead]

Owner ruling: the 2-wheel/1-archive/3-shim matrix is a report of a BLOCKER, not the release (aarch64 and Alpine/musl are in scope by explicit decision). MB.3.57 (priority 1) ports blastit-sys to aarch64-gnu and x86_64-musl; this node re-runs after it with the same rules (native build per leg, probe on target, needs_split over half-landing, 4-vs-12 proven counts, page counts refused by scripts/platform_matrix.py). Everything this node landed — the two building legs, the self-defending counts, the workflows — is kept and must not be redone.

## 2026-11-15T12:02 [lead]

REVIEW FAIL (attempt 1) — and it is the failure mode the "CI-only" label exists for: the authorised-but-never-run publish workflow has two bugs that would make the Release never complete.
MAJOR 1: `.github/workflows/publish-pypi.yml:254` — `/art/${{ matrix.name }}/blastit`, but the tarball root is `blastit-<target>` (`matrix.name` is `x86_64-gnu`), so the step always fails and `release` (needs `binaries`) never uploads archives/shims. Fix to `/art/blastit-${{ matrix.target }}/blastit`.
MAJOR 2: `publish-pypi.yml:463` — TestPyPI's `download-artifact` has no `pattern`, so a dispatch rehearsal pushes `shim-*.so` and the standalone `.tar.gz` to TestPyPI and races `shims`/`binaries` (absent from `needs`). Mirror the `pypi` job (`pattern: wheel-*`, then `pattern: sdist`).
MAJOR 3: `docs/decisions/ADR-0010:40,41,44,68` and `docs/design/backend-runtimes.md:250,251,255,353` still claim 8 wheels / 4 tarballs / "up to 12" / "4 proven", and ADR:68 names a JSON that does not exist; `scripts/platform_matrix.py:253-264` excludes them from `--check` although its docstring says internal claims are "reported" (nothing reports them) — so the RESULT's "no document may claim … `--check` will refuse it" is false. Both doc paths are now in this node's paths: correct the counts to what ships (and what will ship after MB.3.57), and make the docstring true (scan and REPORT internal docs) or drop the claim from it.
MINORs (all in the LOG entry of the reviewer's report, fix or grade): `publish-pypi.yml:476` `pypi` omits `binaries`/`shims` from `needs` while its own comment at :519 forbids exactly that half-landed publish; RESULT:347's negative-test commands are not reproducible ("3 proven shims" exits 0) — quote the real ones; `platform_matrix.py:289` labels declared-but-unbuilt wheels/archives "built & probed"; `:229` `shim_legs()` omits `curl` so the probe's fallback always wins; `scripts/build-standalone.sh:61` falls back to `target/release/blastit` even for an explicit cross `--target` (could package a host binary under a foreign name).
STATUS: this node stays OPEN and now also depends on MB.3.57 (the blastit-sys port), because its base will change and the matrix it must build is the 4-leg one. Re-run after .57 lands, with the same rules (native build per leg, probe on the target, needs_split over half-landing, proven counts refused by platform_matrix.py).
KEPT, do not redo: the two building legs' evidence, the self-defending counts, the workflow structure, and the `make check`/tag/notes gates.

## 2026-11-15T12:02 [lead]

edited fields

## 2026-11-15T12:02 [lead]

status review -> ready
