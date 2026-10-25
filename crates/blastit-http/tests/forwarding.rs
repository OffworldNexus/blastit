//! Client address, scheme and `Host` derivation (ARCHITECTURE.md §2.3, §8.2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_frame::Scheme;
use blastit_http::HttpConfig;

use common::{Harness, PEER};

/// A front end with an unusual `server_port`, so tests can tell it from the
/// port the listener happens to use.
fn config(trust_forwarded: bool) -> HttpConfig {
    HttpConfig {
        trust_forwarded,
        server_port: 8443,
        ..HttpConfig::default()
    }
}

#[tokio::test]
async fn a_trusted_forwarded_for_names_the_real_client() {
    let mut h = Harness::start(config(true));
    h.client
        .write(
            b"GET / HTTP/1.1\r\nHost: shop.example:8080\r\nX-Forwarded-For: 198.51.100.9, 203.0.113.7\r\nX-Forwarded-Proto: https\r\n\r\n",
        )
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(
        head.remote_addr, "203.0.113.7",
        "the last entry is the one the load balancer appended"
    );
    assert_eq!(head.scheme, Scheme::Https);
    assert_eq!(head.server_name, "shop.example", "the port is stripped");
    assert_eq!(
        head.server_port, 8443,
        "the port comes from the configuration"
    );
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 200, &[], b"").await;
    let _ = h.client.read_response().await;
}

#[tokio::test]
async fn forwarded_headers_are_ignored_when_they_are_not_trusted() {
    let mut h = Harness::start(config(false));
    h.client
        .write(
            b"GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 198.51.100.9\r\nX-Forwarded-Proto: https\r\n\r\n",
        )
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.remote_addr, PEER, "the socket peer, not the header");
    assert_eq!(head.scheme, Scheme::Http);
    assert_eq!(h.scheduler.end().await, id);
}

#[tokio::test]
async fn an_unknown_forwarded_scheme_stays_http() {
    for value in ["ftp", "", "https, http", "HTTP"] {
        let mut h = Harness::start(config(true));
        let request = format!("GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-Proto: {value}\r\n\r\n");
        h.client.write(request.as_bytes()).await;
        let (id, head) = h.scheduler.head().await;
        assert_eq!(
            head.scheme,
            Scheme::Http,
            "X-Forwarded-Proto: {value:?} is not https"
        );
        assert_eq!(h.scheduler.end().await, id);
    }
}

#[tokio::test]
async fn forwarded_headers_alone_are_still_forwarded_to_the_app() {
    let mut h = Harness::start(config(true));
    h.client
        .write(b"GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 198.51.100.9\r\n\r\n")
        .await;
    let (id, head) = h.scheduler.head().await;
    assert!(
        head.headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(b"x-forwarded-for")),
        "the header is not hop-by-hop, so the app can still see it"
    );
    assert_eq!(head.remote_addr, "198.51.100.9");
    assert_eq!(h.scheduler.end().await, id);
}

#[tokio::test]
async fn a_missing_host_falls_back_to_localhost_on_http_1_0() {
    let mut h = Harness::start(config(true));
    // HTTP/1.1 requires a `Host`; HTTP/1.0 does not, and gets the default.
    h.client.write(b"GET / HTTP/1.0\r\n\r\n").await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.server_name, "localhost");
    assert_eq!(head.server_port, 8443);
    assert_eq!(head.remote_addr, PEER);
    assert_eq!(h.scheduler.end().await, id);
}

#[tokio::test]
async fn an_ipv6_host_keeps_its_literal_and_loses_its_port() {
    let mut h = Harness::start(config(true));
    h.client
        .write(b"GET / HTTP/1.1\r\nHost: [2001:db8::1]:8080\r\n\r\n")
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.server_name, "2001:db8::1");
    assert_eq!(h.scheduler.end().await, id);
}

#[tokio::test]
async fn a_refused_request_names_the_forwarded_client() {
    let mut h = Harness::start(config(true));
    h.client
        .write(
            b"GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 198.51.100.9, 203.0.113.7\r\nUpgrade: h2c\r\n\r\n",
        )
        .await;
    let _ = h.client.read_to_end().await;
    assert_eq!(
        h.events.rejections(),
        [(
            "203.0.113.7".to_string(),
            501,
            "upgrade_unsupported".to_string()
        )]
    );
}

#[tokio::test]
async fn a_rejection_without_headers_names_the_peer() {
    let mut h = Harness::start(config(true));
    // The request line is fine, the header block is not, so there is nothing
    // to derive an address from.
    h.client
        .write(b"GET / HTTP/1.1\r\nBad Header: x\r\n\r\n")
        .await;
    let _ = h.client.read_to_end().await;
    assert_eq!(
        h.events.rejections(),
        [(PEER.to_string(), 400, "malformed_request".to_string())]
    );
}

#[tokio::test]
async fn one_connection_carries_the_default_scheme_and_the_peer() {
    let mut h = Harness::start(HttpConfig::default());
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.scheme, Scheme::Http);
    assert_eq!(head.remote_addr, PEER);
    assert_eq!(head.server_port, 8080, "the §8.2 default of PORT");
    assert_eq!(h.scheduler.end().await, id);
}
