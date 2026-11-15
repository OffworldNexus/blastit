//! A completed `5xx`'s label and its `R19` charge, witnessed without a `libphp`
//! (`MB.3.41`, `MB.3.38`, `ADR-0010` §8.1, design `R19`).
//!
//! `MB.3.41` moved the fact that decides both — whether the handler **returned**
//! or **bailed** — onto `DONE`, one frame after the response itself, because a
//! status cannot say it: an app that answers `http_response_code(500)` returned,
//! a script whose `exit()`/`die()`/fatal the SAPI shim contained did not. The
//! scheduler then labels the request `bailed` or `ok` and bills it its wall or
//! the ordinary clamp. That wiring was exercised only by
//! `crates/blastit/tests/critical_php_embed.rs`, on a host with a real NTS
//! `libphp`, so `make check` could not see its call sites and three mutations
//! survived it (the `MB.3.13` closure audit's gap **N1**, `MB.3.46`):
//!
//! 1. dropping `request.bailed = bailed` in `view_done`'s completed-`5xx` arm —
//!    every bailout becomes `ok` and pays the clamp;
//! 2. dropping the `deferred` half of `view_end` — a `5xx` is billed at its
//!    `RESP_END`, before `DONE` has said which it was (`MB.3.38`'s defect: 3.2 ms
//!    charged for an 18 ms request);
//! 3. a producer or a decoder that says `Completed` whatever the frame says —
//!    the label never reaches the wire, or an out-of-set outcome buys a slot.
//!
//! Each leg below is a real `AF_UNIX` socketpair view driven by [`Script`], on
//! `tokio::time::pause()`, so the three spans a request has — what the view
//! reports, what the scheduler observed (dispatch to `DONE`), and the event's
//! own arrival-to-last-byte `duration_ms` — are exact, distinct numbers. Every
//! leg carries its accepted twin in the same test: the same frames with a
//! different `DONE`, or a defined outcome byte, which is what proves the
//! assertion distinguishes the two cases rather than passing on either.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use blastit_events::{Outcome, RequestInfo};
use blastit_frame::{DoneOutcome, Frame, RequestCost};
use common::{Options, Script, World, aborted, ended, head, status_of};
use tokio::sync::Semaphore;

/// What the bailing view says the request cost it: the wall figure `MB.3.38`
/// quote (a script that dies 18 ms in, claiming 3.2 ms) and the CPU figure next
/// to it, which is the view's own measurement and is never the scheduler's to
/// clamp.
const REPORTED_WALL_NS: u64 = 3_200_000;
const REPORTED_CPU_NS: u64 = 1_000_000;

/// How long the test lets the request take before the view ends it, on the
/// paused clock: far longer than the report above.
const HELD: Duration = Duration::from_millis(18);

/// How long after its response end a view may take to send its `DONE`.
const LATE_DONE: Duration = Duration::from_millis(10);

/// The report a bailing view sends.
fn report() -> RequestCost {
    RequestCost::new(REPORTED_WALL_NS, Some(REPORTED_CPU_NS))
}

/// A world whose only view answers `status`, reports `cost` and sends `outcome`
/// in its `DONE` — with the response end and the `DONE` each held until the
/// returned gate is permitted.
///
/// Holding them (never the head) is what keeps the scheduler's floor — the span
/// from dispatch to the view's first frame — at nothing, so the reported and
/// observed figures can be told apart.
fn bailing(
    status: u16,
    cost: Option<RequestCost>,
    outcome: DoneOutcome,
) -> (World, Arc<Semaphore>) {
    let gate = Arc::new(Semaphore::new(0));
    let world = World::start(Options::new().script(
        0,
        Script::AnswerStatus {
            status,
            body: b"boom".to_vec(),
            cost,
            outcome,
            gate: Some(gate.clone()),
        },
    ));
    (world, gate)
}

/// Sends one request and waits until its head is out — but not any further.
///
/// The head on the wire is what fixes the scheduler's floor at now; the view's
/// own log entry is written before it sends anything, so waiting on that would
/// let the test move the clock first.
async fn send_and_await_head(world: &mut World, id: u32, target: &str) {
    world.http.send_request(id, head("GET", target), b"").await;
    world.http.await_head(id).await;
}

/// How many requests view `route` has been given.
fn dispatched(world: &World, route: usize) -> usize {
    world.log.lock().unwrap().request_ids(route).len()
}

/// The `request` event of `path`, once the scheduler has emitted it.
async fn event(world: &World, path: &str) -> RequestInfo {
    world
        .until(|world| world.event_by_path(path).is_some())
        .await;
    world.event_by_path(path).expect("the request was logged")
}

