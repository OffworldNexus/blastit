//! Routing: the health path, `404`, `SCRIPT_NAME`, percent decoding, and the
//! `request` event that records the decision (ARCHITECTURE.md §2.4, §6, §8.1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_events::Outcome;
use blastit_frame::{Frame, ReqHead, Scheme};
use common::{Options, Script, World, body_of, ended, head, route, status_of};

#[tokio::test]
async fn health_path_is_answered_by_the_scheduler() {
    tokio::time::pause();
    let mut world = World::start(Options::new());
    let mut head_frame = head("GET", "/_blastit/health");
    head_frame.remote_addr = "198.51.100.9".to_string();
    world.http.send_head(1, head_frame).await;
    world.http.send(1, &Frame::ReqEnd).await;

    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"ok");
    assert!(ended(&frames));

    // No view was involved, and the event says so.
    world.until(|world| !world.requests().is_empty()).await;
    let events = world.requests();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].path(), "/_blastit/health");
    assert_eq!(events[0].status(), 200);
    assert_eq!(events[0].outcome(), Outcome::Ok);
    assert_eq!(events[0].rule(), None);
    assert_eq!(events[0].remote_addr(), "198.51.100.9");
    assert_eq!(
        world.events.lock().unwrap().last().unwrap().route(),
        None,
        "health is not a route"
    );
    assert!(world.log.lock().unwrap().spawns.is_empty());
}

#[tokio::test]
async fn health_path_respects_a_custom_path() {
    tokio::time::pause();
    let mut options = Options::new();
    options.cfg.health_path = "/healthz".to_string();
    let mut world = World::start(options);
    world
        .http
        .send_request(1, head("GET", "/healthz"), b"")
        .await;
    assert_eq!(status_of(&world.http.response(1).await), 200);
    // The default path is now just another request for the catch-all route.
    world
        .http
        .send_request(2, head("GET", "/_blastit/health"), b"")
        .await;
    let frames = world.http.response(2).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(
        body_of(&frames),
        b"hello",
        "the view answered, not the scheduler"
    );
}

#[tokio::test]
async fn unmatched_request_gets_the_schedulers_404() {
    tokio::time::pause();
    let mut world = World::start(
        Options::new().manifest(&format!("version = 1\n{}", route("about", "/about/"))),
    );
    world
        .http
        .send_request(7, head("GET", "/elsewhere"), b"")
        .await;

    let frames = world.http.response(7).await;
    assert_eq!(status_of(&frames), 404);
    assert!(ended(&frames));

    world.until(|world| !world.requests().is_empty()).await;
    let events = world.requests();
    assert_eq!(events[0].status(), 404);
    assert_eq!(events[0].outcome(), Outcome::Unmatched);
    assert_eq!(events[0].path(), "/elsewhere");
    assert_eq!(events[0].rule(), None);
    assert_eq!(
        world.events.lock().unwrap().last().unwrap().route(),
        None,
        "an unmatched request has no route"
    );
    // Nothing matched, so nothing was ever forked.
    assert!(world.log.lock().unwrap().spawns.is_empty());
}

#[tokio::test]
async fn requests_are_routed_by_path_and_method_and_rule() {
    tokio::time::pause();
    let manifest = format!(
        "version = 1\n{}{}{}",
        "[[route]]\nname = \"product\"\npath = ['/products/[0-9]+/', '/products/[0-9]+/x']\nmethods = [\"GET\"]\n",
        route("fallback", ".*"),
        "",
    );
    let mut world = World::start(
        Options::new()
            .manifest(&manifest)
            .script(
                0,
                Script::Answer {
                    body: b"product".to_vec(),
                },
            )
            .script(
                1,
                Script::Answer {
                    body: b"fallback".to_vec(),
                },
            ),
    );

    // First rule, GET: route 0, rule 0.
    world
        .http
        .send_request(1, head("GET", "/products/42/"), b"")
        .await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"product");

    // Second rule of the same route.
    world
        .http
        .send_request(2, head("GET", "/products/42/x"), b"")
        .await;
    assert_eq!(body_of(&world.http.response(2).await), b"product");

    // The methods list is exact: POST does not match route 0.
    world
        .http
        .send_request(3, head("POST", "/products/42/"), b"")
        .await;
    assert_eq!(body_of(&world.http.response(3).await), b"fallback");

    world.until(|world| world.requests().len() == 3).await;
    let events = world.requests();
    assert_eq!(events[0].rule(), Some(0));
    assert_eq!(events[1].rule(), Some(1));
    assert_eq!(events[2].rule(), Some(0));
    let routes: Vec<Option<String>> = world.request_routes();
    assert_eq!(
        routes,
        vec![
            Some("product".to_string()),
            Some("product".to_string()),
            Some("fallback".to_string())
        ]
    );
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 1]);
    // The view sees the request ids the scheduler allocated for it, starting
    // from its own counter rather than http's stream ids.
    assert_eq!(world.log.lock().unwrap().request_ids(0), vec![1, 2]);
}

