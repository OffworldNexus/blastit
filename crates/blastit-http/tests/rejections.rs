//! Refusals, limits and timeouts: every way a request can be turned away
//! before the scheduler sees it (ARCHITECTURE.md §2.3, §8.1).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use blastit_http::HttpConfig;

use common::{Harness, PEER_IP, QUIET};

/// The status line of a refusal the front end wrote itself.
fn refusal(status: u16, phrase: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status} {phrase}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .into_bytes()
}

/// A front end with the defaults, and one connection to it.
fn harness() -> Harness {
    Harness::start(HttpConfig::default())
}

/// Write `request`, read the refusal, and return the event it produced.
async fn refusal_for(config: HttpConfig, request: &[u8], status: u16, phrase: &str, reason: &str) {
    let mut h = Harness::start(config);
    h.client.write(request).await;
    assert_eq!(
        h.client.read_to_end().await,
        refusal(status, phrase),
        "request {request:?}"
    );
    assert_eq!(
        h.events.rejections(),
        [(PEER_IP.to_string(), status, reason.to_string())],
        "request {request:?}"
    );
    assert!(
        h.scheduler.next_within(QUIET).await.is_none(),
        "a refused request never reaches the scheduler"
    );
}

#[tokio::test]
async fn a_malformed_request_is_400() {
    for request in [
        &b"GET / HTTP/1.1\r\nHost h\r\n\r\n"[..],
        b"GET / HTTP/1.1\r\nBad Header: x\r\n\r\n",
        b"GET  HTTP/1.1\r\nHost: h\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: h\r\nX-Nul: a\x00b\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: h\r\nHeader: a\rb\r\n\r\n",
    ] {
        refusal_for(
            HttpConfig::default(),
            request,
            400,
            "Bad Request",
            "malformed_request",
        )
        .await;
    }
}

#[tokio::test]
async fn a_content_length_and_transfer_encoding_together_is_400() {
    refusal_for(
        HttpConfig::default(),
        b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n",
        400,
        "Bad Request",
        "content_length_conflict",
    )
    .await;
}

#[tokio::test]
async fn an_unsupported_transfer_encoding_is_501() {
    for value in ["gzip", "identity", "chunked, gzip"] {
        let request = format!("POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: {value}\r\n\r\n");
        refusal_for(
            HttpConfig::default(),
            request.as_bytes(),
            501,
            "Not Implemented",
            "transfer_encoding_unsupported",
        )
        .await;
    }
}

#[tokio::test]
async fn a_bad_content_length_is_400() {
    for value in ["", "-1", "abc", "1 2", "1, 1"] {
        let request = format!("POST / HTTP/1.1\r\nHost: h\r\nContent-Length: {value}\r\n\r\n");
        refusal_for(
            HttpConfig::default(),
            request.as_bytes(),
            400,
            "Bad Request",
            "invalid_content_length",
        )
        .await;
    }

    refusal_for(
        HttpConfig::default(),
        b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\n",
        400,
        "Bad Request",
        "duplicate_content_length",
    )
    .await;
}

#[tokio::test]
async fn an_upgrade_request_is_501() {
    for value in ["websocket", "h2c", "HTTP/2.0, websocket"] {
        let request =
            format!("GET / HTTP/1.1\r\nHost: h\r\nConnection: Upgrade\r\nUpgrade: {value}\r\n\r\n");
        refusal_for(
            HttpConfig::default(),
            request.as_bytes(),
            501,
            "Not Implemented",
            "upgrade_unsupported",
        )
        .await;
    }
}

#[tokio::test]
async fn an_unsupported_version_is_505() {
    for request in [
        &b"GET / HTTP/2.0\r\nHost: h\r\n\r\n"[..],
        b"GET / HTTP/0.9\r\nHost: h\r\n\r\n",
        b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n",
    ] {
        refusal_for(
            HttpConfig::default(),
            request,
            505,
            "HTTP Version Not Supported",
            "version_unsupported",
        )
        .await;
    }
}

#[tokio::test]
async fn an_expectation_other_than_100_continue_is_417() {
    refusal_for(
        HttpConfig::default(),
        b"POST / HTTP/1.1\r\nHost: h\r\nExpect: 200-ok\r\nContent-Length: 1\r\n\r\n",
        417,
        "Expectation Failed",
        "expect_unsupported",
    )
    .await;
}

