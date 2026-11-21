# Log MB.18

## 2026-11-14T19:47 [conductor]

created (task) Restructure /deploy: architecture + topology model first, preset mirrors second, generic levers third

## 2026-11-14T19:48 [lead]

edited fields

## 2026-11-14T19:48 [lead]

PATH REPAIR by lead: the runnable-docs inventory registers fences by per-page INDEX (docs/deploy/index.md has fences 1-4, digitalocean 1-7, compose 1-2, anything-else 1-2), so moving fences during the restructure requires re-registering them there; added python/tests/runnable_docs/inventory.toml. Sequencing: after MB.3.13.1 settles; docs-only, must not run while anything else edits docs/deploy/**; do not touch docs/frameworks/** or ADRs.

## 2026-11-15T08:46 [lead]

status ready -> active

## 2026-11-15T08:46 [lead]

attempt 1/3 started

## 2026-11-15T08:46 [lead]

base cfe0185

## 2026-11-15T08:57 [lead]

edited fields

## 2026-11-15T08:57 [lead]

Lead: `e2e/docs_deploy.py` is now in this node's paths. Your finding is the right one and it is release-blocking: the restructure moved the timezone/set_config/dead_grant material out of `index.md`, where node item 4 forbids it, but the e2e scenario still asserts six zone-read promises on `index.md` — and it sits first in `e2e/run.py`, so `make e2e` aborts with `docs_deploy: 36/42` before Docker ever starts. Retarget `index_checks` at `anything-else.md#session-state` (your Follow-up 1 has the exact mapping), keep every assertion's teeth (the page must still prove the promises), and add at least one assertion that the index does NOT carry that material again — otherwise the next restructure silently recreates the duplication.
Then run the scenario you own: `uv run --no-project e2e/docs_deploy.py` (or whichever invocation exits 0) and paste its output, and confirm the ordering claim by showing the file's position in `e2e/run.py` is now satisfied. Report the count out of the total.

## 2026-11-15T08:57 [lead]

status active -> ready

## 2026-11-15T08:57 [lead]

status ready -> active

## 2026-11-15T08:57 [lead]

attempt 2/3 started

## 2026-11-15T09:00 [lead]

status active -> review

## 2026-11-15T09:06 [lead]

status review -> done
