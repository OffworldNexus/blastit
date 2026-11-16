# Log MB.21.4.9

## 2026-11-16T16:30 [planner]

created (task) Refuse an AllowEmptyOnly request from its head and never dispatch one before REQ_END: close the 413-vs-200 race behind the dev suite's flake

## 2026-11-16T16:30 [planner]

edited fields

## 2026-11-16T16:30 [planner]

why this leaf: 4.8 measured the residual 200-vs-413 as a product race, not a client defect — plan_request_body early-returns AllowEmptyOnly for a route with no [route.ingress] without reading Content-Length (scheduler.rs:1550-1602), (:1487) marks it body_ready and (:1507) dispatches it from the head, and blastit-view starts the WSGI app on REQ_HEAD (lib.rs:474/:501), so a later body byte's 413 (scheduler.rs:1901/:1907) races an app 200 already forwarded by view_head (:4154). Deterministic at a 1 ms head/body gap (5/5 wrong), invisible without load. Fix in two halves: 413 from the head when the head declares a non-empty body, and no view-bound AllowEmptyOnly request body_ready before REQ_END. Head_allocs' zero-copy allocation delta and one existing protected barrier that waits for the view to start on a body=none head must be updated without weakening an assertion; static/immediate-answer paths stay head-admitted.

## 2026-11-16T16:30 [lead]

status ready -> active

## 2026-11-16T16:30 [lead]

attempt 1/4 started

## 2026-11-16T16:30 [lead]

base eeccd9f
