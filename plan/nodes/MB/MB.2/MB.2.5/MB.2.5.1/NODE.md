---
id: MB.2.5.1
title: "Rewrite precedence and the static method/body gate (decide) - design MB.2.5a"
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MB.2.4.1]
paths:
  - "crates/blastit-scheduler/src/rewrite.rs"
  - "crates/blastit-scheduler/src/route.rs"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/tests/critical_rewrite.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T20:52"
updated: "2026-11-13T22:13"
---

# Goal

Implement the documented total rewrite precedence and the static method/body gate as one pure function, so `MB.2.5.2` only has to call it. This is the design's `MB.2.5a` (the precedence/status table's pure half).

# Context

- `docs/design/static-and-spa.md` §4.2 (`rewrite.rs`: `enum Rewrite { Serve(StaticFile), Forward, Reject { status, allow } }` and `pub fn decide(outcome: StaticOutcome, method: &[u8], has_body: bool, has_app: bool) -> Rewrite` — it **consumes** the outcome, and `Serve` carries the single `StaticFile` onward), §4.3 (all eleven rows of the total order; steps 1–2 stay where they are; refusals are terminal and a denied path is **never** masked by the SPA fallback), §4.4 (the method/body table: `405` + exactly `Allow: GET, HEAD`, `413` for a declared body, `has_body` = `Content-Length > 0` or any `Transfer-Encoding`, nothing read to decide), §5.2 rows 14 and 27–33 (forward vs fallback vs index), §7.2 (`R6`, `R8` → `critical_rewrite.rs`).
- New `crates/blastit-scheduler/src/rewrite.rs`, declared `pub mod rewrite;` in `lib.rs`. It uses `StaticOutcome`/`StaticFile`/`Refused` from `MB.2.4.1`'s `static_asset.rs`; this leaf therefore runs after it.
- `route.rs`: `RouteInfo` carries the route's `StaticPolicy` via `with_static(..)` (§4.2/§4.6); `MB.2.6` calls it from stage 1.
- Out of scope: `scheduler.rs` and every frame/pump change (`MB.2.5.2`); `path.rs` — one decode already happens there and `%2F` already decodes to `/` (`path.rs` line 131), which is exactly §4.3, so it must not change here; `static_asset.rs` and `config.rs` (`MB.2.4.1`).

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_rewrite --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --lib --locked
$ make rs-lint
$ make critical-guard BASE=develop NODE=MB.2.5.1
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
```

# Notes

- New protected suite `tests/critical_rewrite.rs`; this leaf lands its **pure half**: exact hit → `Serve(Exact)`, `/docs` and `/docs/` → `Serve(DirectoryIndex)` with the same file, file-before-fallback, SPA fallback → `Serve(SpaFallback)`, `SpaLost`/`Hidden`/`Refused` → `404`/`404`/`400` with no fallback and no forward, `Missing` + app → `Forward`, `Missing` + no app → `404`, `405` with exactly `Allow: GET, HEAD` for `POST`/`PUT`/`OPTIONS` and for `POST /dashboard` on a `spa = true` route, `413` for `GET`/`HEAD` with a declared body, `HEAD` still selecting the file. `MB.2.5.2` appends the flow cases to this same file (design §8 gives it to both leaves) and runs strictly after this one — never in parallel.
- Budget: ≤ 4 files, ≤ 600 changed lines; `rewrite.rs` ≤ 250 production lines. Build files from §5.2's fixture module where a real descriptor is needed (`resolve` then `decide`), never a hand-made `StaticFile`.
- If `decide` needs a status the §4.4 table does not name, stop and open a Follow-up instead of inventing one.
