# MB.40.3.4 — `RequestIo` trait → concrete `ChanIo` transport

## Result

`pub trait RequestIo` is **deleted** from
`crates/blastit-view-php/src/worker.rs` (73 lines of interface with zero
implementers, six scripted fakes and no second user). The request transport is now
the concrete `ChanIo<'a>`:

* `PhpEmbedRuntime::serve(&self, io: &mut ChanIo<'_>) -> RequestEnd`;
* `fn refuse(io: &mut ChanIo<'_>, sentence: &str) -> RequestEnd`;
* `fn read_body(io: &mut ChanIo<'_>, max: usize) -> Result<Vec<u8>, BodyError>`;
* `fn serve_loop(chan, limits, serve: impl FnMut(&mut ChanIo<'_>) -> RequestEnd, recycle)`
  — the closure argument is a *lifetime* parameter, not a trait object;
* `struct PhpSink<'a, 'io> { io: &'a mut ChanIo<'io>, .. }`.

`ChanIo` and `PhpEmbedRuntime::serve` are **module-private** (`struct
ChanIo<'a>`, `fn serve`): the loop in `serve_forever` is their only caller and
there is no external constructor, so neither is a published-but-uncallable item.
`ChanIo` stays an opaque handle — every field private, its methods (`head`,
`read_body`, `write_head`, `write_body`, `output`, `flush_response`) private
inherent methods — and the frames, the credit windows and the one `Writer` are
the loop's to hand out, so a `ChanIo` cannot be fabricated for a stream that is
not a `view.chan`.

Kept, as pinned by the protected test: `RequestEnd` (`pub struct`),
`flush_response` (`fn flush_response`, now an inherent method with the same
contract prose). No new trait was introduced; no dependency, no `Cargo.toml`
change.

`crates/blastit-view-php/src/lib.rs`: `RequestIo` dropped from the re-export
(`PhpEmbedRuntime, PhpError, PhpRuntimeConfig, RequestEnd, serve_forever`) and
the module-layout bullet now names the concrete transport.

## The six in-file cases

Two were genuinely scripted (`FakeIo`); four already drove `UnixStream::pair()`
through `serve_loop` and only had to be retyped from `&mut dyn RequestIo` to
`&mut ChanIo<'_>`. Every assertion each case had is still asserted.

| # | Case | Before | Now |
|---|---|---|---|
| 1 | `a_socketpair_request_is_answered_and_closed_in_order` | real socketpair, `Echo: StandIn` | real socketpair, `Echo` serves through `ChanIo`; same `window, head, body, end, done` order, `Window{bytes: 5}`, `RespBody(b"hello")`, `RespEndCost{cpu_ns: Some(7)}`, `Done(Completed)`, request id on every frame |
| 2 | `a_large_response_waits_for_window_frames_instead_of_deadlocking` | real socketpair, `Big: StandIn` | unchanged driver; `Big` writes through `ChanIo`, so the body > `INITIAL_WINDOW` is still completed by reading the peer's `WINDOW` frames (credit window assertions intact) |
| 3 | `a_request_body_past_the_worker_budget_is_refused` | **`FakeIo` with two scripted 64-byte chunks**, `body.len() == 128` | **rewritten**: a real `UnixStream::pair()` carries two `REQ_BODY` (64 × `0x41`, 64 × `0x42`) frames and `REQ_END` into a real `ChanIo`; asserts the 128 bytes arrive in order for budget 1024 and that the same frames trip `BodyError::TooLarge` at budget 127 |
| 4 | `a_header_burst_past_the_cap_aborts_the_response` | **`FakeIo`**, `PhpSink` over it | **rewritten**: a real `ChanIo` over a socketpair (the burst aborts before the end-of-head marker, so no frame is written); same `MAX_HEADERS` accepted, `MAX_HEADERS + 1` refused, single `MAX_HEADER_BYTES + 1` header refused, `abort` set, `headers` empty |
| 5 | `a_runtime_that_retires_ends_the_loop_after_its_done` | real socketpair, `Once: StandIn` | unchanged driver, `Once` retyped; `Ok(())` after the first request's `DONE` |
| 6 | `a_response_body_before_the_head_is_a_protocol_error` | real socketpair, `BodyFirst: StandIn` | unchanged driver, `BodyFirst` retyped; the exact refusal sentence `ViewError::Protocol("RESP_BODY before RESP_HEAD")` and the "peer is gone" proof stay |

