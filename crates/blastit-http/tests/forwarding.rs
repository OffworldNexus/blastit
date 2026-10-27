//! Client address, scheme and `Host` derivation (ARCHITECTURE.md §2.3, §8.2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_frame::{ClientIpConfig, Scheme};
use blastit_http::HttpConfig;

use common::{Harness, PEER, PEER_IP};

/// A front end with an unusual `server_port`, so tests can tell it from the
/// port the listener happens to use, whose client-IP header is believed from
/// `trusted`.
fn config(trusted: &[&str]) -> HttpConfig {
    HttpConfig {
        client_ip: ClientIpConfig::new("x-forwarded-for", trusted).unwrap(),
        server_port: 8443,
        ..HttpConfig::default()
    }
}

/// The tests that want the header honoured: the socket peer is the harness's
/// own `PEER`, so the address inside it is the one trusted address.
fn trusting() -> HttpConfig {
    config(&[TRUSTED])
}

/// The address of [`PEER`] without its port: a trusted proxy list names
/// addresses and networks, never ports, and `REMOTE_ADDR` is the same address
/// alone.
const TRUSTED: &str = PEER_IP;

#[tokio::test]
async fn a_trusted_peer_names_the_client_behind_it() {
    let mut h = Harness::start(trusting());
    h.client
        .write(
            b"GET / HTTP/1.1\r\nHost: shop.example:8080\r\nX-Forwarded-For: 198.51.100.9, 203.0.113.7\r\nX-Forwarded-Proto: https\r\n\r\n",
        )
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(
        head.remote_addr, "198.51.100.9",
        "the entry the trusted peer appended (203.0.113.7) is skipped, and the \
         first address it did not write is the client"
    );
    assert_eq!(
        head.peer_addr, PEER,
        "the socket is still reported as the socket"
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
async fn a_forged_entry_on_the_left_never_names_the_client() {
    let mut h = Harness::start(trusting());
    // The client sent "6.6.6.6" itself; the trusted peer appended the address
    // it saw (198.51.100.9) and its own (203.0.113.7). Only the middle entry is
    // something the last hop vouched for.
    h.client
        .write(
            b"GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 6.6.6.6, 198.51.100.9, 203.0.113.7\r\n\r\n",
        )
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.remote_addr, "198.51.100.9");
    assert_ne!(head.remote_addr, "6.6.6.6");
    assert_eq!(h.scheduler.end().await, id);
}

#[tokio::test]
async fn a_client_that_is_not_trusted_cannot_forge_its_own_address() {
    let mut h = Harness::start_from(trusting(), "198.51.100.9:1111");
    // The peer of this connection is not trusted, so its header is not read at
    // all: the client cannot name itself, and cannot choose its scheme.
    h.client
        .write(b"GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 203.0.113.7\r\nX-Forwarded-Proto: https\r\n\r\n")
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(
        head.remote_addr, "198.51.100.9",
        "the peer's port is not part of `REMOTE_ADDR`"
    );
    assert_eq!(head.peer_addr, "198.51.100.9:1111");
    assert_eq!(head.scheme, Scheme::Http, "and not its scheme either");
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 200, &[], b"").await;
    let _ = h.client.read_response().await;
}

#[tokio::test]
async fn forwarded_headers_are_ignored_when_they_are_not_trusted() {
    let mut h = Harness::start(config(&[]));
    h.client
        .write(
            b"GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 198.51.100.9\r\nX-Forwarded-Proto: https\r\n\r\n",
        )
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(
        head.remote_addr, PEER_IP,
        "the socket peer's address, not the header"
    );
    assert_eq!(head.scheme, Scheme::Http);
    assert_eq!(h.scheduler.end().await, id);
}

#[tokio::test]
async fn an_unknown_forwarded_scheme_stays_http() {
    for value in ["ftp", "", "https, http", "HTTP"] {
        let mut h = Harness::start(trusting());
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
    let mut h = Harness::start(trusting());
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
    let mut h = Harness::start(trusting());
    // HTTP/1.1 requires a `Host`; HTTP/1.0 does not, and gets the default.
    h.client.write(b"GET / HTTP/1.0\r\n\r\n").await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.server_name, "localhost");
    assert_eq!(head.server_port, 8443);
    assert_eq!(head.remote_addr, PEER_IP);
    assert_eq!(h.scheduler.end().await, id);
}

#[tokio::test]
async fn an_ipv6_host_keeps_its_literal_and_loses_its_port() {
    let mut h = Harness::start(trusting());
    h.client
        .write(b"GET / HTTP/1.1\r\nHost: [2001:db8::1]:8080\r\n\r\n")
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.server_name, "2001:db8::1");
    assert_eq!(h.scheduler.end().await, id);
}

#[tokio::test]
async fn a_refused_request_names_the_forwarded_client() {
    let mut h = Harness::start(trusting());
    h.client
        .write(
            b"GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 198.51.100.9, 203.0.113.7\r\nUpgrade: h2c\r\n\r\n",
        )
        .await;
    let _ = h.client.read_to_end().await;
    assert_eq!(
        h.events.rejections(),
        [(
            "198.51.100.9".to_string(),
            501,
            "upgrade_unsupported".to_string()
        )]
    );
}

#[tokio::test]
async fn a_rejection_without_headers_names_the_peer() {
    let mut h = Harness::start(trusting());
    // The request line is fine, the header block is not, so there is nothing
    // to derive an address from.
    h.client
        .write(b"GET / HTTP/1.1\r\nBad Header: x\r\n\r\n")
        .await;
    let _ = h.client.read_to_end().await;
    assert_eq!(
        h.events.rejections(),
        [(PEER_IP.to_string(), 400, "malformed_request".to_string())]
    );
}

#[tokio::test]
async fn one_connection_carries_the_default_scheme_and_the_peer() {
    let mut h = Harness::start(HttpConfig::default());
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.scheme, Scheme::Http);
    assert_eq!(head.remote_addr, PEER_IP);
    assert_eq!(head.server_port, 8080, "the §8.2 default of PORT");
    assert_eq!(h.scheduler.end().await, id);
}
