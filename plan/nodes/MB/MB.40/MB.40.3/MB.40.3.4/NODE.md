---
id: MB.40.3.4
title: "RequestIo trait -> concrete ChanIo transport (converge the ADR-0016 exemption)"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.40.3.3]
paths:
  - "crates/blastit-view-php/src/worker.rs"
  - "crates/blastit-view-php/src/lib.rs"
  - crates/blastit/tests/critical_artifacts.rs
  - Cargo.toml
  - Cargo.lock
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T01:47"
updated: "2026-11-19T01:47"
---

# Goal
Retire the last recorded deviation from `R3`: delete the single-implementation `RequestIo` trait from `crates/blastit-view-php/src/worker.rs` in favour of the concrete `ChanIo` request transport, drive its six in-file scripted `FakeIo` cases with real `UnixStream::pair()` + `view.chan` frames, and converge `crates/blastit/tests/critical_artifacts.rs`'s ADR-0016 exemption to the tree by removing **only** the now-obsolete `"pub trait RequestIo"` entry from its `required` list.

# Context
`MB.40.3` item 4c. The audit found `RequestIo` has zero external implementers and six `FakeIo` cases inside `worker.rs`, i.e. the trait exists only to be faked — exactly the mock-ceremony seam the harness forbids. `RequestEnd` and `flush_response` stay (pinned by the same protected test).

`crates/blastit/tests/critical_artifacts.rs::the_deleted_transport_trait_stays_deleted` currently requires `"pub trait RequestIo"` to still exist, as a *recorded deviation* from `ADR-0016` decision 2 (whose title is "The worker trait is deleted, not re-homed"). Deleting the trait makes the tree agree with the ADR, so the exemption entry is stale and must go — this is a **tightening** of the guard, never a weakening.

# Exact Targets (`file:line` pre-diagnosed)
- `crates/blastit-view-php/src/worker.rs:929` (`pub trait RequestIo`), `:1029` (`struct ChanIo<'a>`), `:1195` (`impl RequestIo for ChanIo`), served through `:416` `PhpEmbedRuntime::serve(&self, io: &mut dyn RequestIo)`, `:529` `refuse`, `:757` `read_body`, `:811` `serve_forever(mut serve: impl FnMut(&mut dyn RequestIo) -> RequestEnd)`.
- In-file scripted fakes `:2290-2311` (`struct FakeIo` + `impl RequestIo for FakeIo`) used by the cases at `:2380`, `:2384`, `:2515`, `:2403`, `:2435`, `:1857`.
- Re-export of the trait at `crates/blastit-view-php/src/lib.rs:68`.
- Protected test `crates/blastit/tests/critical_artifacts.rs`: the `required` array entry and the module-doc paragraphs describing the `RequestIo` part of the deviation.

# Authorization (protected critical test — exactly this)
In `crates/blastit/tests/critical_artifacts.rs` you may change **only**:
1. delete the single `"pub trait RequestIo",` element from the `required` array of `the_deleted_transport_trait_stays_deleted`, and
2. adjust the module-doc sentences that describe `RequestIo` as part of the standing deviation so they state that it is now deleted and only `RequestEnd`/`flush_response` remain exempt (keep the `MB.36.5`/`MB.36.11` deviation history intact).

Every other assertion must stay: `RequestEnd`, `flush_response`, the `WorkerRuntime`/`flush_request` refusal over all of `crates/`, the planted-path refusal proof, and the count/name of every test. Never add `#[ignore]`, never delete or rename a test, never relax a scan rule. `uv run --no-project tools/critical_guard.py --base e6b1291 --node MB.40.3.4` must pass.

# Acceptance
- `make fast-check`
- `cargo test -p blastit-view-php --locked`
- `cargo test -p blastit --locked --test critical_artifacts`
- `uv run --no-project tools/critical_guard.py --base e6b1291 --node MB.40.3.4`
- Evidence in RESULT.md: `grep -rn "FakeIo\|dyn RequestIo\|pub trait RequestIo" crates/` returns nothing; every protected test name and count unchanged.

# Notes
Single Cargo writer. No new dependency. `ChanIo` stays the only implementation and must remain the type `serve`/`serve_forever` take.
