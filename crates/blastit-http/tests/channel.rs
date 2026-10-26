//! `http.chan` behaviour and the shape init needs: a real listener, a dead
//! scheduler, and connections that outlive one request
//! (ARCHITECTURE.md §2.3, §3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashMap;
use std::time::Duration;

use blastit_events::{Component, Level};
use blastit_frame::{Frame, RespHead};
use blastit_http::{HttpConfig, HttpError};

use common::{Harness, Listening, PEER, QUIET, Rig, find};

/// A response head with nothing but a status.
fn bare_head(status: u16) -> RespHead {
    RespHead {
        status,
        reason: Vec::new(),
        headers: Vec::new(),
    }
}

#[tokio::test]
async fn serve_is_the_shape_init_needs() {
    // `serve` takes exactly what init has: the bound listener and one end of
    // the `http.chan` socketpair.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (http_chan, scheduler_chan) = tokio::net::UnixStream::pair().expect("socketpair");
    let events = common::EventLog::default();
    let task = tokio::spawn(blastit_http::serve(
        listener,
        http_chan,
        HttpConfig::default(),
        events.emit(),
    ));
    let mut scheduler = common::Scheduler::new(scheduler_chan);
    let mut client =
        common::Client::new(tokio::net::TcpStream::connect(addr).await.expect("connect"));

    client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, head) = scheduler.head().await;
    assert_eq!(head.target, b"/");
    assert_eq!(scheduler.end().await, id);
    scheduler.respond(id, 200, &[], b"served").await;
    assert_eq!(
        client.read_response().await,
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nserved\r\n0\r\n\r\n"
    );

    // When the scheduler goes, so does `serve`.
    scheduler.close();
    let outcome = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("serve must return")
        .expect("the task must not panic");
    assert!(
        matches!(outcome, Err(HttpError::ChannelClosed)),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn serve_answers_a_request_over_a_real_listener() {
    let mut listening = Listening::start(HttpConfig::default()).await;
    let mut client = listening.connect().await;

    client
        .write(b"GET /health HTTP/1.1\r\nHost: h\r\n\r\n")
        .await;
    let (id, head) = listening.scheduler.head().await;
    assert_eq!(head.target, b"/health");
    assert!(
        head.remote_addr.starts_with("127.0.0.1:"),
        "a real connection reports its socket peer: {}",
        head.remote_addr
    );
    assert_eq!(listening.scheduler.end().await, id);
    listening.scheduler.respond(id, 200, &[], b"pong").await;

    assert_eq!(
        client.read_response().await,
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\npong\r\n0\r\n\r\n"
    );
    assert!(listening.events.is_empty());
}

#[tokio::test]
async fn a_dead_scheduler_fails_the_next_request_with_502() {
    let listening = Listening::start(HttpConfig::default()).await;
    let mut client = listening.connect().await;
    listening.scheduler.close();

    client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    assert_eq!(
        client.read_to_end().await,
        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );

    // `serve` reports the channel as fatal, which is what init waits for.
    let outcome = tokio::time::timeout(Duration::from_secs(5), listening.task)
        .await
        .expect("serve must return")
        .expect("the server task must not panic");
    assert!(
        matches!(outcome, Err(HttpError::ChannelClosed)),
        "{outcome:?}"
    );
    assert!(
        listening.events.is_empty(),
        "a 502 is not an http.rejected event"
    );
}

#[tokio::test]
async fn a_scheduler_that_dies_mid_response_fails_the_request_with_502() {
    let mut listening = Listening::start(HttpConfig::default()).await;
    let mut client = listening.connect().await;

    client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = listening.scheduler.head().await;
    assert_eq!(listening.scheduler.end().await, id);
    listening.scheduler.close();

    assert_eq!(
        client.read_to_end().await,
        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "an in-flight request is answered, not dropped"
    );
    let outcome = tokio::time::timeout(Duration::from_secs(5), listening.task)
        .await
        .expect("serve must return")
        .expect("the server task must not panic");
    assert!(
        matches!(outcome, Err(HttpError::ChannelClosed)),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_scheduler_that_dies_mid_head_closes_the_connection() {
    let mut listening = Listening::start(HttpConfig::default()).await;
    let mut client = listening.connect().await;

    client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = listening.scheduler.head().await;
    assert_eq!(listening.scheduler.end().await, id);
    // Half a response, then the channel goes away.
    listening
        .scheduler
        .send(id, &Frame::RespHead(bare_head(200)))
        .await;
    listening
        .scheduler
        .send(id, &Frame::RespBody(b"half".to_vec()))
        .await;
    listening.scheduler.close();

    assert_eq!(
        client.read_to_end().await,
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nhalf\r\n",
        "the client keeps what it got, and the chunked body is never finished"
    );
}

#[tokio::test]
async fn every_connection_of_one_server_shares_the_channel() {
    let mut rig = Rig::new(HttpConfig::default());
    let mut clients = Vec::new();
    for index in 0..3 {
        let (mut client, task) = rig.connect();
        client
            .write(format!("GET /{index} HTTP/1.1\r\nHost: h\r\n\r\n").as_bytes())
            .await;
        clients.push((client, task));
    }

    // Collect the id each connection got, by the path it asked for.
    let mut ids: HashMap<usize, u32> = HashMap::new();
    while ids.len() < clients.len() {
        if let (id, Frame::ReqHead(head)) = rig.scheduler.next().await {
            let index: usize = String::from_utf8_lossy(&head.target[1..])
                .parse()
                .expect("target");
            ids.insert(index, id);
        }
    }
    let unique: std::collections::HashSet<u32> = ids.values().copied().collect();
    assert_eq!(unique.len(), clients.len(), "ids are unique: {ids:?}");
    assert!(unique.iter().all(|id| *id != 0), "zero is never handed out");

    let mut scheduler = rig.scheduler;
    for (index, (client, _)) in clients.iter_mut().enumerate() {
        let id = ids[&index];
        scheduler
            .respond(id, 200, &[], format!("/{index}").as_bytes())
            .await;
        let response = client.read_response().await;
        assert!(
            response.ends_with(format!("\r\n/{index}\r\n0\r\n\r\n").as_bytes()),
            "{response:?}"
        );
    }
}

#[tokio::test]
async fn a_client_that_goes_away_mid_response_cancels_the_stream() {
    let rig = Rig::new(HttpConfig::default());
    let (mut client, task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client.write(b"GET /slow HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, id);
    drop(client);

    // The front end notices at the first write and cancels the stream.
    scheduler.send(id, &Frame::RespHead(bare_head(200))).await;
    let cancelled = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match scheduler.next().await {
                (id, Frame::Cancel) => return id,
                (_, Frame::RespEnd) => panic!("the response was finished for a gone client"),
                _ => {}
            }
        }
    })
    .await
    .expect("a CANCEL must arrive");
    assert_eq!(cancelled, id);

    let outcome = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("the connection must end")
        .expect("the task must not panic");
    assert!(outcome.is_ok(), "{outcome:?}");
}

#[tokio::test]
async fn the_events_sink_carries_nothing_that_must_never_be_logged() {
    let mut h = Harness::start(HttpConfig {
        max_body_bytes: 1,
        ..HttpConfig::default()
    });
    h.client
        .write(b"POST /secret?token=abc HTTP/1.1\r\nHost: h\r\nContent-Length: 2\r\n\r\n")
        .await;
    let _ = h.client.read_to_end().await;

    let events = h.events.all();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.component(), Component::Http);
    assert_eq!(event.level(), Level::Info);
    assert_eq!(event.kind().name(), "http.rejected");
    assert!(event.route().is_none());
    assert!(event.request_id().is_none());

    // §8.1: no query string, no header value and no body ever appears.
    let encoded = blastit_events::encode_json(event);
    assert!(
        encoded.contains(r#""remote_addr":"203.0.113.7:54321""#),
        "{encoded}"
    );
    assert!(encoded.contains(r#""status":413"#), "{encoded}");
    assert!(
        encoded.contains(r#""reason":"body_too_large""#),
        "{encoded}"
    );
    assert!(!encoded.contains("token"), "{encoded}");
    assert!(!encoded.contains("Content-Length"), "{encoded}");
    assert!(!encoded.contains("secret"), "{encoded}");
}

#[tokio::test]
async fn a_server_keeps_serving_after_a_client_misbehaves() {
    let mut listening = Listening::start(HttpConfig::default()).await;

    let mut rude = listening.connect().await;
    rude.write(b"NOT-A-REQUEST\r\n\r\n").await;
    assert_eq!(
        rude.read_to_end().await,
        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    assert_eq!(listening.events.rejections().len(), 1);

    let mut polite = listening.connect().await;
    polite.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = listening.scheduler.head().await;
    assert_eq!(listening.scheduler.end().await, id);
    listening
        .scheduler
        .respond(id, 200, &[], b"still here")
        .await;
    let response = polite.read_response().await;
    assert!(response.ends_with(b"0\r\n\r\n"), "{response:?}");
    assert!(find(&response, b"still here").is_some());
}

#[tokio::test]
async fn the_server_hands_out_its_configuration_and_channel_state() {
    let mut rig = Rig::new(HttpConfig {
        client_ip: blastit_frame::ClientIpConfig::new("x-forwarded-for", &[] as &[&str]).unwrap(),
        server_port: 1234,
        ..HttpConfig::default()
    });
    assert_eq!(rig.server.config().server_port, 1234);
    assert!(
        !rig.server
            .config()
            .client_ip
            .trusted
            .contains(&"203.0.113.7".parse().unwrap()),
        "nothing is trusted when the list is empty"
    );
    assert_eq!(rig.server.limits(), rig.server.config().frame_limits);
    assert!(!rig.server.is_closed());

    let (mut client, task) = rig.connect();
    client
        .write(b"GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 9.9.9.9\r\n\r\n")
        .await;
    let (id, head) = rig.scheduler.head().await;
    assert_eq!(head.remote_addr, PEER, "untrusted headers are ignored");
    assert_eq!(head.server_port, 1234);
    assert_eq!(rig.scheduler.end().await, id);
    task.abort();
    assert!(
        rig.scheduler
            .next_within(QUIET)
            .await
            .is_none_or(|(_, frame)| { !matches!(frame, Frame::ReqHead(_)) })
    );
}
