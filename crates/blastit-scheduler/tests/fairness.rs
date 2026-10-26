//! Fair queuing between clients: the admission queue's order, the per-client
//! cap, and the client key (ARCHITECTURE.md §2.4).
//!
//! The queue is one FIFO per client served round-robin when
//! `BLASTIT_FAIR_QUEUING` is on, and the first-come-first-served FIFO of §2.4
//! when it is off. Everything here is observable through the fake views: the
//! order they receive `REQ_HEAD` frames in is the dispatch order, and the
//! `request` events carry each request's outcome.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use blastit_events::Outcome;
use blastit_frame::{Frame, ReqHead};
use blastit_scheduler::{ClientKey, TrustedProxies};
use common::{
    Log, Options, Script, World, body_of, dispatch_position, ended, head_from, status_of,
    targets_of,
};
use tokio::sync::Semaphore;

/// One client, with a port the scheduler must ignore.
const A: &str = "198.51.100.10:4001";
/// The same client on another connection: a different port, one key.
const A_OTHER_PORT: &str = "198.51.100.10:4002";
/// A second client.
const B: &str = "198.51.100.11:4001";

/// A `Hold` script whose gate the test releases.
fn held() -> (Arc<Semaphore>, Script) {
    let gate = Arc::new(Semaphore::new(0));
    let script = Script::Hold {
        body: b"held".to_vec(),
        gate: gate.clone(),
    };
    (gate, script)
}

/// `BLASTIT_FAIR_QUEUING=on` with the shared budget of one worker, and the gate
/// the view holds its requests on.
fn fair_options(queue_size: usize) -> (Arc<Semaphore>, Options) {
    let (gate, script) = held();
    let mut options = Options::new().script(0, script);
    options.cfg.fair_queuing = true;
    options.cfg.max_workers = 1;
    options.cfg.queue_size = queue_size;
    (gate, options)
}

/// The `Retry-After` value of a response, if it carries one.
fn retry_after(frames: &[Frame]) -> Option<String> {
    let head = frames.iter().find_map(|frame| match frame {
        Frame::RespHead(head) => Some(head),
        _ => None,
    })?;
    head.headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(b"retry-after"))
        .map(|(_, value)| String::from_utf8_lossy(value).into_owned())
}

/// The request head a test set got, as a `log.received` index would see it.
fn dispatched(log: &Log, target: &str) -> Option<ReqHead> {
    log.received.iter().find_map(|(_, _, frame)| match frame {
        Frame::ReqHead(head) if head.target == target.as_bytes() => Some(head.clone()),
        _ => None,
    })
}

#[tokio::test]
async fn a_burst_from_one_client_does_not_starve_another() {
    tokio::time::pause();
    let (gate, options) = fair_options(256);
    let mut world = World::start(options);

    // A opens with one request, which takes the only worker slot.
    world
        .http
        .send_request(1, head_from("GET", "/a/0", A, &[]), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;

    // Then a burst of 99 more from A, and one from B. Everything is queued:
    // the only slot is taken until the gate is released.
    for index in 1..100u32 {
        world
            .http
            .send_request(
                index + 1,
                head_from("GET", &format!("/a/{index}"), A, &[]),
                b"",
            )
            .await;
    }
    world
        .http
        .send_request(500, head_from("GET", "/b", B, &[]), b"")
        .await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        world.log.lock().unwrap().request_ids(0).len(),
        1,
        "the burst waits behind the one live request"
    );

    // Freeing one slot serves A again (its turn was next), and the one after
    // that serves B: round-robin, not the 100-deep FIFO behind A.
    gate.add_permits(1);
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    gate.add_permits(1);
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 3)
        .await;

    let log = world.log.lock().unwrap();
    assert_eq!(targets_of(&log, 0)[0], "/a/0");
    let position = dispatch_position(&log, "/b").expect("B was dispatched");
    assert!(
        position <= 2,
        "B is served within the first couple of dispatches, not behind 100: {position}"
    );
}

