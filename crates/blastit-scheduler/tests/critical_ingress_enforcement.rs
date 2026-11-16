//! Scheduler ingress enforcement and the `--learn` fallback (`R12`, `R14..R16`,
//! `R26`, design §4.2 D.2).
//!
//! The protected cases: a `raw` body streams with zero scheduler buffer, a
//! chunked body exactly at `BLASTIT_MAX_BODY_BYTES` passes, a valid chunked
//! `json` body reaches the view canonically re-encoded with no stale framing
//! headers, a non-empty body on a route that takes none is a `413`, a buffered
//! body over the cap is a `413` before `REQ_END`, and concurrent unrouted
//! `--learn` requests run one at a time behind a bounded queue whose overflow is
//! a `503`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use blastit_frame::{Frame, ReqHead};
use blastit_manifest::Manifest;
use common::{Options, Script, World, body_of, ended, head, route, status_of};
use tokio::sync::Semaphore;

/// A one-route manifest with the given `[route.ingress]` body codec.
fn ingress_manifest(path: &str, body: &str, allow_raw: bool) -> String {
    let allow = if allow_raw { "allow_raw = true\n" } else { "" };
    format!(
        "version = 1\n[[route]]\nname = \"upload\"\npath = ['{path}']\n\
         [route.ingress]\nbody = \"{body}\"\n{allow}"
    )
}

/// A one-route manifest with **no** `[route.ingress]`, so the route takes no
/// body at all (`R12`).
///
/// The pattern covers the sub-paths the tests use to tell one request from
/// another in the view's log: a view numbers its own requests from one, so the
/// path is the only stable name for "this head, not that one".
fn ingress_less_manifest() -> String {
    format!("version = 1\n{}", route("app", "/app/.*"))
}

/// A `Blastfile` whose mount claims **every** path (`static "/"`) and whose view
/// claims every one-segment path, so a request the mount cannot answer is a
/// `Forward` to the view (`§9.2` steps 1–2).
///
/// This is the front-controller shape: the mount is consulted first for a head
/// that a route also matched, and the route's own `R12` rule must still decide
/// what the view is allowed to see.
const MOUNT_BLASTFILE: &str = concat!(
    "static \"/\" {\n",
    "  root \"public\"\n",
    "}\n",
    "\n",
    "view app GET, POST \"/*\" {\n",
    "}\n",
);

/// A `public/` tree a `static "/"` mount can miss in, removed on drop.
struct StaticTree {
    base: PathBuf,
}

impl StaticTree {
    /// `<base>/public/index.html` — one file, so the mount is real and every
    /// other path is a genuine miss rather than an absent root.
    fn build() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!(
            "blastit-ingress-mount-{}-{nanos}",
            std::process::id()
        ));
        let path = base.join("public/index.html");
        std::fs::create_dir_all(path.parent().expect("the tree has a parent"))
            .expect("create the fixture tree");
        std::fs::write(&path, b"<title>fixture</title>\n").expect("write the fixture file");
        let base = std::fs::canonicalize(&base).expect("canonical fixture base");
        Self { base }
    }

    fn dir(&self) -> &Path {
        &self.base
    }
}

impl Drop for StaticTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A world booting [`MOUNT_BLASTFILE`] over `tree`, with one view that answers
/// every request it receives.
fn mount_world(tree: &StaticTree) -> World {
    let mut options = Options::new().blastfile(MOUNT_BLASTFILE).script(
        0,
        Script::Answer {
            body: b"the-view".to_vec(),
        },
    );
    options.cfg.manifest_dir = tree.dir().to_path_buf();
    World::start(options)
}

/// A request head carrying `Content-Type`, with no `Content-Length` (the
/// chunked shape).
fn chunked_head(method: &str, target: &str, content_type: &str) -> ReqHead {
    let mut request = head(method, target);
    request
        .headers
        .push((b"Content-Type".to_vec(), content_type.as_bytes().to_vec()));
    request
        .headers
        .push((b"Transfer-Encoding".to_vec(), b"chunked".to_vec()));
    request
}

