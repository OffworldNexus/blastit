---
id: MB.36.7
title: "Prove MB.36 end to end: wheel smoke, Docker + rootless e2e, the DX-invariance replay, relocated critical suites, and a per-crate mutation audit (test-curator)"
kind: task
status: ready
owner: "test-curator"
priority: 2
depends_on: [MB.36.6]
paths:
  - "docs/testing/AUDIT-MB.36.md"
  - "e2e/**"
  - "python/tests/**"
  - "crates/blastit/tests/**"
  - "crates/blastit-view/tests/**"
  - "crates/blastit-view-python/tests/**"
  - "crates/blastit-view-php/tests/**"
  - "crates/blastit-view-static/tests/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T14:07"
updated: "2026-11-17T14:07"
---

# Goal

Prove the split end to end and audit it, rather than trusting a green
`cargo test -p …`: build and smoke the wheel, run the Docker + rootless e2e harness, replay
every documented command the refactor promised not to change, and mutation-audit the moved
crates.

# Context

- Design of record: `MB.36.1`'s **DX-invariance list** and the critical-test relocation table;
  `ADR-0016`.
- What nothing has proved yet: the four view crates under the **wheel** (`python, php,
  static`) and the **standalone** archive (`php, static`) in containers, the static/PHP/WSGI
  e2e scenarios after the moves, and whether the relocated protected suites still fail when
  the code they guard breaks.
- The audit is a `test-curator` deliverable: `docs/testing/AUDIT-MB.36.md` (tier table, moved
  cases, survivors, verdict), plus new tests **only** where a real gap is found.
- Replay list (must appear verbatim in the audit, with the observed output):
  `blastit dev` on a static-only manifest, `blastit dev` on `[route.php] entry`, `blastit dev`
  on a WSGI `app`, `blastit dev --no-reload`, `blastit check`, and the SQLAlchemy/PHP e2e
  scenarios. `e2e/run.py` drives the container path; `make docs-test` drives the documented
  commands.

# Acceptance

```console
$ scripts/build-wheel.sh --manylinux 2_28
$ E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py
$ uv run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt
$ make critical-guard BASE=develop NODE=MB.36.7
$ make rs-test-critical
$ make docs-check && make docs-test
$ PKG=blastit-view make mutants-rust
$ PKG=blastit-view-static make mutants-rust
$ PKG=blastit-view-php make mutants-rust
$ test -f docs/testing/AUDIT-MB.36.md && echo "audit written"
$ uv run --no-project tools/plan.py validate
```
- Every case that was protected before the epic still exists and still fails when its
  invariant is broken: the audit lists each source→destination pair with the suite that now
  runs it, and names any survivor it judged untestable with the reason.
- The wheel smoke proves the wheel's ABI, `blastit-dbproxy`, one static route and one WSGI
  route; the standalone leg proves the archive boots PHP with no interpreter.
- The Docker and rootless e2e legs pass on the **refactored** tree, not on a cached prebuilt
  image (`E2E_DOCKERFILE=Dockerfile.prebuilt` against the wheel just built).
- `RESULT.md` pastes: wheel path + `sha256sum`, the three e2e tails, the mutants summaries,
  `make docs-test`, and the audit's verdict line.

# Notes

- `make e2e` is the same sequence (`scripts/build-wheel.sh` + `e2e/run.py` +
  `e2e/rootless/run.py`); run it that way if the wheel is already current, and say which
  commands actually ran.
- Mutation campaigns are scoped per crate (`PKG=`) and can be heavy — run them one at a time,
  never beside another build.
- A survivor in moved code is an audit finding, not automatically a new node: file it under
  `## Follow-ups` unless it sits on a security invariant (then it blocks the epic gate).
- Do not edit a protected test to make a mutation survive; that is the exact failure the tier
  exists to prevent.