`a_request_body_past_the_flow_window_is_a_protocol_error` already built a real
`ChanIo` over a socketpair and is untouched. Test-only scaffolding added for the
two rewrites:

* `Direct` — a real socketpair plus the process-wide `Writer`/`Decoder` with a
  private `io(id)` that hands out one request's `ChanIo`;
* `send_frames(chan, limits, id, Vec<Frame>)` — encodes real `view.chan` frames
  onto the scheduler's end (the same `Limits::encode` the `Peer` helper uses).

`FakeIo` and its `impl RequestIo for FakeIo` are gone from every file: no
`Mock*`/`Fake*` type remains in `crates/blastit-view-php/`. The surviving
`StandIn` is the test module's own two-call seam for `serve_loop` (it has four
implementations, cannot leave `#[cfg(test)]`, and existed before this node); it
is not a production abstraction and nothing on the wire names it.

## ADR-0016 exemption convergence (protected test)

In `crates/blastit/tests/critical_artifacts.rs`, exactly two things changed:

1. `"pub trait RequestIo",` removed from the `required` array of
   `assert_transport_layout` (the helper `the_deleted_transport_trait_stays_deleted`
   calls). `"pub struct RequestEnd"` and `"fn flush_response"` are still
   required, side by side with the untouched
   `text.contains("runtime: PhpEmbedRuntime")` pin.
2. The `//!` module doc (R3 bullet + the exemption section) and the
   `PHP_SOLE_IMPLEMENTER` doc comment now state the convergence: `RequestIo` is
   deleted (`MB.40.3.4`), the worker's transport is the concrete `ChanIo`, and
   only `RequestEnd`/`flush_response` remain exempt. The `MB.36.5` move and the
   `MB.36.11` deviation record are kept verbatim in substance.

Everything else is byte-identical: the `WorkerRuntime`/`flush_request`
zero-exemption scan over all of `crates/`, the `RequestEnd`/`flush_response`
rules with `PHP_SOLE_IMPLEMENTER`, the planted-name proof's name/count pair and
the test count (6).

The third authorised change is the tightening the architecture review found
necessary, below: the `RequestIo` rule no longer exempts anything, so the guard
now detects exactly the regression it claims to.

### Tightening (architecture review: MAJOR)

The first cut removed the `required` pin but left
`Rule { name: "RequestIo", allowed: PHP_SOLE_IMPLEMENTER }`, so a re-introduced
`pub trait RequestIo` in `blastit-view-php/src/worker.rs` scanned **clean** — a
vacuous guard. Fixed in both rules arrays:

* `Rule { name: "RequestIo", .. }` is `allowed: &[]` in
  `the_deleted_transport_trait_stays_deleted` **and** in the negative twin
  `the_trait_scan_catches_a_planted_name`, so the deleted name is refused in
  every file, its old home included.
* Planted fixture (4) plants only `pub struct RequestEnd;` (a non-exempt file
  naming nothing forbidden still scans clean — 2 findings, the earlier plants).
* New step (5) rewrites that same fixture with
  `pub struct RequestEnd;\npub trait RequestIo {}\n` and requires the tightened
  scan to report **3** findings, one of them `worker.rs:2`. The old fixture, which
  certified a re-planted `RequestIo` as acceptable, is gone.

The proof runs inside the test suite (a `Scratch` tree whose
`blastit-view-php/src/worker.rs` is a copy that reintroduces the trait); the real
file is never edited. Test name and count are unchanged (6), and this is a
**tightening**: the tree now agrees with `ADR-0016` decision 2 about the trait,
the exemption shrank from three names to two, and `RequestIo` — the deleted one —
is now exempt nowhere at all.

### `grep` evidence, honestly

```
$ grep -rn "FakeIo\|dyn RequestIo\|pub trait RequestIo" crates/ \
    | grep -v "crates/blastit/tests/critical_artifacts.rs"
(no output)
```

The whole-tree grep the node's Acceptance asks for can never be empty: the
protected planted-path proof *is* two literal occurrences of those strings in
`crates/blastit/tests/critical_artifacts.rs` — `b"fn f(io: &mut dyn RequestIo)
{}\n"` (the "another path is refused" plant) and `b"pub struct RequestEnd;\npub
trait RequestIo {}\n"` (the "its old home is not an exemption" plant). Both are
written into `Scratch` fixture trees, never into `crates/`. That file is inside
the Authorization as the planted-path refusal proof, so the fixtures stay; outside
that one file the grep is empty, and `grep -rn RequestIo crates/` finds nothing
in `crates/blastit-view-php/` at all.