#[tokio::test]
async fn raw_streams_chunk_one_to_the_view_before_chunk_two() {
    // `R16`: a `raw` body is passed through with zero scheduler buffer. The
    // observable proof is that the view receives the first chunk while the
    // client still holds the second.
    let mut world = World::start(
        Options::new()
            .manifest(&ingress_manifest("/upload/", "raw", true))
            .script(
                0,
                Script::Consume {
                    body: b"stored".to_vec(),
                },
            ),
    );

    world.http.send_head(1, head("POST", "/upload/")).await;
    let first = vec![b'a'; 4096];
    world.http.send_body(1, &first).await;

    world
        .until(|world| {
            world.log.lock().unwrap().saw(
                0,
                |frame| matches!(frame, Frame::ReqBody(body) if body == &first),
            )
        })
        .await;

    // Not a byte of the second chunk exists anywhere: the scheduler forwarded
    // the first as it arrived instead of buffering the request.
    let second = vec![b'b'; 4096];
    assert!(
        !world.log.lock().unwrap().saw(
            0,
            |frame| matches!(frame, Frame::ReqBody(body) if body == &second)
        ),
        "the view received chunk two before the client sent it"
    );
    assert!(
        !world
            .log
            .lock()
            .unwrap()
            .saw(0, |frame| matches!(frame, Frame::ReqEnd)),
        "the request ended while chunk two was still unsent"
    );

    world.http.send_body(1, &second).await;
    world.http.send(1, &Frame::ReqEnd).await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"stored");

    // The view saw both chunks, in order.
    let body: Vec<u8> = world
        .log
        .lock()
        .unwrap()
        .received_by(0)
        .into_iter()
        .filter_map(|(_, frame)| match frame {
            Frame::ReqBody(chunk) => Some(chunk),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(body.len(), first.len() + second.len());
    assert_eq!(&body[..first.len()], &first[..]);
    assert_eq!(&body[first.len()..], &second[..]);
}

#[tokio::test]
async fn a_chunked_json_body_at_the_cap_is_accepted() {
    // `R12`/`R14`: `BLASTIT_MAX_BODY_BYTES` is a ceiling, not a lower bound — a
    // body exactly at the cap is accepted.
    let json = b"[1,2,3]";
    let mut options = Options::new()
        .manifest(&ingress_manifest("/json/", "json", false))
        .script(
            0,
            Script::Answer {
                body: b"ok".to_vec(),
            },
        );
    options.cfg.max_body_bytes = json.len() as u64;
    let mut world = World::start(options);

    world
        .http
        .send_head(1, chunked_head("POST", "/json/", "application/json"))
        .await;
    // Two frames, still exactly at the cap once de-chunked.
    world.http.send_body(1, &json[..2]).await;
    world.http.send_body(1, &json[2..]).await;
    world.http.send(1, &Frame::ReqEnd).await;

    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"ok");
    assert_eq!(world.requests()[0].bytes_in(), json.len() as u64);
}

