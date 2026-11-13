---
id: MA.3.7
title: "Product fixes: dev watermark boot contract, queries=[] denying listener, one-pass dev --learn"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit/src/**"
  - "crates/blastit-dbproxy/src/**"
  - "crates/blastit/tests/critical_*.rs"
  - "crates/blastit-dbproxy/tests/critical_*.rs"
  - docs/knowledge/LESSONS.md
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-12T23:41"
updated: "2026-11-13T00:50"
---

# Goal

Close the three product gaps the runnable-docs harness proved against the documented `0.1.1` contract: (a) `blastit dev` must either serve its first view or fail at boot naming `BLASTIT_MEM_HIGH_WATERMARK` (today a loaded host answers every request with `503 queue_timeout` after 10 s and says nothing); (b) a route with `queries = []` must get the denying listener the docs promise, instead of a `DATABASE_URL` whose socket does not exist; (c) a single `blastit dev --learn` pass must attribute the queries of a newly discovered route, as `blastit check --learn` already does (today it adds the route and zero queries, so the documented one-pass flow is a lie).

# Context

- Found by `MA.3.1`'s runnable-docs harness and confirmed by its reviewer (`plan/nodes/MA/MA.3/MA.3.1/RESULT.md` `## Drift found`, `docs/testing/AUDIT-MA.2.3.md`).
- Normative docs: `docs/local.md` (local workflow, `queries = []` denying listener, `dev --learn`), `README.md` Getting Started, `docs/ci.md`; design `docs/design/ma-lean-dx.md` §1.1/§1.2 and R7/R9/R26/R29; `ADR-0003`, `ADR-0004`.
- Reproductions: `BLASTIT_MEM_HIGH_WATERMARK` unset on a host above the 85 % default -> the README `curl` gets `503 queue_timeout` after 10 s; a page manifest with `queries = []` -> the view's `DATABASE_URL` points at a socket that does not exist (uncaught -> 500 instead of the promised `42501`); `dev --learn` -> `learn: 1 routes, 0 queries added` while `check --learn` adds them and a second `dev --learn` pass adds them.
- Skills `rust-standards`, `security-invariants`, `testing`, `dx-first`.

# Acceptance

```console
$ cargo test -p blastit --locked
$ cargo test -p blastit-dbproxy --locked
$ make check
$ make docs-test
```

- (a) On a host above the 85 % watermark, `blastit dev` either serves the first request or exits non-zero with a message naming `BLASTIT_MEM_HIGH_WATERMARK` and the current usage; a protected test pins both arms (host below the watermark: serves; forced high: the named failure, never a silent 503).
- (b) `queries = []` yields a reachable denying listener: a protected test proves an ungranted statement gets `42501` (and a granted one still passes) with the socket present.
- (c) One `blastit dev --learn` pass over both routes writes each route's exact queries; a protected test proves route two's query appears without a second pass, and that a literal-bearing statement is still refused.
- `make docs-test` goes green with **no product knobs injected** by the harness (this node does not edit the harness; the gate must pass because the product serves).
- No `sudo`, no relaxed assertion, no skip.

# Notes

- If this exceeds ~600 changed lines or needs the harness changed, report `STATUS: needs_split` with the proposed children instead of sprawling.
- Do not edit `docs/`, `README.md`, `Makefile`, or `python/tests/runnable_docs/**` (MA.3.1 owns the harness; MA.3.2 owns the prose).
