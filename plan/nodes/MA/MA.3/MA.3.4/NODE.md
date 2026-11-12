---
id: MA.3.4
title: Run Docker root + rootless e2e suites and fix e2e fallout
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MA.3.1]
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T19:16"
updated: "2026-11-12T19:16"
---

# Goal

Run the full Docker e2e matrix — root container scenario plus the rootless capability suite — against the `0.1.1` tree and fix any fallout inside `e2e/**` so `make e2e` is green.

# Context

- `make e2e` = `scripts/build-wheel.sh`, then `e2e/run.py` (root Docker scenarios) and `e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt`.
- **This is the only `MA.3` node allowed to run Docker**; nothing else may run a container concurrently, so `lead` must not dispatch it beside another Docker-consuming node.
- `MA.2` changed `dev`, `--learn`, `check`, ingress enforcement and the manifest schema; expect scenario drift in `e2e/scenario.py`, `e2e/grants.py`, `e2e/check_logs.py`, `e2e/rootless/`.
- Load skills `security-invariants`, `testing`.

# Acceptance

```console
$ make e2e
... root scenarios: PASS ... rootless capabilities: PASS ...
$ make check
$ git status --porcelain e2e/
```

- Both suites pass; paste the tail of each suite in `RESULT.md`.
- Fail-closed proof still holds: production `blastit` in the container refuses to boot without root/capabilities, and the rootless run proves the documented capability set is enough.
- Changed tests under `e2e/**` are strengthened or corrected, never weakened: no added `skip`, no widened assertion, no swallowed failure. Each edit is justified in `RESULT.md`.

# Notes

- Do not edit `crates/**`, `python/tests/**` or `docs/**` to force a pass. A real product bug is a follow-up node, logged with the failing output.
- Clean up containers/volumes afterwards (`docker ps -a` empty for the harness labels); never `git push` or `git tag`.
- Takes longer than 10 minutes; run once, un-interrupted, and keep the logs.
