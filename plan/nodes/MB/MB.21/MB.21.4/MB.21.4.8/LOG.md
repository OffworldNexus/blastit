# Log MB.21.4.8

## 2026-11-16T14:44 [planner]

created (task) Bound the dev test client's response pairing: parse a response against its own request, close after a body, and deadline every wait with a small diagnostic

## 2026-11-16T14:44 [planner]

why this leaf: the protected 413 assertion in dev_boots_five_unprivileged_roles_and_strips_secrets is flaky at gate rate (2 of the last 4 full make check runs here, plus one reviewer run: 200 read where the log says 413 unmatched; never in 21 targeted re-runs), so it would fail the release PR. Helper verified by grep to live in the suite itself (Dev::request, critical_dev_and_fail_closed.rs:442; only smoke.rs:315 has a twin), hence no tests/common/** glob. Scope is the test client only: framing (head + declared body), close after a body, and a bounded deadline with a small diagnostic; the 413 expectation is kept byte-for-byte. If the mechanism proves server-side the leaf reports file/line and opens the follow-up instead of weakening the test.
