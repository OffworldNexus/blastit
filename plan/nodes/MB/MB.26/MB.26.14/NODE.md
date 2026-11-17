---
id: MB.26.14
title: "Locate an endpoint-level body raw warning (sites.rs raw_body's endpoint chain is untested)"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-manifest/tests/warning_sites.rs", "crates/blastit-manifest/src/blastfile/sites.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T07:23"
updated: "2026-11-17T07:23"
---

## Goal

Give `sites.rs::raw_body`'s endpoint branch a fixture, so an endpoint-level `body raw` squiggles
its own codec instead of `(0,0)`.

## Context

Moved from `MB.22.12` at the `0.2.0` release gate: post-0.2.0 work, re-homed from the `MB.22` epic.

`AUDIT-MB.22.md` §3 survivor 2 (measured 2026-11-17). `raw_body` looks at a view's own `body`
node and then at each endpoint's `EndpointBlock` → `body` (§10.3). Replacing the endpoint chain
with `std::iter::empty()` leaves the whole suite green: no fixture writes

```
view v "/x" {
    GET "/y" {
        body raw
    }
}
```

so the branch is unproven. `validate` emits `RawIngress` for it (the view is the route), and the
editor would draw that warning at `(0,0)`.

## Acceptance

```console
$ cargo test -p blastit-manifest --test warning_sites --locked
```

- One fixture with an endpoint-level `body raw` asserts the warning's site is the `raw` token's
  line and column (not `Position::NONE`), and the same fixture with a view-level `body raw`
  keeps its existing site.
- Re-applying the `std::iter::empty()` mutation to the endpoint chain turns that test red.