#[tokio::test]
async fn a_valid_chunked_json_body_reaches_the_view_canonically() {
    // `R14`: the view sees validated, canonically re-encoded JSON and none of
    // the stale framing headers — no `Transfer-Encoding`, no `Expect`, one
    // `Content-Length`.
    let raw = b" { \"b\" : 1e0 , \"a\" : \"\\u00e9\" } ";
    let canonical = "{\"a\":\"\u{e9}\",\"b\":1.0}";
    let mut world = World::start(
        Options::new()
            .manifest(&ingress_manifest("/json/", "json", false))
            .script(
                0,
                Script::Answer {
                    body: b"ok".to_vec(),
                },
            ),
    );

    world
        .http
        .send_head(
            1,
            chunked_head("POST", "/json/", "application/vnd.api+json; charset=UTF-8"),
        )
        .await;
    world.http.send_body(1, &raw[..10]).await;
    world.http.send_body(1, &raw[10..]).await;
    world.http.send(1, &Frame::ReqEnd).await;

    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);

    let received = world.log.lock().unwrap().received_by(0);
    let seen_head = received
        .iter()
        .find_map(|(_, frame)| match frame {
            Frame::ReqHead(request) => Some(request),
            _ => None,
        })
        .expect("the view received the request head");
    assert!(
        !seen_head
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(b"transfer-encoding")),
        "HTTP_TRANSFER_ENCODING must never reach the view"
    );
    assert!(
        !seen_head
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(b"content-encoding")),
        "a stale Content-Encoding must never reach the view"
    );
    assert!(
        !seen_head
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(b"expect")),
        "a stale Expect must never reach the view"
    );
    let content_type = seen_head
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(b"content-type"))
        .map(|(_, value)| String::from_utf8(value.clone()).unwrap());
    assert_eq!(content_type.as_deref(), Some("application/json"));
    let content_lengths: Vec<String> = seen_head
        .headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(b"content-length"))
        .map(|(_, value)| String::from_utf8(value.clone()).unwrap())
        .collect();
    assert_eq!(
        content_lengths,
        [canonical.len().to_string()],
        "exactly one canonical Content-Length"
    );

    let body: Vec<u8> = received
        .into_iter()
        .filter_map(|(_, frame)| match frame {
            Frame::ReqBody(chunk) => Some(chunk),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(
        String::from_utf8(body).unwrap(),
        canonical,
        "the view receives canonical, sorted-key JSON"
    );
}

#[tokio::test]
async fn a_buffered_body_over_the_cap_is_refused_before_req_end() {
    tokio::time::pause();
    // `R12`/`R14` and §4.3: a buffered `json` body is capped **per frame**, not
    // once the client can be trusted to stop. Ten bytes against a nine-byte cap
    // is already a `413` while no `REQ_END` has arrived, so the scheduler never
    // grows its buffer to hold the whole body (bounded memory, fail fast).
    let json = b"{\"a\":1234}";
    assert_eq!(json.len(), 10);
    let mut options = Options::new()
        .manifest(&ingress_manifest("/json/", "json", false))
        .script(
            0,
            Script::Answer {
                body: b"leaked".to_vec(),
            },
        );
    options.cfg.max_body_bytes = 9;
    // A long queue timeout keeps the paused clock off the queue timer, so only
    // this test's own short bound can end the wait.
    options.cfg.queue_timeout = Duration::from_secs(3600);
    let mut world = World::start(options);

    world
        .http
        .send_head(1, chunked_head("POST", "/json/", "application/json"))
        .await;
    world.http.send_body(1, json).await;
    // No `REQ_END`: the cap must already have been enforced on that one frame.
    let frames = tokio::time::timeout(Duration::from_secs(5), world.http.awaited_response(1))
        .await
        .expect("no 413 before REQ_END: the per-frame body cap was not enforced");
    assert_eq!(status_of(&frames), 413);
    assert!(ended(&frames));
    assert!(
        !world
            .log
            .lock()
            .unwrap()
            .saw(0, |frame| matches!(frame, Frame::ReqBody(_))),
        "the view received body bytes the scheduler should have refused"
    );
    assert!(
        world.log.lock().unwrap().request_ids(0).is_empty(),
        "an unvalidated buffered body was dispatched to the view"
    );
}

#[tokio::test]
async fn a_non_empty_body_on_an_ingress_less_route_is_413() {
    // `R12`: a route that declared no `[route.ingress]` takes no body at all,
    // and the scheduler answers `413` before a view runs.
    let mut world =
        World::start(Options::new().manifest(&format!("version = 1\n{}", route("app", "/app/"))));

    let mut request = head("POST", "/app/");
    request
        .headers
        .push((b"Content-Length".to_vec(), b"4".to_vec()));
    world.http.send_head(1, request).await;
    world.http.send_body(1, b"body").await;
    world.http.send(1, &Frame::ReqEnd).await;

    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 413);
    assert!(ended(&frames));
    assert!(
        world.log.lock().unwrap().request_ids(0).is_empty(),
        "Python must never see a body the route does not take"
    );
}

