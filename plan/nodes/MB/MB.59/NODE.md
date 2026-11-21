---
id: MB.59
title: "Drop '/ byte-range' from TESTING_PLAN.md §2 per owner H-017 decision"
kind: task
status: done
owner: writer
priority: 1
depends_on: []
paths: [TESTING_PLAN.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-21T11:31"
updated: "2026-11-21T11:32"
---

# Goal
Apply owner `H-017` sign-off on `TESTING_PLAN.md` §2 (line 16): replace `conditional \`ETag\` / byte-range requests` with `conditional \`ETag\` requests` (Blastit's static responder answers `ETag`/`304` and full `200`). Change nothing else.

# Acceptance
- `grep -c "byte-range" TESTING_PLAN.md` outputs `0`