#[tokio::test]
async fn a_head_over_the_limit_is_431() {
    let config = HttpConfig {
        max_header_bytes: 128,
        ..HttpConfig::default()
    };
    let mut request = b"GET / HTTP/1.1\r\nHost: h\r\n".to_vec();
    for index in 0..10 {
        request.extend_from_slice(format!("X-Pad-{index}: {}\r\n", "p".repeat(20)).as_bytes());
    }
    request.extend_from_slice(b"\r\n");

    refusal_for(
        config,
        &request,
        431,
        "Request Header Fields Too Large",
        "header_too_large",
    )
    .await;
}

#[tokio::test]
async fn a_head_that_never_finishes_is_408() {
    let config = HttpConfig {
        header_timeout: Duration::from_millis(60),
        ..HttpConfig::default()
    };
    let mut h = Harness::start(config);
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n").await;
    assert_eq!(
        h.client.read_to_end().await,
        refusal(408, "Request Timeout")
    );
    assert_eq!(
        h.events.rejections(),
        [(PEER_IP.to_string(), 408, "header_timeout".to_string())]
    );
    assert!(h.scheduler.next_within(QUIET).await.is_none());
}

#[tokio::test]
async fn a_slowloris_that_sends_nothing_is_closed_silently() {
    let config = HttpConfig {
        header_timeout: Duration::from_millis(60),
        ..HttpConfig::default()
    };
    let mut h = Harness::start(config);
    // No byte at all: there is nothing to answer, so the connection just ends.
    assert!(h.client.read_within(Duration::from_secs(2)).await == Some(Vec::new()));
    assert!(h.events.is_empty(), "an idle connection is not a rejection");
    let outcome = h.task.await.expect("the connection task must end");
    assert!(outcome.is_ok(), "{outcome:?}");
}

#[tokio::test]
async fn a_head_that_never_finishes_after_a_first_request_is_408() {
    let config = HttpConfig {
        header_timeout: Duration::from_millis(60),
        ..HttpConfig::default()
    };
    let mut h = Harness::start(config);
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 200, &[], b"ok").await;
    let _ = h.client.read_response().await;

    // The second request starts and then stops.
    h.client.write(b"GET /again HTTP/1.1\r\n").await;
    assert_eq!(
        h.client.read_to_end().await,
        refusal(408, "Request Timeout")
    );
    assert_eq!(
        h.events.rejections(),
        [(PEER_IP.to_string(), 408, "header_timeout".to_string())]
    );
}

#[tokio::test]
async fn an_idle_keep_alive_connection_is_closed_silently() {
    let config = HttpConfig {
        keepalive_timeout: Duration::from_millis(60),
        ..HttpConfig::default()
    };
    let mut h = Harness::start(config);
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 200, &[], b"ok").await;
    let _ = h.client.read_response().await;

    let started = Instant::now();
    assert_eq!(
        h.client.read_within(Duration::from_secs(2)).await,
        Some(Vec::new()),
        "the connection is closed without a word"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(60),
        "the keep-alive bound was waited for"
    );
    assert!(h.events.is_empty());
    assert!(h.scheduler.next_within(QUIET).await.is_none());
}

#[tokio::test]
async fn a_request_read_in_pieces_is_accepted() {
    let mut h = harness();
    let request = b"GET /piecemeal HTTP/1.1\r\nHost: h\r\nX-A: 1\r\n\r\n";
    for byte in request {
        h.client.write(&[*byte]).await;
    }
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.target, b"/piecemeal");
    assert_eq!(head.headers.len(), 2);
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 200, &[], b"ok").await;
    assert!(
        h.client
            .read_response()
            .await
            .starts_with(b"HTTP/1.1 200 OK\r\n")
    );
    assert!(h.events.is_empty());
}

#[tokio::test]
async fn a_head_too_large_for_one_frame_is_431() {
    // The channel's frames cannot even carry the request head: the request is
    // refused before it is written, and nothing reaches the scheduler.
    refusal_for(
        HttpConfig {
            frame_limits: blastit_frame::Limits::new(20).expect("20 is a usable frame size"),
            ..HttpConfig::default()
        },
        b"GET / HTTP/1.1\r\nHost: h\r\n\r\n",
        431,
        "Request Header Fields Too Large",
        "header_too_large",
    )
    .await;
}

#[tokio::test]
async fn a_bare_lf_in_the_head_is_400() {
    for request in [
        // The request line itself.
        &b"GET / HTTP/1.1\nHost: h\r\n\r\n"[..],
        // A header line.
        b"GET / HTTP/1.1\r\nHost: h\nAccept: */*\r\n\r\n",
        // The blank line that ends the head.
        b"GET / HTTP/1.1\r\nHost: h\r\n\n",
        // A body that is only reachable if the head was accepted.
        b"GET / HTTP/1.1\nHost: h\r\nX-A: 1\n\r\n",
    ] {
        refusal_for(
            HttpConfig::default(),
            request,
            400,
            "Bad Request",
            "malformed_request",
        )
        .await;
    }
}