#[tokio::test]
async fn a_non_empty_body_on_a_none_route_is_413() {
    // The same rule for an explicit `body = "none"`, on the chunked path where
    // the head alone cannot prove the body empty: the first byte is a `413`.
    //
    // No wait for the view here: the head is *not* dispatchable, so there is
    // nothing to wait for. The byte alone must be the `413`, and the view must
    // never have seen the head — that is the whole point of the gate.
    let mut world = World::start(
        Options::new()
            .manifest(&ingress_manifest("/none/", "none", false))
            .script(0, Script::Silent),
    );

    world.http.send_head(1, head("POST", "/none/")).await;
    world.http.send_body(1, b"x").await;

    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 413);
    assert!(ended(&frames));
    assert!(
        !world
            .log
            .lock()
            .unwrap()
            .saw(0, |frame| matches!(frame, Frame::ReqBody(_))),
        "no body byte reached the view"
    );
    assert!(
        world.log.lock().unwrap().request_ids(0).is_empty(),
        "the view was handed a head this route must refuse from the body's first byte"
    );
}

#[tokio::test]
async fn a_declared_body_on_an_ingress_less_route_is_413_from_the_head() {
    // `R12`, and the race `MB.21.4.8` measured: a route that declared no
    // `[route.ingress]` must refuse a body **from the head**, before any byte
    // and before `REQ_END`. A view is already warm and answers on `REQ_HEAD`
    // (the WSGI app really does), so a head that were dispatched would race the
    // app's `200` against the required `413` — a 1 ms gap between head and body
    // made the `200` win 5/5 against a real `blastit dev`.
    let mut world = World::start(Options::new().manifest(&ingress_less_manifest()).script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    ));

    // A warm view: one request completed through the same route, so the app is
    // running and answers a head the moment it sees one.
    world
        .http
        .send_request(1, head("GET", "/app/warm"), b"")
        .await;
    assert_eq!(status_of(&world.http.response(1).await), 200);
    assert_eq!(view_paths(&world, 0), vec!["/app/warm"]);

    let mut request = head("POST", "/app/refused");
    request
        .headers
        .push((b"Content-Length".to_vec(), b"5".to_vec()));
    world.http.send_head(2, request).await;
    // The gap the real race needed: with the head dispatched, the warm app
    // answers `200` inside it. The `413` must already be complete here — before
    // any body byte exists, and without a `REQ_END` at all.
    tokio::time::sleep(Duration::from_millis(1)).await;
    let frames = world.http.response(2).await;
    assert_eq!(status_of(&frames), 413);
    assert!(ended(&frames));

    // The byte that used to win the race changes nothing now.
    world.http.send_body(2, b"hello").await;
    assert_eq!(
        view_paths(&world, 0),
        vec!["/app/warm"],
        "the request that declared a body was dispatched to the view"
    );
    assert!(
        !world
            .log
            .lock()
            .unwrap()
            .saw(0, |frame| matches!(frame, Frame::ReqBody(_))),
        "a body byte reached the view"
    );
    // No second answer, and no byte counted: the request was over at its head.
    assert!(
        world.http.leftover().is_empty(),
        "the late body byte produced another frame"
    );
    // The `request` event is written by the scheduler after the response, so
    // wait for it instead of reading once: a count is a barrier, a lock is not.
    world
        .until(|world| {
            world
                .requests()
                .iter()
                .any(|event| event.path() == "/app/refused" && event.bytes_in() == 0)
        })
        .await;
    let events = world.requests();
    let refused = events.last().expect("the refused request was reported");
    assert_eq!(refused.path(), "/app/refused");
    assert_eq!(refused.status(), 413);
    assert_eq!(refused.bytes_in(), 0);
}

