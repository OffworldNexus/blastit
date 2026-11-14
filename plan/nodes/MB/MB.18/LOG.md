# Log MB.18

## 2026-11-14T19:47 [conductor]

created (task) Restructure /deploy: architecture + topology model first, preset mirrors second, generic levers third

## 2026-11-14T19:48 [lead]

edited fields

## 2026-11-14T19:48 [lead]

PATH REPAIR by lead: the runnable-docs inventory registers fences by per-page INDEX (docs/deploy/index.md has fences 1-4, digitalocean 1-7, compose 1-2, anything-else 1-2), so moving fences during the restructure requires re-registering them there; added python/tests/runnable_docs/inventory.toml. Sequencing: after MB.3.13.1 settles; docs-only, must not run while anything else edits docs/deploy/**; do not touch docs/frameworks/** or ADRs.
