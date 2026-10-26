//! Fair queuing: the two-level fair scheduler, the cost it charges, and the
//! client-IP resolution it keys on (ARCHITECTURE.md §2.4, D5).
//!
//! The queue's *order* is the only thing fair queuing changes, so these tests
//! drive it through the one thing that is observable from outside: which
//! request each view receives next. The flow tables, the virtual clock and the
//! eviction rules are unit-tested next to the code that implements them
//! (`crates/blastit-scheduler/src/fair.rs`).
//!
//! A view's ids are its own (§3.1: the scheduler rewrites them), so a request is
//! identified here by its path.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use blastit_events::Outcome;
use blastit_frame::{ClientIpConfig, Frame, RequestCost};
use common::{Options, Script, World, body_of, head, head_from, status_of};
use tokio::sync::Semaphore;

/// The manifest with two routes for the blocked-head tests: `/a/…` and `/b/…`.
const TWO_ROUTES: &str = "version = 1\n\
     [[route]]\nname = \"a\"\npath = ['/a/.*']\n\
     [[route]]\nname = \"b\"\npath = ['/b/.*']\n";

/// A view that answers with `body` and reports `cost` for every request.
fn reporting(body: &[u8], cost: RequestCost, gate: Arc<Semaphore>) -> Script {
    Script::HeldCost {
        body: body.to_vec(),
        cost,
        gate,
    }
}

/// The paths of the requests view `route` received, in the order it received
/// them — which is the order the scheduler dispatched them.
fn served(world: &World, route: usize) -> Vec<String> {
    world
        .log
        .lock()
        .unwrap()
        .received_by(route)
        .into_iter()
        .filter_map(|(_, frame)| match frame {
            Frame::ReqHead(head) => Some(String::from_utf8_lossy(&head.target).into_owned()),
            _ => None,
        })
        .collect()
}

/// Three requests from three socket addresses, each carrying its own forged
/// client-IP chain: two name the same real client, the third names another.
///
/// The first request of the shared client is the one that holds the only worker
/// slot, so it is the one that pays for that client's flow (`cost`). What the
/// test then observes is which request goes next: the third one, whose client
/// has spent nothing — or the second one, if the two requests of the shared
/// client were counted as two clients, each with nothing spent, which is what a
/// resolver that reads the *leftmost* entry of the chain would do.
///
/// Returns the order the view received the three requests in, by path.
async fn three_clients(
    trusted: &[&str],
    peers: (&str, &str, &str),
    chains: (&str, &str, &str),
    header: &str,
) -> Vec<String> {
    let cost = RequestCost::new(40_000_000, 20_000_000);
    let (first, second, third) = peers;
    let (first_chain, second_chain, third_chain) = chains;
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new().script(0, reporting(b"ok", cost, gate.clone()));
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 64;
    options.cfg.queue_timeout = Duration::from_secs(600);
    options.cfg.fair_queuing = true;
    options.cfg.fair.client_ip = ClientIpConfig::new(header, trusted).unwrap();
    let mut world = World::start(options);

    let held = head_from("GET", "/a/1", first, Some(first_chain));
    world.http.send_request(1, held, b"").await;
    world.until(|world| served(world, 0).len() == 1).await;

    let same = head_from("GET", "/a/2", second, Some(second_chain));
    world.http.send_request(2, same, b"").await;
    let other = head_from("GET", "/b/1", third, Some(third_chain));
    world.http.send_request(3, other, b"").await;
    world.advance(Duration::from_millis(1)).await;

    // The held request ends and pays for its client's flow.
    gate.add_permits(1);
    world.until(|world| served(world, 0).len() == 2).await;
    let order = served(&world, 0);

    // ... and the rest follow.
    gate.add_permits(2);
    world.until(|world| served(world, 0).len() == 3).await;
    for id in 1..=3 {
        let _ = world.http.response(id).await;
    }
    order
}

