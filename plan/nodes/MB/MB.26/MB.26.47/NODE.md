---
id: MB.26.47
title: "docs-test's dev-learn fence exits 1 after SIGINT, twice in one session: make it deterministic"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths: ["python/tests/runnable_docs/**", docs/local.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-19T00:36"
---

# Goal

Make `docs/local.md`'s `dev-learn` console fence stop exiting 1 after `SIGINT` under `make
docs-test`.

Seen twice in one session on two different trees (MB.32.2's reviewer and MB.35.2's doc-writer),
always as `docs/local.md` fence 5 (`dev-learn`, "exited 1 after SIGINT"), always followed by a
green re-run. Two independent sightings of a green-on-retry is a race, not a host flake — and a
harness that can fail on a correct page either blunts the gate or teaches the next agent to
re-run until it passes.

1. Reproduce deterministically: run `make docs-test` in a loop (bounded, at least 5 runs) and
   capture the failing run's FULL output, including the child's exit status and any stderr it
   wrote before the signal.
2. Name the mechanism. Candidates to rule in or out with evidence: the harness sends `SIGINT`
   before the child has installed its handler; the child is `blastit dev` execve'd in place
   (LESSONS: the PID is stable but the argv is not), so the signal may land in a different
   process than the one the harness tracked; or the fence's expected output races the
   scheduler's first byte.
3. Fix it in the harness or in the page — never by loosening the fence's expected output, and
   never by adding a retry that hides a real failure.
4. Prove the fix: N consecutive green runs, and one mutation that makes the old path fail (the
   `left`/`right` of the failure, pasted).

# Context

- `python/tests/runnable_docs/` holds the harness; `docs/local.md` fence 5 is the page.
- `docs/knowledge/LESSONS.md` already records two `SIGINT`-adjacent traps (`blastit dev` execve,
  and the supervisor reading `ECONNRESET` as a lost peer); read them before theorising.
- This is not a test to delete: the fence is how a reader learns `--learn`.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ make docs-test
$ for i in 1 2 3 4 5; do timeout 600 make docs-test >"/tmp/opencode/dt-$i.log" 2>&1 or echo "run $i failed"; done
```

and in `RESULT.md`: the reproduced failure with its full output, the mechanism, the five-run
tail, and the mutation that proves the new guard.

# Notes

- If the race is in the shipped `blastit dev` signal handling rather than the harness, the fix
  belongs there and this node's `paths` grows to `crates/blastit/src/**` in the same change.

# Provenance

Re-filed from `MB.39` at the `v0.2.0` gate: runnable-docs `SIGINT` timing determinism on `docs/local.md` `dev-learn` fence.