#[tokio::test]
async fn script_name_is_stripped_before_routing() {
    tokio::time::pause();
    let mut options = Options::new()
        .manifest(&format!("version = 1\n{}", route("about", "/about/")))
        .script(
            0,
            Script::Answer {
                body: b"about".to_vec(),
            },
        );
    options.cfg.script_name = Some("/app".to_string());
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head("GET", "/app/about/"), b"")
        .await;
    assert_eq!(body_of(&world.http.response(1).await), b"about");

    // The mount point itself becomes `/`, which the catch-all here does not
    // match: this route only answers `/about/`.
    world.http.send_request(2, head("GET", "/app"), b"").await;
    assert_eq!(status_of(&world.http.response(2).await), 404);

    // Not a segment boundary: `/app` must not strip from `/application`.
    world
        .http
        .send_request(3, head("GET", "/application/about/"), b"")
        .await;
    assert_eq!(status_of(&world.http.response(3).await), 404);

    world.until(|world| world.requests().len() == 3).await;
    let events = world.requests();
    // `path` is the target without its query string, not the stripped path.
    assert_eq!(events[0].path(), "/app/about/");
    assert_eq!(events[0].outcome(), Outcome::Ok);
    assert_eq!(events[1].outcome(), Outcome::Unmatched);
    assert_eq!(events[2].outcome(), Outcome::Unmatched);
}

#[tokio::test]
async fn percent_decoding_keeps_the_view_and_its_target_intact() {
    tokio::time::pause();
    let mut world = World::start(
        Options::new()
            .manifest(&format!("version = 1\n{}", route("slashed", "/a/b")))
            .script(
                0,
                Script::Answer {
                    body: b"ok".to_vec(),
                },
            ),
    );

    // `%2F` decodes to a slash, so it matches `/a/b`.
    world
        .http
        .send_request(7, head("GET", "/a%2Fb?x=1"), b"")
        .await;
    assert_eq!(body_of(&world.http.response(7).await), b"ok");
    // ... while the view is forwarded the target verbatim, query string and
    // all, because it rebuilds its own environ (§3.2).
    world
        .until(|world| !world.log.lock().unwrap().received.is_empty())
        .await;
    let (id, frame) = world.log.lock().unwrap().received_by(0)[0].clone();
    match frame {
        Frame::ReqHead(head) => {
            assert_eq!(head.target, b"/a%2Fb?x=1");
            assert_eq!(head.method, b"GET");
        }
        other => panic!("expected a request head, got {other:?}"),
    }
    // http's stream 7 became this view's request 1 (§3.1 rewrites ids).
    assert_eq!(id, 1);
}