#[tokio::test]
async fn with_fair_queuing_off_a_blocked_head_does_not_hold_up_another_route() {
    tokio::time::pause();
    // Route `a`'s view never becomes ready, so its request cannot be served;
    // route `b`'s view is ready. The queue holds a's request first, and b's
    // request must still go out: §2.4's FIFO skips what it cannot serve rather
    // than letting the head hold the queue (D5 keeps that walk untouched).
    let mut options = Options::new()
        .manifest(TWO_ROUTES)
        .script(
            0,
            Script::NeverReady {
                after: Duration::from_secs(3600),
            },
        )
        .script(
            1,
            Script::Answer {
                body: b"b".to_vec(),
            },
        );
    options.cfg.max_workers = 4;
    assert!(!options.cfg.fair_queuing, "the FIFO is the default");
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/1"), b"").await;
    world.http.send_request(2, head("GET", "/b/1"), b"").await;
    world.until(|world| !served(world, 1).is_empty()).await;
    assert_eq!(served(&world, 1), ["/b/1"], "b is served behind a's head");
    assert!(served(&world, 0).is_empty(), "a's view never became ready");
    assert_eq!(body_of(&world.http.response(2).await), b"b");
}

#[tokio::test]
async fn with_fair_queuing_on_a_blocked_head_does_not_hold_up_another_route() {
    tokio::time::pause();
    // The same walk, with the fair order: a route that cannot take a request is
    // not one of the candidates, so it cannot hold up the routes that can.
    let mut options = Options::new()
        .manifest(TWO_ROUTES)
        .script(
            0,
            Script::NeverReady {
                after: Duration::from_secs(3600),
            },
        )
        .script(
            1,
            Script::Answer {
                body: b"b".to_vec(),
            },
        );
    options.cfg.max_workers = 4;
    options.cfg.fair_queuing = true;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/1"), b"").await;
    world.http.send_request(2, head("GET", "/b/1"), b"").await;
    world.until(|world| !served(world, 1).is_empty()).await;
    assert_eq!(served(&world, 1), ["/b/1"]);
    assert!(served(&world, 0).is_empty());
    assert_eq!(body_of(&world.http.response(2).await), b"b");
}

/// One worker slot, a view that answers only when permitted, and client A's
/// burst queued behind A's first request.
///
/// Returns the world and the gate; `count` is how many *more* requests of A
/// are queued.
async fn burst_behind_one_held(fair: bool, count: u32) -> (World, Arc<Semaphore>) {
    let cost = RequestCost::new(40_000_000, 20_000_000);
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new().script(0, reporting(b"ok", cost, gate.clone()));
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 256;
    options.cfg.queue_timeout = Duration::from_secs(600);
    options.cfg.fair_queuing = fair;
    let mut world = World::start(options);

    // Client A takes the only slot.
    world
        .http
        .send_request(1, head_from("GET", "/a/1", "10.0.0.1:999", None), b"")
        .await;
    world.until(|world| served(world, 0).len() == 1).await;

    // The rest of A's burst waits behind it.
    for index in 2..=count + 1 {
        let head = head_from("GET", &format!("/a/{index}"), "10.0.0.1:999", None);
        world.http.send_request(index, head, b"").await;
    }
    world.advance(Duration::from_millis(1)).await;
    (world, gate)
}

#[tokio::test]
async fn with_fair_queuing_off_the_queue_is_arrival_order() {
    tokio::time::pause();
    let (mut world, gate) = burst_behind_one_held(false, 3).await;
    // Client B arrives last, and the FIFO serves it last: cost plays no part.
    world
        .http
        .send_request(100, head_from("GET", "/b", "10.1.0.1:999", None), b"")
        .await;

    gate.add_permits(5);
    world.until(|world| served(world, 0).len() == 5).await;
    assert_eq!(
        served(&world, 0),
        ["/a/1", "/a/2", "/a/3", "/a/4", "/b"],
        "the FIFO is arrival order, cost or no cost"
    );
    for id in [1, 2, 3, 4, 100] {
        let _ = world.http.response(id).await;
    }
}