#[tokio::test]
async fn one_client_is_still_first_come_first_served() {
    tokio::time::pause();
    let (gate, options) = fair_options(8);
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head_from("GET", "/1", A, &[]), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    // One client, even on a second connection: the port is not part of the key.
    world
        .http
        .send_request(2, head_from("GET", "/2", A_OTHER_PORT, &[]), b"")
        .await;
    world
        .http
        .send_request(3, head_from("GET", "/3", A, &[]), b"")
        .await;

    gate.add_permits(1);
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    gate.add_permits(1);
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 3)
        .await;

    let log = world.log.lock().unwrap();
    assert_eq!(targets_of(&log, 0), vec!["/1", "/2", "/3"]);
}

#[tokio::test]
async fn a_client_that_queues_too_much_is_refused_with_retry_after() {
    tokio::time::pause();
    let (gate, mut options) = fair_options(64);
    options.cfg.max_queued_per_client = 2;
    options.cfg.queue_timeout = Duration::from_secs(10);
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head_from("GET", "/hold", A, &[]), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    // Two requests fit in A's own queue.
    world
        .http
        .send_request(2, head_from("GET", "/a/1", A, &[]), b"")
        .await;
    world
        .http
        .send_request(3, head_from("GET", "/a/2", A, &[]), b"")
        .await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        world.log.lock().unwrap().request_ids(0).len(),
        1,
        "A's two requests wait"
    );

    // The third exceeds the cap and is answered immediately.
    world
        .http
        .send_request(4, head_from("GET", "/a/3", A, &[]), b"")
        .await;
    let refused = world.http.response(4).await;
    assert_eq!(status_of(&refused), 503);
    assert!(ended(&refused));
    assert_eq!(
        retry_after(&refused).as_deref(),
        Some("10"),
        "§8: a queue 503 carries Retry-After: {refused:?}"
    );
    let event = world.event_by_path("/a/3").expect("its request event");
    assert_eq!(event.outcome(), Outcome::ClientQueueFull);
    assert_eq!(event.status(), 503);

    // Another client is not caught by A's cap: it waits like anyone else.
    world
        .http
        .send_request(5, head_from("GET", "/b", B, &[]), b"")
        .await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert!(
        world.event_by_path("/b").is_none(),
        "B waits rather than being refused"
    );

    // Everything A and B queued is served once the gate opens.
    gate.add_permits(4);
    world.until(|world| world.requests().len() >= 5).await;
    assert_eq!(world.event_by_path("/b").unwrap().status(), 200);
    for path in ["/hold", "/a/1", "/a/2"] {
        assert_eq!(world.event_by_path(path).unwrap().status(), 200, "{path}");
    }
}

#[tokio::test]
async fn the_per_client_cap_only_applies_to_fair_queuing() {
    tokio::time::pause();
    let (gate, _) = held();
    let mut options = Options::new().script(
        0,
        Script::Hold {
            body: b"held".to_vec(),
            gate: gate.clone(),
        },
    );
    options.cfg.fair_queuing = false;
    options.cfg.max_queued_per_client = 1;
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 8;
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head_from("GET", "/1", A, &[]), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    for (id, path) in [(2, "/2"), (3, "/3"), (4, "/4")] {
        world
            .http
            .send_request(id, head_from("GET", path, A, &[]), b"")
            .await;
    }
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert!(
        world.requests().is_empty(),
        "with fair queuing off the cap is not consulted: {:?}",
        world.requests()
    );
    assert_eq!(world.log.lock().unwrap().request_ids(0).len(), 1);

    gate.add_permits(4);
    for id in 1..=4 {
        assert_eq!(body_of(&world.http.response(id).await), b"held");
    }
    for path in ["/1", "/2", "/3", "/4"] {
        assert_eq!(world.event_by_path(path).unwrap().status(), 200, "{path}");
    }
}