/// The event's own wall span, in nanoseconds: `duration_ms` is the one place
/// `R19`'s ceiling and the charge are supposed to agree.
fn event_wall_ns(event: &RequestInfo) -> u64 {
    // Truncated, never rounded up: this figure is a lower bound on the span the
    // event reports, so `cost_ns >= it` is the claim being made.
    (event.duration_ms() * 1_000_000.0) as u64
}

#[tokio::test]
async fn a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp() {
    tokio::time::pause();
    // The contained bailout: the handler answered `500` and then never
    // returned. It is billed the wall its client waited, and its report — which
    // claims a fifth of that — is not believed at all.
    let (mut world, gate) = bailing(500, Some(report()), DoneOutcome::Bailed);

    send_and_await_head(&mut world, 1, "/bail").await;
    world.advance(HELD).await;
    // The response end, then the `DONE`, both at 18ms.
    gate.add_permits(2);

    // The response the client sees is a complete one: a bailout after a `5xx`
    // is not a `RESP_ABORT`, and it is not `view_died` either.
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 500);
    assert!(
        ended(&frames),
        "a `5xx` a contained bailout answered is a complete response: {frames:?}"
    );

    let bailed = event(&world, "/bail").await;
    assert_eq!(bailed.status(), 500);
    assert_eq!(
        bailed.outcome(),
        Outcome::Bailed,
        "a `DONE` that says the handler bailed is `bailed`, not `ok`"
    );
    // `R19`: the crash ceiling at one live thread is the request's own wall, so
    // the charge is at least the span the event itself reports.
    let wall_ns = event_wall_ns(&bailed);
    assert!(
        bailed.cost_ns() >= wall_ns,
        "{}ns charged for a request the event says lasted {}ns",
        bailed.cost_ns(),
        wall_ns
    );
    assert!(
        bailed.cost_ns() >= HELD.as_nanos() as u64,
        "a bailout pays its wall ({}ms), not the clamp: {}ns",
        HELD.as_millis(),
        bailed.cost_ns()
    );
    assert!(
        bailed.cost_ns() > REPORTED_WALL_NS,
        "the failing view's report of {REPORTED_WALL_NS}ns is not believed: {}ns",
        bailed.cost_ns()
    );
    // The CPU figure is a measurement, not a claim about the span: it travels
    // through untouched.
    assert_eq!(
        bailed.cpu_ns(),
        Some(REPORTED_CPU_NS),
        "the CPU time is still the view's own report"
    );

    // The accepted twin: byte-for-byte the same frames, and a `DONE` that says
    // the handler *returned*. The status alone decides nothing, so this is an
    // answer — `ok`, with its `500`, and the ordinary clamp.
    let (mut world, gate) = bailing(500, Some(report()), DoneOutcome::Completed);
    send_and_await_head(&mut world, 1, "/returned").await;
    world.advance(HELD).await;
    gate.add_permits(2);

    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 500);
    assert!(ended(&frames), "the same complete response: {frames:?}");

    let returned = event(&world, "/returned").await;
    assert_eq!(
        returned.status(),
        500,
        "the status is the app's own, and is still reported"
    );
    assert_eq!(
        returned.outcome(),
        Outcome::Ok,
        "an app that answered its own `500` returned from its handler"
    );
    assert!(
        returned.cost_ns() >= REPORTED_WALL_NS,
        "the report is believed within the clamp: {}ns",
        returned.cost_ns()
    );
    assert!(
        returned.cost_ns() < HELD.as_nanos() as u64,
        "and clamped below the wall it cannot exceed: {}ns",
        returned.cost_ns()
    );
    assert_eq!(
        returned.cpu_ns(),
        Some(REPORTED_CPU_NS),
        "the CPU time is the same view measurement either way"
    );
}