#[tokio::test]
async fn an_expensive_client_queues_behind_a_cheap_one() {
    tokio::time::pause();
    // A's hundred requests are all queued, and A has just spent 40ms of the
    // worker's time on the first of them. One request from B — another /24, so
    // another flow — must go next.
    let (mut world, gate) = burst_behind_one_held(true, 100).await;
    assert_eq!(served(&world, 0), ["/a/1"]);

    world
        .http
        .send_request(1000, head_from("GET", "/b", "10.1.0.1:999", None), b"")
        .await;
    world.advance(Duration::from_millis(1)).await;

    gate.add_permits(1);
    world.until(|world| served(world, 0).len() == 2).await;
    assert_eq!(
        served(&world, 0),
        ["/a/1", "/b"],
        "one cheap request beats a hundred queued expensive ones"
    );

    // ... and A's own hundred still follow, in arrival order: a flow is not
    // punished beyond the budget it spent.
    gate.add_permits(101);
    world.until(|world| served(world, 0).len() == 102).await;
    let order = served(&world, 0);
    assert_eq!(&order[..2], ["/a/1", "/b"]);
    let expected: Vec<String> = (2..=101).map(|index| format!("/a/{index}")).collect();
    assert_eq!(
        &order[2..],
        &expected[..],
        "one flow keeps its arrival order"
    );
    for id in (1..=101).chain(std::iter::once(1000)) {
        let _ = world.http.response(id).await;
    }
    assert_eq!(world.requests().len(), 102);
}

#[tokio::test]
async fn a_cheap_client_is_favoured_over_an_expensive_one() {
    tokio::time::pause();
    // A holds the only slot and has queued a second request; B queues one of
    // its own. When A's first request ends — it cost 40ms — B goes first.
    let gate = Arc::new(Semaphore::new(0));
    let cost = RequestCost::new(40_000_000, 20_000_000);
    let mut options = Options::new().script(0, reporting(b"ok", cost, gate.clone()));
    options.cfg.max_workers = 1;
    options.cfg.queue_timeout = Duration::from_secs(600);
    options.cfg.fair_queuing = true;
    let mut world = World::start(options);

    let a_first = head_from("GET", "/a/1", "10.0.0.1:999", None);
    world.http.send_request(1, a_first, b"").await;
    world.until(|world| served(world, 0).len() == 1).await;

    let a_second = head_from("GET", "/a/2", "10.0.0.1:999", None);
    world.http.send_request(2, a_second, b"").await;
    let b = head_from("GET", "/b", "10.2.0.1:999", None);
    world.http.send_request(3, b, b"").await;
    world.advance(Duration::from_millis(1)).await;

    gate.add_permits(1);
    world.until(|world| served(world, 0).len() == 2).await;
    assert_eq!(
        served(&world, 0),
        ["/a/1", "/b"],
        "the client that has spent nothing goes first"
    );
    // A permit each, so both of the requests that are still waiting can end.
    gate.add_permits(2);
    world.until(|world| served(world, 0).len() == 3).await;
    assert_eq!(served(&world, 0), ["/a/1", "/b", "/a/2"]);
    for id in 1..=3 {
        let _ = world.http.response(id).await;
    }
}

#[tokio::test]
async fn a_report_larger_than_the_observed_time_is_clamped() {
    tokio::time::pause();
    // A view that claims ten seconds — or the whole `u64` — for a request the
    // scheduler watched for a few milliseconds is a view that must not be able
    // to buy its clients the whole queue. The view waits for a permit, so the
    // test decides how long the request takes: 5ms of the paused clock.
    for report in [10_000_000_000, u64::MAX] {
        let cost = RequestCost::new(report, 3_000);
        let gate = Arc::new(Semaphore::new(0));
        let mut options = Options::new().script(0, reporting(b"ok", cost, gate.clone()));
        options.cfg.fair_queuing = true;
        let mut world = World::start(options);

        world.http.send_request(1, head("GET", "/"), b"").await;
        world.until(|world| served(world, 0).len() == 1).await;
        world.advance(Duration::from_millis(5)).await;
        gate.add_permits(1);
        assert_eq!(body_of(&world.http.response(1).await), b"ok");
        world
            .until(|world| world.event_by_path("/").is_some())
            .await;

        let event = world.event_by_path("/").unwrap();
        assert_eq!(event.outcome(), Outcome::Ok);
        assert!(
            event.cost_ns() >= 5_000_000,
            "the request took the 5ms the test let it take: {}ns",
            event.cost_ns()
        );
        assert!(
            event.cost_ns() < report,
            "the report of {report}ns is clamped, not believed: {}ns",
            event.cost_ns()
        );
        // The clamp is the scheduler's own observation, which cannot exceed the
        // request's own duration (§8.1: arrival to the last response byte).
        let bound = ((event.duration_ms() * 1_000_000.0).ceil() as u64).max(5_000_000);
        assert!(
            event.cost_ns() <= bound,
            "{}ns charged for a {bound}ns request",
            event.cost_ns()
        );
        assert_eq!(
            event.cpu_ns(),
            Some(3_000),
            "the CPU time is the view's own report, and is not the scheduler's to clamp"
        );
    }
}