#[tokio::test]
async fn an_ingress_less_request_is_served_only_after_req_end() {
    // `Content-Length: 0` and a chunked head are the two shapes a route that
    // takes no body may hold *empty*, and neither is provable from the head: an
    // `AllowEmptyOnly` request that can reach a view becomes body-ready at its
    // `REQ_END` and nowhere earlier. The proof is a barrier, not a sleep: a
    // third request completes on the same `http.chan` (so the scheduler
    // necessarily handled every earlier frame), and both heads are still
    // unserved.
    let mut world = World::start(Options::new().manifest(&ingress_less_manifest()).script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    ));

    let mut request = head("GET", "/app/gated");
    request
        .headers
        .push((b"Content-Length".to_vec(), b"0".to_vec()));
    world.http.send_head(1, request).await;
    world
        .http
        .send_head(3, chunked_head("GET", "/app/chunked", "text/plain"))
        .await;

    world
        .http
        .send_request(2, head("GET", "/app/barrier"), b"")
        .await;
    assert_eq!(status_of(&world.http.response(2).await), 200);
    // The barrier: the second request completed, so every frame before its
    // `REQ_END` was handled — and neither head is at the view.
    assert_eq!(
        view_paths(&world, 0),
        vec!["/app/barrier"],
        "a head that has not ended was dispatched to the view"
    );

    // An empty `REQ_END` is the proof, and it is what admits each request.
    world.http.send(1, &Frame::ReqEnd).await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"ok");
    world.http.send(3, &Frame::ReqEnd).await;
    let frames = world.http.response(3).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(
        view_paths(&world, 0),
        vec!["/app/barrier", "/app/gated", "/app/chunked"]
    );
}

#[tokio::test]
async fn a_chunked_ingress_less_head_cannot_reach_the_view_before_req_end() {
    // A chunked head declares a body whose length is unknown, so the head
    // cannot refuse it and cannot dispatch it either: the first byte is the
    // `413`, and the view must never see the head — no `REQ_HEAD`, so no app
    // response can win.
    let mut world = World::start(Options::new().manifest(&ingress_less_manifest()).script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    ));

    world
        .http
        .send_head(
            1,
            chunked_head("POST", "/app/chunked", "application/octet-stream"),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(1)).await;
    world.http.send_body(1, b"x").await;

    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 413);
    assert!(ended(&frames));
    assert!(
        world.log.lock().unwrap().request_ids(0).is_empty(),
        "a chunked head reached the view before its `REQ_END`"
    );
}

#[tokio::test]
async fn a_mount_miss_with_a_body_before_req_end_is_413_without_a_view() {
    // The forward path: a mount that cannot answer hands the request to the
    // view (`§9.2` step 2), and it must hand it over under the route's own
    // `R12` rule. A chunked head whose first byte arrives here used to be
    // dispatched at the head, so the app's `200` beat the byte's `413` — the
    // same race, on the one path a front controller's mount fronts.
    let tree = StaticTree::build();
    let mut world = mount_world(&tree);

    world
        .http
        .send_head(
            1,
            chunked_head("POST", "/gated-miss", "application/octet-stream"),
        )
        .await;
    // The gap a forwarded head needs to be answered in.
    tokio::time::sleep(Duration::from_millis(1)).await;
    world.http.send_body(1, b"x").await;

    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 413);
    assert!(ended(&frames));
    assert!(
        world.log.lock().unwrap().request_ids(0).is_empty(),
        "the view was handed a head the mount could not answer, before its `REQ_END`"
    );
    assert!(
        world.log.lock().unwrap().spawns.is_empty(),
        "a view was started for a request that must be refused"
    );
}