### Dead public surface (architecture review: MINOR)

`PhpEmbedRuntime::serve` and `ChanIo` are no longer `pub`: the only caller is
`serve_loop` in the same module (`serve_loop(chan, limits, |io| runtime.serve(io),
|| runtime.recycle())`), and there is no external constructor for `ChanIo`, so
both were published-but-uncallable. `grep -rn "ChanIo" crates/` now finds the
name only in prose (this guard's docs and `lib.rs`'s module map) — no code outside
`worker.rs` — and `grep -rn "\.serve(" crates/blastit-view-php/` finds only the
two in-module call sites. Nothing needed `pub(crate)`: the compiler accepts the
fully private pair, so there is no `private_interfaces` motivation left and no
dead public item in the crate's rustdoc.

## Verification

`cargo test -p blastit-view-php --locked` (29 lib cases, the two rewrites
included; the `php_native` integration case needs a host `libphp` and stays
`ignored`) — post-review pass, after `serve`/`ChanIo` went private:

```
test result: ok. 29 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

`cargo test -p blastit --locked --test critical_artifacts` — post-review pass, with
the tightened `RequestIo` rule and the new step (5):

```
running 6 tests
test the_deleted_transport_trait_stays_deleted ... ok
test the_trait_scan_catches_a_planted_name ... ok
test the_wheel_without_an_extension_is_refused ... ok
test the_wheel_extension_links_no_interpreter_and_no_php ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.38s
```

Clippy and rustdoc on the crate (rustdoc matters: the `ChanIo` docs carry
intra-doc links, and `make check`'s `rs-doc` denies warnings):

```
$ cargo clippy -p blastit-view-php --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.30s
$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-view-php --no-deps --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.47s
```

`uv run --no-project tools/critical_guard.py --base 3d4d16f --node MB.40.3.4`
(post-review):

```
critical-guard: 3d4d16f..working tree, 7 changed file(s)
node MB.40.3.4: paths ['crates/blastit-view-php/src/worker.rs', 'crates/blastit-view-php/src/lib.rs', 'crates/blastit/tests/critical_artifacts.rs', 'Cargo.toml', 'Cargo.lock']
  M  crates/blastit/tests/critical_artifacts.rs  [authorized by crates/blastit/tests/critical_artifacts.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.40.3.4
```

`crates/blastit/tests/critical_artifacts.rs` diff is confined to the authorised
lines (`git diff --stat crates/` post-review):

```
 crates/blastit-view-php/src/lib.rs         |   8 +-
 crates/blastit-view-php/src/worker.rs      | 330 ++++++++++++++++-------------
 crates/blastit/tests/critical_artifacts.rs | 106 +++++----
 3 files changed, 246 insertions(+), 198 deletions(-)
```

`make fast-check` is the final gate of this pass and runs on the tree exactly as
described above (only this file changed afterwards, and it is not compiled):
exit `0`, `All checks passed!`, `67 files already formatted`, `plan OK`.

`git status --porcelain` for this node's own work is exactly:

```
 M crates/blastit-view-php/src/lib.rs
 M crates/blastit-view-php/src/worker.rs
 M crates/blastit/tests/critical_artifacts.rs
```

(`plan/BUDGET.md`, `plan/JOURNAL.md`, `MB.40.3.3/LOG.md`, `MB.40.3.4/NODE.md`
and the untracked `MB.40.3.4/LOG.md` are the orchestrator's, not this leaf's;
`Cargo.toml`/`Cargo.lock` are untouched.)

## Follow-ups

* `docs/design/view-crates-architecture.md` §4.2.1/§7 and
  `docs/design/backend-runtimes.md` still describe `RequestIo` as the live
  per-request contract (the latter from the `MB.3` era). Both are outside this
  node's `paths`; `docs/decisions/ADR-0016` now matches the tree as written.
  A docs node should retire the remaining "recorded deviation" prose once
  `RequestEnd`/`flush_response` are pinned as intended API rather than an
  exemption.
* `crates/blastit-view-php/src/worker.rs` is 2 807 lines with tests (~1 560
  production). Deleting the trait shrank it; splitting the remaining production
  half (report, `Prepared`, `PhpSink`, the loop) needs a node whose `paths`
  allow a new module file and is not this convergence leaf's work.