#[tokio::test]
async fn with_no_report_the_observed_time_is_charged() {
    tokio::time::pause();
    // A view that ends its response the way it did before D5 (`RESP_END` with
    // no cost), held for 7ms: the scheduler charges the time it observed.
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new().script(
        0,
        Script::Hold {
            body: b"ok".to_vec(),
            gate: gate.clone(),
        },
    );
    options.cfg.fair_queuing = true;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/"), b"").await;
    world.until(|world| served(world, 0).len() == 1).await;
    world.advance(Duration::from_millis(7)).await;
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(1).await), b"ok");
    world
        .until(|world| world.event_by_path("/").is_some())
        .await;

    let event = world.event_by_path("/").unwrap();
    assert_eq!(event.status(), 200);
    assert!(
        event.cost_ns() >= 7_000_000,
        "the observed time is the fallback, not zero: {}ns",
        event.cost_ns()
    );
    assert_eq!(event.cpu_ns(), None, "no view measured this request");
}

#[tokio::test]
async fn a_forged_header_from_an_untrusted_peer_is_ignored() {
    tokio::time::pause();
    // Every request claims to come from an address of its own invention, but no
    // peer is trusted: the first two share a /24 (one client) and the third is
    // another client, so the third goes next. If any header were believed, the
    // first two would be two clients and the second request would go next.
    let order = three_clients(
        &[],
        ("198.51.100.1:999", "198.51.100.1:1000", "192.0.2.7:999"),
        ("203.0.113.99", "203.0.113.100", "203.0.113.101"),
        "x-forwarded-for",
    )
    .await;
    assert_eq!(
        order,
        ["/a/1", "/b/1"],
        "the third client's request comes second"
    );
}

#[tokio::test]
async fn behind_a_trusted_proxy_a_forged_leftmost_entry_never_names_the_client() {
    tokio::time::pause();
    // The trusted proxy appends what it saw, so the rightmost entry is its own
    // address and the one before it is the client; everything to the left is
    // the client's own invention. The first two requests forge *different*
    // leftmost entries but name the same client, so they are one client and the
    // third — which names someone else — goes next. A resolver that read the
    // leftmost entry would see three clients and serve them in arrival order.
    let order = three_clients(
        &["127.0.0.1"],
        ("127.0.0.1:999", "127.0.0.1:1000", "127.0.0.1:1001"),
        (
            "6.6.6.6, 203.0.113.5, 127.0.0.1",
            "6.6.6.7, 203.0.113.5, 127.0.0.1",
            "6.6.6.8, 198.51.100.9, 127.0.0.1",
        ),
        "x-forwarded-for",
    )
    .await;
    assert_eq!(
        order,
        ["/a/1", "/b/1"],
        "the forged leftmost entry does not decide the client"
    );
}

#[tokio::test]
async fn under_a_star_the_forged_leftmost_entry_is_not_the_client() {
    tokio::time::pause();
    // `*` trusts every peer, which is the configuration where the reference
    // implementation reads the *leftmost* entry — the one the client wrote
    // itself. blastit reads the rightmost instead, so the same two requests are
    // one client here as well.
    let order = three_clients(
        &["*"],
        ("10.0.0.1:999", "10.0.0.2:999", "10.0.0.3:999"),
        (
            "6.6.6.6, 203.0.113.5",
            "6.6.6.7, 203.0.113.5",
            "6.6.6.8, 198.51.100.9",
        ),
        "x-forwarded-for",
    )
    .await;
    assert_eq!(
        order,
        ["/a/1", "/b/1"],
        "the rightmost entry is the client, not the forged one"
    );
}