#[tokio::test]
async fn a_mount_miss_reaches_the_view_only_after_an_empty_req_end() {
    // The allowed twin of the case above: the same mount miss, the same route,
    // no byte — and the view's `200`. The gate is the barrier request's answer,
    // not a sleep: the second request completes on the same `http.chan`, so the
    // scheduler has necessarily handled every earlier frame, and the first head
    // is still not at the view.
    let tree = StaticTree::build();
    let mut world = mount_world(&tree);

    world
        .http
        .send_head(1, chunked_head("POST", "/gated-miss", "text/plain"))
        .await;
    world
        .http
        .send_request(2, head("GET", "/barrier-miss"), b"")
        .await;
    assert_eq!(status_of(&world.http.response(2).await), 200);
    assert_eq!(
        view_paths(&world, 0),
        vec!["/barrier-miss"],
        "a forwarded head that has not ended was dispatched to the view"
    );

    world.http.send(1, &Frame::ReqEnd).await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"the-view");
    assert_eq!(view_paths(&world, 0), vec!["/barrier-miss", "/gated-miss"]);
}

#[tokio::test]
async fn an_ingress_less_head_that_never_ends_is_a_bounded_503() {
    tokio::time::pause();
    // A head with no `REQ_END` at all must not park a request: the wait is the
    // same `queue_timeout` bound a half-collected body has, so the client is
    // told to come back instead of holding a stream forever.
    let mut options = Options::new().manifest(&ingress_less_manifest()).script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    );
    options.cfg.queue_timeout = Duration::from_millis(50);
    let mut world = World::start(options);

    let mut request = head("GET", "/app/never");
    request
        .headers
        .push((b"Content-Length".to_vec(), b"0".to_vec()));
    world.http.send_head(1, request).await;

    let frames = tokio::time::timeout(Duration::from_secs(5), world.http.awaited_response(1))
        .await
        .expect("a head with no REQ_END parked the request instead of timing it out");
    assert_eq!(status_of(&frames), 503);
    assert_eq!(body_of(&frames), b"queue timeout\n");
    assert!(ended(&frames));
    assert!(
        world.log.lock().unwrap().request_ids(0).is_empty(),
        "the view was handed a request that never ended"
    );
}

#[tokio::test]
async fn an_ingress_less_head_that_waits_holds_one_queue_slot_and_no_more() {
    // A head waiting for its `REQ_END` keeps a stream open, so it is counted
    // against `queue_size` exactly like a half-collected body: a peer cannot
    // park an unbounded number of them, each holding its raw head. The barrier
    // is the second head's own answer, not a sleep — `queue_size = 2` is
    // already reached by the second waiting head, so it is the `503`.
    let mut options = Options::new().manifest(&ingress_less_manifest()).script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    );
    options.cfg.queue_size = 2;
    let mut world = World::start(options);

    world
        .http
        .send_head(
            1,
            chunked_head("POST", "/app/held", "application/octet-stream"),
        )
        .await;
    world
        .http
        .send_head(
            2,
            chunked_head("POST", "/app/over", "application/octet-stream"),
        )
        .await;

    let frames = world.http.response(2).await;
    assert_eq!(status_of(&frames), 503);
    assert_eq!(body_of(&frames), b"queue full\n");
    assert!(ended(&frames));

    // The request inside the allowance is untouched by the refusal: its empty
    // `REQ_END` still admits it.
    world.http.send(1, &Frame::ReqEnd).await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"ok");
    assert_eq!(view_paths(&world, 0), vec!["/app/held"]);
}