#[tokio::test]
async fn a_header_key_is_only_believed_from_a_trusted_proxy() {
    tokio::time::pause();
    let (gate, mut options) = fair_options(64);
    options.cfg.max_queued_per_client = 1;
    options.cfg.client_key = ClientKey::Header("x-forwarded-for".to_string());
    options.cfg.trusted_proxies = TrustedProxies::parse("203.0.113.0/24").unwrap();
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head_from("GET", "/hold", A, &[]), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;

    // From an untrusted peer, the header is ignored: these two requests share
    // the peer's key, so with a cap of one the second is refused.
    world
        .http
        .send_request(
            2,
            head_from(
                "GET",
                "/u/1",
                "198.51.100.20:1",
                &[("X-Forwarded-For", "10.0.0.1")],
            ),
            b"",
        )
        .await;
    world
        .http
        .send_request(
            3,
            head_from(
                "GET",
                "/u/2",
                "198.51.100.20:2",
                &[("X-Forwarded-For", "10.0.0.2")],
            ),
            b"",
        )
        .await;
    let refused = world.http.response(3).await;
    assert_eq!(
        status_of(&refused),
        503,
        "the untrusted header does not split the peer"
    );
    assert_eq!(
        world.event_by_path("/u/2").unwrap().outcome(),
        Outcome::ClientQueueFull
    );

    // From a trusted proxy, the header names the client: two different values
    // are two clients, each with its own queue.
    world
        .http
        .send_request(
            4,
            head_from(
                "GET",
                "/t/1",
                "203.0.113.7:6000",
                &[("X-Forwarded-For", "10.0.0.1")],
            ),
            b"",
        )
        .await;
    world
        .http
        .send_request(
            5,
            head_from(
                "GET",
                "/t/2",
                "203.0.113.7:6001",
                &[("X-Forwarded-For", "10.0.0.2")],
            ),
            b"",
        )
        .await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert!(
        world.event_by_path("/t/1").is_none() && world.event_by_path("/t/2").is_none(),
        "the trusted header gives each its own queue"
    );

    // The same header value again is the same client, and its queue is full.
    world
        .http
        .send_request(
            6,
            head_from(
                "GET",
                "/t/3",
                "203.0.113.7:6002",
                &[("X-Forwarded-For", "10.0.0.1")],
            ),
            b"",
        )
        .await;
    let refused = world.http.response(6).await;
    assert_eq!(status_of(&refused), 503);
    assert_eq!(
        world.event_by_path("/t/3").unwrap().outcome(),
        Outcome::ClientQueueFull
    );

    gate.add_permits(4);
    world.until(|world| world.requests().len() >= 6).await;
}

#[tokio::test]
async fn the_header_value_never_reaches_the_event_stream() {
    tokio::time::pause();
    let (gate, mut options) = fair_options(8);
    options.cfg.client_key = ClientKey::Header("x-forwarded-for".to_string());
    options.cfg.trusted_proxies = TrustedProxies::parse("*").unwrap();
    let mut world = World::start(options);

    // A distinctive client address, small enough to never be truncated, that
    // the key is derived from but §8.1 forbids logging.
    world
        .http
        .send_request(
            1,
            head_from(
                "GET",
                "/keyed",
                "203.0.113.9:1",
                &[("X-Forwarded-For", "198.18.7.7")],
            ),
            b"",
        )
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    gate.add_permits(1);
    world
        .until(|world| world.event_by_path("/keyed").is_some())
        .await;
    let lines = world.event_lines();
    assert!(!lines.is_empty());
    for line in &lines {
        assert!(
            !line.contains("198.18.7.7"),
            "the client key leaked into the event stream: {line}"
        );
    }
    let event = world.event_by_path("/keyed").unwrap();
    assert_eq!(event.outcome(), Outcome::Ok);
    assert_eq!(event.remote_addr(), "203.0.113.9:1");
    assert!(dispatched(&world.log.lock().unwrap(), "/keyed").is_some());
}

#[tokio::test]
async fn fairness_holds_when_routes_compete_for_the_global_budget() {
    tokio::time::pause();
    let (gate, _) = held();
    // `prefork = true` on the admin route so its view is ready before the two
    // clients compete: the test is about the order the slot is handed out in,
    // not about how long a view takes to fork.
    let manifest = format!(
        "version = 1\n{}[[route]]\nname = \"admin\"\npath = ['/admin/.*']\nprefork = true\n",
        common::route("shop", "/shop/.*")
    );
    let mut options = Options::new()
        .manifest(&manifest)
        .script(
            0,
            Script::Hold {
                body: b"held".to_vec(),
                gate: gate.clone(),
            },
        )
        .script(
            1,
            Script::Answer {
                body: b"admin".to_vec(),
            },
        );
    options.cfg.fair_queuing = true;
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 256;
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head_from("GET", "/shop/0", A, &[]), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    for index in 1..100u32 {
        world
            .http
            .send_request(
                index + 1,
                head_from("GET", &format!("/shop/{index}"), A, &[]),
                b"",
            )
            .await;
    }
    world
        .http
        .send_request(500, head_from("GET", "/admin/x", B, &[]), b"")
        .await;

    gate.add_permits(1);
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    gate.add_permits(1);
    world
        .until(|world| world.log.lock().unwrap().request_ids(1).len() == 1)
        .await;

    let log = world.log.lock().unwrap();
    let position = dispatch_position(&log, "/admin/x").expect("B was dispatched");
    assert!(
        position <= 2,
        "with routes competing for the one slot, B is served within the first \
         couple of dispatches: {position}"
    );
}

