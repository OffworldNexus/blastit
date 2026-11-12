//! Scheduler ingress enforcement and the `--learn` fallback (`R12`, `R14..R16`,
//! `R26`, design §4.2 D.2).
//!
//! The protected cases: a `raw` body streams with zero scheduler buffer, a
//! chunked body exactly at `BLASTIT_MAX_BODY_BYTES` passes, a valid chunked
//! `json` body reaches the view canonically re-encoded with no stale framing
//! headers, a non-empty body on a route that takes none is a `413`, and
//! concurrent unrouted `--learn` requests run one at a time.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

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
    let mut world = World::start(
        Options::new()
            .manifest(&ingress_manifest("/none/", "none", false))
            .script(0, Script::Silent),
    );

    world.http.send_head(1, head("POST", "/none/")).await;
    world
        .until(|world| !world.log.lock().unwrap().request_ids(0).is_empty())
        .await;
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
    world
        .until(|world| {
            world
                .log
                .lock()
                .unwrap()
                .saw(0, |frame| matches!(frame, Frame::Cancel))
        })
        .await;
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