#[tokio::test]
async fn a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done() {
    tokio::time::pause();
    // The response ends at 18 ms and its `DONE` arrives 10 ms later, because
    // the label lives on that frame and `view_end` has to wait for it
    // (`MB.3.38`). What is charged is the span the event reports — arrival to
    // the last byte — and *not* the longer dispatch→`DONE` span the scheduler
    // observed while it waited.
    let (mut world, gate) = bailing(500, Some(report()), DoneOutcome::Bailed);

    send_and_await_head(&mut world, 1, "/late").await;
    world.advance(HELD).await;
    // Only the response end is released: the `DONE` is still to come.
    gate.add_permits(1);

    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 500);
    assert!(ended(&frames), "the response ended at 18ms: {frames:?}");
    assert!(
        world.event_by_path("/late").is_none(),
        "a completed `5xx` waits for its `DONE` before it is labelled or billed"
    );

    // ... and now the frame that says which it was arrives, 10ms later.
    world.advance(LATE_DONE).await;
    gate.add_permits(1);

    let late = event(&world, "/late").await;
    assert_eq!(late.outcome(), Outcome::Bailed);
    let observed_ns = (HELD + LATE_DONE).as_nanos() as u64;
    assert!(
        late.cost_ns() >= HELD.as_nanos() as u64,
        "the span to the last byte is paid: {}ns",
        late.cost_ns()
    );
    assert!(
        late.cost_ns() < observed_ns,
        "{}ns charged: the `DONE`'s own delay is not part of the request the \
         client waited for ({observed_ns}ns)",
        late.cost_ns()
    );
    assert!(
        late.cost_ns() >= event_wall_ns(&late),
        "and it is still at least the wall the event reports: {}ns vs {}ns",
        late.cost_ns(),
        event_wall_ns(&late)
    );
}

#[tokio::test]
async fn an_out_of_set_done_outcome_takes_the_view_down_where_a_defined_one_completes_it() {
    tokio::time::pause();
    // A `DONE` whose outcome byte is `4` — a code this build does not define,
    // so only a compromised (or a later, newer) view sends one. It must not be
    // read as `Completed`: that would close the response the view never
    // finished, free its slot and label a failed request `ok`. It fails the
    // channel instead, and the request is what a view that broke the protocol
    // gets: `view_died`.
    let mut world = World::start(Options::new().script(
        0,
        Script::RawDone {
            status: 500,
            code: 4,
        },
    ));

    world
        .http
        .send_request(1, head("GET", "/hostile"), b"")
        .await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 500, "the head the view did send");
    assert!(
        aborted(&frames),
        "a view that broke the protocol never completed this response: {frames:?}"
    );
    match frames.last() {
        Some(Frame::RespAbort { reason }) => {
            assert_eq!(
                reason, "view_died",
                "and it is a view failure, not an answer"
            );
        }
        other => panic!("expected a `RESP_ABORT`, got {other:?}"),
    }

    let hostile = event(&world, "/hostile").await;
    assert_eq!(
        hostile.outcome(),
        Outcome::ViewDied,
        "an undefined outcome byte is not an `ok` completion"
    );
    assert_eq!(hostile.status(), 500);
    let logged = world
        .requests()
        .iter()
        .filter(|event| event.path() == "/hostile")
        .count();
    assert_eq!(
        logged, 1,
        "the request is logged once, with the failure's label"
    );

    // Its slot is not silently reusable: the channel is gone, and the next
    // request is served by a *new* process.
    world
        .until(|world| world.log.lock().unwrap().closed.contains(&0))
        .await;
    assert_eq!(
        world.log.lock().unwrap().pids.len(),
        1,
        "one process was forked so far"
    );
    world.set_script(
        0,
        Script::Answer {
            body: b"fresh".to_vec(),
        },
    );
    world.http.send_request(2, head("GET", "/after"), b"").await;
    world.until(|world| dispatched(world, 0) == 2).await;
    let after = world.http.response(2).await;
    assert_eq!(status_of(&after), 200);
    assert_eq!(
        world.log.lock().unwrap().pids.len(),
        2,
        "the lost view was replaced before the route served again"
    );

    // The accepted twin: the same frames with the one outcome byte this build
    // *does* define (`0`, `Completed`). The response the view left unfinished
    // is completed on its behalf, the request is an answer, and the very same
    // process takes the next one — which is what shows the leg above is reading
    // the byte and not the status.
    let mut world = World::start(Options::new().script(
        0,
        Script::RawDone {
            status: 500,
            code: 0,
        },
    ));
    world
        .http
        .send_request(1, head("GET", "/defined"), b"")
        .await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 500);
    assert!(
        ended(&frames),
        "a defined `DONE` completes the response: {frames:?}"
    );
    let defined = event(&world, "/defined").await;
    assert_eq!(defined.outcome(), Outcome::Ok);
    assert_eq!(defined.status(), 500);

    world.http.send_request(2, head("GET", "/again"), b"").await;
    world.until(|world| dispatched(world, 0) == 2).await;
    assert_eq!(
        status_of(&world.http.response(2).await),
        500,
        "the same view answered the second request"
    );
    assert_eq!(
        world.log.lock().unwrap().pids.len(),
        1,
        "no process was recycled for an outcome the protocol defines"
    );
}
