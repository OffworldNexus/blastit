//! Ids, isolation and body plumbing: what a view may and may not address
//! (ARCHITECTURE.md §3.1, §3.2).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_frame::Frame;
use common::{Options, Script, World, body_of, head, route, status_of};

fn two_routes() -> String {
    format!("version = 1\n{}{}", route("a", "/a/"), route("b", "/b/"))
}

#[tokio::test]
async fn frames_for_unknown_request_ids_are_dropped() {
    tokio::time::pause();
    let mut world = World::start(
        Options::new()
            .manifest(&two_routes())
            .script(
                0,
                Script::ForeignId {
                    body: b"a".to_vec(),
                },
            )
            .script(
                1,
                Script::Answer {
                    body: b"b".to_vec(),
                },
            ),
    );

    // Route 0's view answers under a request id it was never given, next to
    // its real answer; route 1 is serving a different request at the same
    // time. The bogus frame must reach neither stream.
    world.http.send_request(1, head("GET", "/a/"), b"").await;
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    let first = world.http.response(1).await;
    assert_eq!(status_of(&first), 200);
    assert_eq!(body_of(&first), b"a");
    assert_eq!(first.len(), 3, "only head, body and end: {first:?}");
    let second = world.http.response(2).await;
    assert_eq!(status_of(&second), 200);
    assert_eq!(body_of(&second), b"b");
    assert_eq!(second.len(), 3, "{second:?}");

    // Nothing else made it out of the scheduler.
    assert!(world.http.leftover().is_empty());
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 1]);
    // The request ids are per view, and each view only ever saw its own.
    assert_eq!(world.log.lock().unwrap().request_ids(0), vec![1]);
    assert_eq!(world.log.lock().unwrap().request_ids(1), vec![1]);
}

#[tokio::test]
async fn one_route_never_gets_a_second_process_while_the_first_lives() {
    tokio::time::pause();
    let mut world = World::start(Options::new().manifest(&two_routes()).script(
        0,
        Script::Answer {
            body: b"a".to_vec(),
        },
    ));
    for stream in 1..=4u32 {
        world
            .http
            .send_request(stream, head("GET", "/a/"), b"")
            .await;
        assert_eq!(body_of(&world.http.response(stream).await), b"a");
    }
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);
    assert_eq!(
        world.log.lock().unwrap().request_ids(0),
        vec![1, 2, 3, 4],
        "each request gets its own id on the same view"
    );
}

#[tokio::test]
async fn a_body_is_forwarded_frame_by_frame_and_counted() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    ));
    let body: Vec<u8> = (0..70u32).map(|byte| byte as u8).collect();
    world
        .http
        .send_request(1, head("POST", "/upload"), &body)
        .await;
    assert_eq!(body_of(&world.http.response(1).await), b"ok");

    let event = &world.requests()[0];
    assert_eq!(event.bytes_in(), body.len() as u64);
    assert_eq!(event.bytes_out(), 2);
    assert_eq!(event.outcome(), blastit_events::Outcome::Ok);

    let received = world.log.lock().unwrap().received_by(0);
    let mut forwarded = Vec::new();
    for (_, frame) in received {
        if let Frame::ReqBody(chunk) = frame {
            forwarded.extend_from_slice(&chunk);
        }
    }
    assert_eq!(forwarded, body);
}

#[tokio::test]
async fn a_body_for_an_unknown_stream_is_ignored() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    ));
    // No `REQ_HEAD` ever announced stream 9.
    world.http.send(9, &Frame::ReqBody(b"junk".to_vec())).await;
    world.http.send(9, &Frame::ReqEnd).await;
    world.http.send_request(1, head("GET", "/x"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"ok");
    // The junk never reached a view, and never produced a response.
    assert!(
        world
            .log
            .lock()
            .unwrap()
            .received_by(0)
            .iter()
            .all(|(id, _)| *id == 1)
    );
    assert!(world.http.leftover().is_empty());
    assert_eq!(world.requests().len(), 1);
}

#[tokio::test]
async fn a_response_abort_after_the_head_aborts_the_response() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(0, Script::AbortAfterHead));
    world.http.send_request(1, head("GET", "/x"), b"").await;
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert!(common::aborted(&frames), "{frames:?}");

    let event = world.requests()[0].clone();
    assert_eq!(event.outcome(), blastit_events::Outcome::ViewDied);
    assert_eq!(event.status(), 200);

    // `DONE` followed the abort, so the slot is free for the next request.
    world.http.send_request(2, head("GET", "/x"), b"").await;
    let frames = world.http.awaited_response(2).await;
    assert!(common::aborted(&frames), "{frames:?}");
    world.until(|world| world.requests().len() == 2).await;
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);
}

#[tokio::test]
async fn a_response_abort_without_a_head_becomes_a_502() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(0, Script::AbortWithoutHead));
    world.http.send_request(1, head("GET", "/x"), b"").await;
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 502, "{frames:?}");
    assert!(common::ended(&frames));
    assert_eq!(body_of(&frames), b"bad gateway\n");
    assert_eq!(
        world.requests()[0].outcome(),
        blastit_events::Outcome::ViewDied
    );
    assert_eq!(world.requests()[0].status(), 502);
    // `done` followed, so a second request is served by the same process.
    world.http.send_request(2, head("GET", "/x"), b"").await;
    assert_eq!(status_of(&world.http.awaited_response(2).await), 502);
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);
}