#[tokio::test]
async fn do_connecting_ip_decides_the_client_on_app_platform() {
    tokio::time::pause();
    // App Platform: the socket peer is a DigitalOcean ingress address and
    // `do-connecting-ip` is the one header the edge overwrites (see §10), so it
    // is the one blastit is configured to read.
    let cost = RequestCost::new(40_000_000, 20_000_000);
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new().script(0, reporting(b"ok", cost, gate.clone()));
    options.cfg.max_workers = 1;
    options.cfg.queue_timeout = Duration::from_secs(600);
    options.cfg.fair_queuing = true;
    options.cfg.fair.client_ip = ClientIpConfig::new("do-connecting-ip", &["10.0.0.0/8"]).unwrap();
    let mut world = World::start(options);

    // One client, two requests: the same `do-connecting-ip`, different ingress
    // addresses.
    let mut first = head_from("GET", "/a/1", "10.244.3.9:1", None);
    first
        .headers
        .push((b"do-connecting-ip".to_vec(), b"203.0.113.5".to_vec()));
    world.http.send_request(1, first, b"").await;
    world.until(|world| served(world, 0).len() == 1).await;

    let mut second = head_from("GET", "/a/2", "10.244.7.1:1", None);
    second
        .headers
        .push((b"do-connecting-ip".to_vec(), b"203.0.113.5".to_vec()));
    world.http.send_request(2, second, b"").await;

    // A different client, also from an ingress address.
    let mut other = head_from("GET", "/b", "10.244.3.9:2", None);
    other
        .headers
        .push((b"do-connecting-ip".to_vec(), b"198.51.100.9".to_vec()));
    world.http.send_request(3, other, b"").await;
    world.advance(Duration::from_millis(1)).await;

    gate.add_permits(1);
    world.until(|world| served(world, 0).len() == 2).await;
    assert_eq!(
        served(&world, 0),
        ["/a/1", "/b"],
        "the client that has spent nothing goes first"
    );
    gate.add_permits(2);
    world.until(|world| served(world, 0).len() == 3).await;
    assert_eq!(served(&world, 0), ["/a/1", "/b", "/a/2"]);
    for id in 1..=3 {
        let _ = world.http.response(id).await;
    }
}

#[tokio::test]
async fn a_503_carries_a_retry_after() {
    tokio::time::pause();
    // §8: a `503` tells the client when to come back, which is §2.4's queue
    // timeout rounded up and never less than a second.
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new().script(
        0,
        Script::Hold {
            body: b"held".to_vec(),
            gate: gate.clone(),
        },
    );
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 1;
    options.cfg.queue_timeout = Duration::from_millis(1500);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world.until(|world| !served(world, 0).is_empty()).await;
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world.http.send_request(3, head("GET", "/3"), b"").await;

    let frames = world.http.response(3).await;
    assert_eq!(status_of(&frames), 503);
    let retry_after = retry_after_of(&frames).expect("a 503 carries Retry-After");
    assert_eq!(retry_after, "2", "1500ms rounds up to two seconds");
    gate.add_permits(2);
    for id in 1..=2 {
        let _ = world.http.response(id).await;
    }
}

#[tokio::test]
async fn a_queue_timeout_carries_a_retry_after() {
    tokio::time::pause();
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new().script(
        0,
        Script::Hold {
            body: b"held".to_vec(),
            gate: gate.clone(),
        },
    );
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 8;
    options.cfg.queue_timeout = Duration::from_secs(2);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world.until(|world| !served(world, 0).is_empty()).await;
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world.advance(Duration::from_secs(3)).await;

    let frames = world.http.awaited_response(2).await;
    assert_eq!(status_of(&frames), 503);
    assert_eq!(retry_after_of(&frames).as_deref(), Some("2"));
    gate.add_permits(1);
    let _ = world.http.response(1).await;
}

/// The `Retry-After` of a response, if it has one.
fn retry_after_of(frames: &[Frame]) -> Option<String> {
    let Some(Frame::RespHead(head)) = frames.first() else {
        return None;
    };
    head.headers.iter().find_map(|(name, value)| {
        (name.eq_ignore_ascii_case(b"retry-after"))
            .then(|| String::from_utf8_lossy(value).into_owned())
    })
}