#[tokio::test]
async fn a_body_may_contain_any_bytes_it_likes() {
    // The bare-LF check stops at the end of the head: a body is not a request.
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 7\r\n\r\na\nb\nc\r\n")
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.whole_body(id).await, b"a\nb\nc\r\n");
    h.scheduler.respond(id, 200, &[], b"ok").await;
    let response = h.client.read_response().await;
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"), "{response:?}");
    assert!(h.events.is_empty(), "a body is never a rejection");
}

#[tokio::test]
async fn connect_is_refused_with_501() {
    refusal_for(
        HttpConfig::default(),
        b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n",
        501,
        "Not Implemented",
        "connect_unsupported",
    )
    .await;
}

#[tokio::test]
async fn an_absolute_form_target_is_400() {
    for request in [
        &b"GET http://example.com/x HTTP/1.1\r\nHost: h\r\n\r\n"[..],
        b"GET https://example.com HTTP/1.1\r\nHost: h\r\n\r\n",
        // Authority form, which only `CONNECT` uses.
        b"GET example.com:443 HTTP/1.1\r\nHost: h\r\n\r\n",
    ] {
        refusal_for(
            HttpConfig::default(),
            request,
            400,
            "Bad Request",
            "absolute_form_target",
        )
        .await;
    }
}

#[tokio::test]
async fn a_target_that_is_not_a_path_is_400() {
    for request in [
        &b"GET foo HTTP/1.1\r\nHost: h\r\n\r\n"[..],
        b"GET * HTTP/1.1\r\nHost: h\r\n\r\n",
        b"POST * HTTP/1.1\r\nHost: h\r\nContent-Length: 0\r\n\r\n",
    ] {
        refusal_for(
            HttpConfig::default(),
            request,
            400,
            "Bad Request",
            "invalid_target",
        )
        .await;
    }
}

#[tokio::test]
async fn options_asterisk_is_forwarded_as_it_is() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"OPTIONS * HTTP/1.1\r\nHost: h\r\n\r\n")
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.method, b"OPTIONS");
    assert_eq!(head.target, b"*", "the app decides what to do with it");
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 204, &[], b"").await;
    let _ = h.client.read_head().await;
    assert!(h.events.is_empty());
}

#[tokio::test]
async fn transfer_encoding_on_http_1_0_is_400() {
    for request in [
        &b"POST / HTTP/1.0\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n"[..],
        b"POST / HTTP/1.0\r\nTransfer-Encoding: chunked\r\n\r\n",
        b"POST / HTTP/1.0\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n",
    ] {
        refusal_for(
            HttpConfig::default(),
            request,
            400,
            "Bad Request",
            "transfer_encoding_on_http_1_0",
        )
        .await;
    }
}

#[tokio::test]
async fn an_http_1_1_request_without_a_host_is_400() {
    refusal_for(
        HttpConfig::default(),
        b"GET / HTTP/1.1\r\n\r\n",
        400,
        "Bad Request",
        "missing_host",
    )
    .await;
    refusal_for(
        HttpConfig::default(),
        b"GET / HTTP/1.1\r\nHost: \r\n\r\n",
        400,
        "Bad Request",
        "invalid_host",
    )
    .await;
    refusal_for(
        HttpConfig::default(),
        b"GET / HTTP/1.1\r\nHost: a b\r\n\r\n",
        400,
        "Bad Request",
        "invalid_host",
    )
    .await;
}

#[tokio::test]
async fn more_than_one_host_is_400_on_any_version() {
    for request in [
        &b"GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n"[..],
        b"GET / HTTP/1.0\r\nHost: a\r\nHost: b\r\n\r\n",
    ] {
        refusal_for(
            HttpConfig::default(),
            request,
            400,
            "Bad Request",
            "duplicate_host",
        )
        .await;
    }
}
#[tokio::test]
async fn a_rejection_is_logged_before_it_is_written() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/2.0\r\n\r\n").await;
    // The event is emitted before the response bytes, so seeing the response
    // means the log already has the entry.
    let _ = h.client.read_to_end().await;
    let events = h.events.all();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.component(), blastit_events::Component::Http);
    assert_eq!(event.kind().name(), "http.rejected");
    assert_eq!(event.level(), blastit_events::Level::Info);
    assert!(event.route().is_none());
    assert!(event.request_id().is_none());
}
