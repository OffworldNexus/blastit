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

## 2026-11-15T12:18 [lead]

status ready -> active

## 2026-11-15T12:18 [lead]

attempt 2/3 started

## 2026-11-15T12:18 [lead]

rebased on 05ab3ea (MB.3.57 ported blastit-sys: all four targets check clean, x86_64-musl executes, aarch64 seccomp is CI-only under qemu). The full 4-leg matrix is now buildable — attempt 2 must build it, and must also fix the three MAJORs from the attempt-1 review (publish-pypi.yml:254's artifact path, :463's missing download-artifact pattern, and the ADR-0010/backend-runtimes 8/4/12 counts).

## 2026-11-15T14:55 [lead]

status active -> review

## 2026-11-15T14:58 [lead]

REVIEW FAIL (attempt 2): one BLOCKER, six MAJORs. Every one is in the class "authored and never run", and the systemic answer is in item (7) below.
BLOCKER `.github/workflows/publish-pypi.yml:454` — `verify: needs: [guard, wheels, sdist]` names `wheels`, which was split into `wheels_gnu`/`wheels_musl`. GitHub rejects the whole workflow, so the rehearsal and every publish path are dead. Fix to `[guard, wheels_gnu, wheels_musl, sdist]`.
MAJOR 1 `publish-pypi.yml:403-410` — `#` comment lines inside `docker run … \` continuations; the shell ends the command at `alpine:3.21`, so the `sh -c` proof runs on the RUNNER and `verify_musl` always fails (reviewer verified). Move the comment above the `docker run`.
MAJOR 2 `publish-pypi.yml:273-275,320-322,681-684` — all four `binaries_*` jobs upload `dist/SHA256SUMS`; `release` downloads with `merge-multiple: true`, so 3 of 4 checksums are silently overwritten. Use `SHA256SUMS.${{ matrix.target }}` and concatenate in `release`.
MAJOR 3 `.github/workflows/php-sapi.yml:118-124` — the suite half runs the Ubuntu-built (glibc) test binaries inside Alpine/musl legs, where they cannot start (`Makefile:558` even asserts glibc). Skip the suite half for `libc=musl` or build the suites inside the Alpine image.
MAJOR 4 `scripts/platform_matrix.py:272-281` — `CLAIM`/`PROVEN_CLAIM` require a BARE digit but every live count in ADR-0010 §4 / backend-runtimes is BACKTICKED, so `--check` matches 1 count repo-wide (`0 shims`, skipped) and the gate is vacuous on the documents it exists to guard. Accept optional backticks/`**`, and add the real ADR/design lines to a repo-level test so the gate cannot go vacuous again.
MAJOR 5 `docs/design/backend-runtimes.md:328,373,397,410,411` and `ADR-0010:47` still cite `scripts/platform-matrix.json`, `tools/check_platform_matrix.py` and `python/tests/critical/test_platform_matrix.py` — none exist, and §7.2 claims a protected critical test and a sha256 gate that were never written. Describe only `scripts/platform_matrix.py` / `make platform-matrix-check`.
MAJOR 6 `.github/workflows/wheels.yml:11-15` says aarch64 and musl are "declared and dropped with a written reason (the code does not compile there)" while the same file runs `wheels_musl` on `ubuntu-24.04-arm` and `LEGS` enables all four. Delete the clause; say the arm/musl legs are CI-only.
MINORs: RESULT cites `docs/getting-started.md:69` in a 67-line file and claims a `note` per stated count while zero notes are emitted; `scripts/build-standalone.sh:78-83` says `-static-libgcc` "drops the extra runtime dependency" but the shipped musl binary has `NEEDED libgcc_s.so.1` and the ADR correctly requires `apk add libgcc`; `platform_matrix.py:258-265` scans 6 hand-picked pages of 92 (ADR-0009:29's "up to 3 proven shims" is unchecked).
(7) SYSTEMIC REQUIREMENT — this is the third review finding workflow defects that a machine should have caught. Extend the job-graph auditor so it validates, at minimum: every `needs:` target names an existing job; every `${{ matrix.<k> }}` reference belongs to a job with a `strategy.matrix` carrying that key; every `download-artifact` name/pattern matches an `upload-artifact` name; every `paths`/artifact path a job consumes exists in what its `needs` produce; and no `#` line sits inside a line-continuation. Wire it into `make check` and prove it by re-introducing two of the defects above in a /tmp copy and watching it fail. Report the count of checks it performs.
SHIPPING (reviewer): No until the BLOCKER is fixed. Matrix and proven counts are honest; `make check` green.

## 2026-11-15T14:58 [lead]

status review -> ready

## 2026-11-15T14:58 [lead]

status ready -> active

## 2026-11-15T14:58 [lead]

attempt 3/3 started

## 2026-11-15T15:04 [lead]

status active -> review

## 2026-11-15T15:10 [lead]

REVIEW FAIL (attempt 3): one BLOCKER and two MAJORs remain, all specified precisely by the reviewer, all still in "authored, never run" code. Escalating to the expensive tier per the ladder (attempt 3 of 3 exhausted) rather than a fourth cheap patch.
BLOCKER `publish-pypi.yml:250,312` — `owed=$(… platform_matrix.py --leg shims --libc gnu --arch ${{ matrix.arch }} | grep -c "^shim=")`: that tuple has THREE minors, so `--leg` exits 2 ("--leg matched 3 legs … pass --arch") with EMPTY stdout, and `set -euo pipefail` kills the step (reviewer measured STEP_EXIT=1) → both `binaries_*` jobs fail → `verify_musl`/`testpypi`/`pypi`/`release` never run → a tag uploads NOTHING. Fix: add `--count shims [--libc] [--arch]` to `platform_matrix.py` (or `--matrix shims … | grep -o '"name"' | wc -l`) and use it in both steps; ALSO make the auditor refuse a `--leg shims` invocation that omits `--minor` — the auditor should have caught this.
MAJOR 1 `scripts/check_workflows.py:309-321` — the duplicate check compares upload `name:`, not the FILE names inside artifacts: re-introducing attempt-2's MAJOR 2 (all legs upload `dist/SHA256SUMS`, release merges with `merge-multiple`) still reports 207/0, so RESULT:33's claim is false. For every consumer with `merge-multiple: true`, refuse two producers whose literal `path:` basenames collide.
MAJOR 2 `scripts/check_workflows.py:262-269` — the `#`-in-continuation scan runs only over `jobs.*.steps`, never `runs.steps` of the composite action, so a `#` inside a `\` continuation in `.github/actions/build-wheel/action.yml` passes 207/0 while the header claims "…and 1 action". Run the same loop over action steps.
MINORs: five uploads lack `if-no-files-found: error` (an empty artifact warns and ships nothing); the auditor has no checked-in self-test — the /tmp negative proof runs in no gate, the same vacuity mode as attempt 2's MAJOR 4, so move it into a script `make check` runs; `check_workflows.py` fetches pyyaml at check time (`uv run --no-project --offline` fails with a cold cache), so `make check` becomes the one gate needing the network — pin it in the lock instead.

## 2026-11-15T15:10 [lead]

edited fields

## 2026-11-15T15:10 [lead]

attempt 4/4 started

## 2026-11-15T16:12 [lead]

status review -> review

## 2026-11-15T16:26 [lead]

status review -> done