#[tokio::test]
async fn fair_queuing_off_keeps_the_shared_fifo() {
    tokio::time::pause();
    let (gate, _) = held();
    let mut options = Options::new().script(
        0,
        Script::Hold {
            body: b"held".to_vec(),
            gate: gate.clone(),
        },
    );
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 256;
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head_from("GET", "/a/0", A, &[]), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    for index in 1..50u32 {
        world
            .http
            .send_request(
                index + 1,
                head_from("GET", &format!("/a/{index}"), A, &[]),
                b"",
            )
            .await;
    }
    world
        .http
        .send_request(500, head_from("GET", "/b", B, &[]), b"")
        .await;

    for _ in 0..5 {
        let seen = world.log.lock().unwrap().request_ids(0).len();
        gate.add_permits(1);
        world
            .until(move |world| world.log.lock().unwrap().request_ids(0).len() > seen)
            .await;
    }

    let log = world.log.lock().unwrap();
    let order = common::dispatch_order(&log);
    assert!(order.len() >= 5, "{order:?}");
    assert!(
        order[..5]
            .iter()
            .all(|(_, target)| target.starts_with("/a/")),
        "the off default is one FIFO: B waits behind A's burst, got {order:?}"
    );
}

#[tokio::test]
async fn a_full_global_queue_still_answers_queue_full_with_retry_after() {
    tokio::time::pause();
    let (gate, mut options) = fair_options(1);
    options.cfg.queue_timeout = Duration::from_secs(30);
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head_from("GET", "/1", A, &[]), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    world
        .http
        .send_request(2, head_from("GET", "/2", A, &[]), b"")
        .await;
    world
        .http
        .send_request(3, head_from("GET", "/3", B, &[]), b"")
        .await;

    let refused = world.http.response(3).await;
    assert_eq!(status_of(&refused), 503);
    assert_eq!(retry_after(&refused).as_deref(), Some("30"));
    assert_eq!(
        world.event_by_path("/3").unwrap().outcome(),
        Outcome::QueueFull
    );

    gate.add_permits(2);
    assert_eq!(body_of(&world.http.response(1).await), b"held");
    assert_eq!(body_of(&world.http.response(2).await), b"held");
}

#[tokio::test]
async fn a_queued_request_still_times_out_per_client() {
    tokio::time::pause();
    let (gate, mut options) = fair_options(8);
    options.cfg.queue_timeout = Duration::from_secs(10);
    options.cfg.request_timeout = Duration::from_secs(120);
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head_from("GET", "/1", A, &[]), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    world
        .http
        .send_request(2, head_from("GET", "/b", B, &[]), b"")
        .await;
    world
        .http
        .send_request(3, head_from("GET", "/2", A, &[]), b"")
        .await;

    let refused = world.http.awaited_response(3).await;
    assert_eq!(status_of(&refused), 503);
    let event = world.event_by_path("/2").unwrap();
    assert_eq!(event.outcome(), Outcome::QueueTimeout);
    assert!(event.queued_ms() >= 10_000.0, "{:?}", event.queued_ms());

    // Both clients' queued requests expired: the timeout is per request, and
    // a per-client queue does not exempt anyone from it.
    let other = world.event_by_path("/b").unwrap();
    assert_eq!(other.outcome(), Outcome::QueueTimeout);
    assert_eq!(other.status(), 503);

    // The request that was already running was never touched.
    gate.add_permits(1);
    world
        .until(|world| world.event_by_path("/1").is_some())
        .await;
    assert_eq!(world.event_by_path("/1").unwrap().status(), 200);
}