#[tokio::test]
async fn bad_targets_and_methods_are_400s() {
    tokio::time::pause();
    let mut world = World::start(
        Options::new()
            .manifest(&format!("version = 1\n{}", route("app", ".*")))
            .script(
                0,
                Script::Answer {
                    body: b"ok".to_vec(),
                },
            ),
    );

    let cases: Vec<(u32, ReqHead)> = vec![
        // A `%` that is not a byte escape.
        (1, head("GET", "/a%zz")),
        (2, head("GET", "/a%")),
        (3, head("GET", "/a%4")),
        // The decoded path must start with a slash (`OPTIONS *` included).
        (4, head("OPTIONS", "*")),
        (5, head("GET", "relative/path")),
        (6, head("GET", "")),
        // PATH_INFO must be UTF-8 for the regex engine.
        (7, head("GET", "/%ff")),
        // ... and so must the method.
        (
            8,
            ReqHead {
                method: vec![0xff, 0xfe],
                ..head("GET", "/fine")
            },
        ),
    ];
    for (id, request) in &cases {
        world.http.send_head(*id, request.clone()).await;
        world.http.send(*id, &Frame::ReqEnd).await;
    }
    for (id, _) in &cases {
        let frames = world.http.response(*id).await;
        assert_eq!(status_of(&frames), 400, "stream {id}");
        assert!(ended(&frames), "stream {id}");
    }

    world
        .until(|world| world.requests().len() == cases.len())
        .await;
    for event in world.requests() {
        assert_eq!(event.status(), 400);
        assert_eq!(event.outcome(), Outcome::Unmatched);
        assert_eq!(event.rule(), None);
    }
    assert!(world.log.lock().unwrap().spawns.is_empty());
}

#[tokio::test]
async fn request_events_carry_every_field() {
    tokio::time::pause();
    let mut world = World::start(
        Options::new()
            .manifest(&format!(
                "version = 1\n{}",
                "[[route]]\nname = \"upload\"\npath = ['/upload/']\n"
            ))
            .script(
                0,
                Script::Answer {
                    body: b"0123456789".to_vec(),
                },
            ),
    );
    let mut request = head("POST", "/upload/?token=secret");
    request.remote_addr = "198.51.100.9".to_string();
    request.scheme = Scheme::Https;
    world.http.send_request(11, request, b"0123").await;
    let frames = world.http.response(11).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"0123456789");

    world.until(|world| !world.requests().is_empty()).await;
    let event = &world.requests()[0];
    assert_eq!(event.method(), "POST");
    assert_eq!(event.path(), "/upload/", "the query string is never logged");
    assert_eq!(event.rule(), Some(0));
    assert_eq!(event.status(), 200);
    assert_eq!(event.outcome(), Outcome::Ok);
    assert_eq!(event.bytes_in(), 4);
    assert_eq!(event.bytes_out(), 10);
    assert!(event.queued_ms() >= 0.0);
    assert!(event.duration_ms() >= 0.0);
    assert_eq!(event.remote_addr(), "198.51.100.9");

    let lines = world.event_lines();
    let line = lines.last().unwrap();
    assert!(line.contains(r#""event":"request""#), "{line}");
    assert!(line.contains(r#""component":"scheduler""#), "{line}");
    assert!(line.contains(r#""route":"upload""#), "{line}");
    assert!(line.contains(r#""path":"/upload/""#), "{line}");
    assert!(line.contains(r#""outcome":"ok""#), "{line}");
    assert!(line.contains(r#""bytes_in":4"#), "{line}");
    assert!(!line.contains("secret"), "the query string leaked: {line}");

    let id = world
        .events
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .request_id()
        .unwrap()
        .to_string();
    assert_eq!(id.len(), 16, "request id {id:?}");
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{id}");
}

#[tokio::test]
async fn keep_alive_streams_are_independent() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(
        0,
        Script::Answer {
            body: b"hi".to_vec(),
        },
    ));
    // Pipelined requests on one channel, answered independently.
    world.http.send_request(1, head("GET", "/a"), b"").await;
    world.http.send_request(2, head("GET", "/b"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"hi");
    assert_eq!(body_of(&world.http.response(2).await), b"hi");
    world.until(|world| world.requests().len() == 2).await;
    let ids = world.log.lock().unwrap().request_ids(0);
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "each request gets its own view request id");
    assert_eq!(world.requests()[0].outcome(), Outcome::Ok);
}

#[tokio::test]
async fn a_silent_view_holds_the_request_in_flight() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(0, Script::Silent));
    world.http.send_request(1, head("GET", "/a"), b"").await;
    world
        .until(|world| !world.log.lock().unwrap().spawns.is_empty())
        .await;
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
    assert!(
        world.requests().is_empty(),
        "the request is still in flight"
    );
    assert!(
        world
            .log
            .lock()
            .unwrap()
            .saw(0, |frame| matches!(frame, Frame::ReqHead(_))),
        "the view did get the request"
    );
}
