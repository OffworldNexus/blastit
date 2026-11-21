---
id: MB.7
title: Exclude critical_static_cli from the root CI phase (root can list anything)
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [.github/workflows/ci.yml]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T01:02"
updated: "2026-11-14T01:06"
---

# Goal

Green the release CI again: `smoke (root)` fails on `critical_static_cli::a_root_the_caller_cannot_list_is_refused`, a test whose premise cannot exist as root.

# Context

PR #64 (release/0.2.0) run: job `smoke (root)` fails with

```text
thread 'a_root_the_caller_cannot_list_is_refused' panicked at crates/blastit/tests/critical_static_cli.rs:542:5
test result: FAILED. 11 passed; 1 failed
```

The test asserts that a static `root` the caller **cannot list** is refused. As uid 0 every directory is listable, so the condition is unconstructible — the failure is the test being run in the wrong privilege regime, exactly like `critical_dev_and_fail_closed` (MA.10). It already runs unprivileged in `make test-critical` and the `test` job.

MB.10's mechanism is in place and is the right one: the build step excludes a named binary from the root phase, prints the exclusion with its reason and where it is covered, and asserts the exclusion set is exactly the known list.

# Acceptance

1. `critical_static_cli` is excluded from the root phase, with:
   - the exclusion list grown to exactly `{critical_dev_and_fail_closed, critical_static_cli}` and the guard updated to that set (it must still fail if the set grows further or a suite vanishes);
   - a workflow comment naming the reason ("the assertion is about an unprivileged caller; as root the precondition cannot exist");
   - a visible log line saying it is excluded and that `make test-critical` / the `test` job cover it unprivileged.
2. No test is skipped, weakened or edited; `.github/workflows/ci.yml` only.
3. `make check` exit 0, `plan.py validate` ok, `bash -n` on the run scripts.
4. State what the next `smoke (root)` run must show: 5+ root binaries each `test result: ok`, `critical_static_cli` named as excluded, no `::error::`.

# Notes

- If instead the suite can be made root-meaningful by constructing the precondition differently (e.g. a directory root with mode 0000 plus a non-root helper), that would be better — but it needs a protected-test edit, so propose it as a separate node rather than doing it here.
- The root phase exists to prove the privileged path boots; it must not become a dumping ground for tests. Each exclusion needs its stated reason.
