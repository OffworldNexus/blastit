---
id: MA.10
title: "Leaf 17: run only root-valid suites in smoke (root)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [.github/workflows/ci.yml]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-13T12:15"
updated: "2026-11-13T12:34"
---

# Goal

Make `smoke (root)` pass by running the correct set of test binaries as root: the privileged-path suites, not the unprivileged-only one.

# Context

MA.9 made the job bounded and self-diagnosing. It now proves the cause in CI (run for `dc553ad`, job 131849341219):

```text
==> root: .../smoke-…            test result: ok. 31 passed
==> root: .../critical_config_and_presets-…   ok. 11 passed
==> root: .../critical_check_engine-…         ok. 16 passed
==> root: .../critical_dev_and_fail_closed-…  test unprivileged_production_has_no_single_uid_fallback ...
::error::… did not finish as root within 90s
```

`crates/blastit/tests/critical_dev_and_fail_closed.rs:1257` spawns the production binary and calls `.output()`; its own comment says: *"`R17`: production fails closed. The suite runs unprivileged, so the capability detection is what refuses."* As **root**, capability detection does not refuse — production boots and serves, so `.output()` waits forever. The test is correct; running it as root is an invalid input to it, and it cannot be fixed by passing `SUDO_UID` without editing a protected critical test.

That suite is already executed as the ordinary unprivileged user by `make test-critical` and by the `test` / `pytest plugin` jobs, so excluding it from the root phase loses no coverage — but the exclusion must be **explicit, commented and printed**, never silent.

# Acceptance

1. The root phase runs exactly the binaries whose assertions are valid as root: `blastit` lib, `blastit` bin, `smoke`, `critical_config_and_presets`, `critical_check_engine` (verify the actual set from cargo's JSON and state it). `critical_dev_and_fail_closed` is excluded with:
   - a workflow comment naming the reason (production does not refuse as root, so the test blocks by design);
   - a visible line in the job log saying it is excluded and which job covers it as unprivileged;
   - a guard that fails the job if the exclusion list ever silently grows (e.g. assert the excluded set is exactly `{critical_dev_and_fail_closed}`).
2. Every root binary keeps its per-binary `timeout` and `--nocapture`, and the preflight from MA.9 stays.
3. `make check` exit 0, `plan.py validate` ok, `bash -n` on the run scripts, diff limited to `.github/workflows/ci.yml`.
4. State precisely what the next CI run must show for this job to be green (per-binary `test result: ok`, no `::error::`, job success under 20 min).
5. If you find evidence that `critical_dev_and_fail_closed` *can* legitimately run as root (e.g. a documented `needs_root` inverse marker), prefer that instead — but only with proof, and do not edit the protected test.

# Notes

- Do not drop the root phase, do not add `continue-on-error`, do not widen the exclusion beyond the proven case, and do not edit `crates/*/tests/critical_*.rs`.
- The whole point of this job is proving the privileged path boots under root; keep that intact.
- Do not push; the conductor pushes and watches the run.