#[tokio::test]
async fn concurrent_unrouted_learn_requests_queue_behind_one_slot() {
    tokio::time::pause();
    // `R26`: `--learn` never short-circuits an unrouted request, but it serves
    // exactly one at a time so `ObsTarget::Unrouted` attribution stays exact.
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new()
        .manifest(&format!("version = 1\n{}", route("known", "/known/")))
        .script(
            0,
            Script::Hold {
                body: b"held".to_vec(),
                gate: gate.clone(),
            },
        );
    options.cfg.learn_mode = true;
    options.cfg.queue_timeout = Duration::from_secs(3600);
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head("GET", "/learn/one"), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;

    // The second unrouted request waits: the single fallback slot is busy.
    world
        .http
        .send_request(2, head("GET", "/learn/two"), b"")
        .await;
    world.advance(Duration::from_millis(1)).await;
    assert_eq!(
        world.log.lock().unwrap().request_ids(0).len(),
        1,
        "the second unrouted request ran while the first still held the slot"
    );

    // Releasing the first lets the second start, and only then.
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(1).await), b"held");
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;

    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(2).await), b"held");

    // An unrouted request is never attributed to the route whose view ran it.
    assert_eq!(
        world.request_routes(),
        vec![None, None],
        "an unrouted request carries no route name"
    );
}

#[tokio::test]
async fn a_full_learn_fallback_queue_answers_503_instead_of_growing() {
    tokio::time::pause();
    // `R26`/§4.2 D.2: the single `--learn` fallback slot is backed by a queue of
    // `BLASTIT_QUEUE_SIZE` *waiting* requests. The first request holds the slot,
    // the second fills the one waiting place, and the third is turned away with
    // `503` and a `Retry-After` instead of being buffered — an unauthenticated
    // scanner must not be able to grow that queue.
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new()
        .manifest(&format!("version = 1\n{}", route("known", "/known/")))
        .script(
            0,
            Script::Hold {
                body: b"held".to_vec(),
                gate: gate.clone(),
            },
        );
    options.cfg.learn_mode = true;
    options.cfg.queue_size = 1;
    options.cfg.queue_timeout = Duration::from_secs(3600);
    let mut world = World::start(options);

    // The first unrouted request takes the one active slot.
    world
        .http
        .send_request(1, head("GET", "/learn/one"), b"")
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;

    // The second fills the one waiting place. Frames on one `http.chan` are
    // ordered and the scheduler handles them one at a time, so by the time it
    // reads the third request the second is necessarily queued: no sleep needed.
    world
        .http
        .send_request(2, head("GET", "/learn/two"), b"")
        .await;
    world
        .http
        .send_request(3, head("GET", "/learn/three"), b"")
        .await;

    let frames = tokio::time::timeout(Duration::from_secs(5), world.http.awaited_response(3))
        .await
        .expect("the third unrouted --learn request was queued instead of refused");
    assert_eq!(status_of(&frames), 503);
    assert_eq!(body_of(&frames), b"queue full\n");
    assert!(ended(&frames));
    let retry_after = frames.iter().find_map(|frame| match frame {
        Frame::RespHead(head) => head
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(b"retry-after"))
            .map(|(_, value)| String::from_utf8(value.clone()).unwrap()),
        _ => None,
    });
    assert_eq!(
        retry_after.as_deref(),
        Some("3600"),
        "a 503 must tell the client when to come back"
    );
    assert_eq!(
        world.log.lock().unwrap().request_ids(0).len(),
        1,
        "the refused request never reached the view"
    );

    // Releasing the first lets the queued second run, and only then.
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(1).await), b"held");
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(2).await), b"held");
}

/// The target of every request head view `route` was handed, in order.
///
/// The id in a view's frame is the view's own (`dispatch` numbers a view's
/// requests from one), so a test that has to say *which* request reached the
/// view names the path instead — and the paths here are distinct on purpose.
fn view_paths(world: &World, route: usize) -> Vec<String> {
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

#[test]
fn routes_from_manifest_carries_the_declared_ingress() {
    // The scheduler can only enforce a policy it actually received: prove the
    // manifest's `[route.ingress]` reaches `RouteInfo`.
    let manifest = Manifest::from_toml_str(&ingress_manifest("/upload/", "raw", true)).unwrap();
    let routes =
        blastit_scheduler::routes_from_manifest(&manifest, &blastit_manifest::UidLayout::default());
    assert_eq!(
        routes[0].ingress_policy().body,
        blastit_ingress::BodyCodec::Raw
    );
    assert!(routes[0].ingress_policy().allow_raw);
}
