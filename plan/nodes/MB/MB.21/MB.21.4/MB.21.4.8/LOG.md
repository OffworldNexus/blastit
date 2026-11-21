# Log MB.21.4.8

## 2026-11-16T14:44 [planner]

created (task) Bound the dev test client's response pairing: parse a response against its own request, close after a body, and deadline every wait with a small diagnostic

## 2026-11-16T14:44 [planner]

why this leaf: the protected 413 assertion in dev_boots_five_unprivileged_roles_and_strips_secrets is flaky at gate rate (2 of the last 4 full make check runs here, plus one reviewer run: 200 read where the log says 413 unmatched; never in 21 targeted re-runs), so it would fail the release PR. Helper verified by grep to live in the suite itself (Dev::request, critical_dev_and_fail_closed.rs:442; only smoke.rs:315 has a twin), hence no tests/common/** glob. Scope is the test client only: framing (head + declared body), close after a body, and a bounded deadline with a small diagnostic; the 413 expectation is kept byte-for-byte. If the mechanism proves server-side the leaf reports file/line and opens the follow-up instead of weakening the test.

## 2026-11-16T16:09 [lead]

status ready -> active

## 2026-11-16T16:09 [lead]

attempt 1/3 started

## 2026-11-16T16:09 [lead]

base c228390; host note: this machine is at 92% memory used by another project, so two host-memory cases in the same file fail for reasons unrelated to this leaf — do not chase them, and report if they block your stress run.

## 2026-11-16T16:28 [lead]

evidence: the client defect is real and fixed (regression test fails on the old reader), 20/20 under load; the residual 200-vs-413 is a PRODUCT race, hypothesis 3: scheduler dispatches an AllowEmptyOnly request on body_ready before REQ_END (scheduler.rs:1487/1507) and view starts the app at REQ_HEAD (lib.rs:474/501), so a later body byte races the app's 200 — reproduced deterministically with a 1 ms head/body gap. That is a product correctness bug (R12), filed as MB.21.4.9; this leaf's own scope is met and its assertion is unchanged.

## 2026-11-16T16:28 [lead]

status active -> done
