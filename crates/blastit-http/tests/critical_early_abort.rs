//! The body gate on the hot path (`R13`, `R12`, design §4.2 D.1).
//!
//! An upload must never be read to its end when the scheduler has already
//! answered it. These are the protected cases: a declared body over
//! `BLASTIT_MAX_BODY_BYTES` is refused before a byte is read, a scripted
//! ten-mebibyte upload stops the moment the scheduler sends `413` and the
//! socket closes right after that answer, chunked uploads behave the same way,
//! and an ordinary upload still reaches the scheduler intact.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;

use blastit_frame::{Frame, RespHead};
use blastit_http::HttpConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use common::{PEER, QUIET, Rig, find, header_has_token, header_value};

/// A response head with an explicit zero length, so the framing is fixed.
fn empty_head(status: u16) -> Frame {
    Frame::RespHead(RespHead {
        status,
        reason: Vec::new(),
        headers: vec![(b"Content-Length".to_vec(), b"0".to_vec())],
    })
}

/// Reads one whole response from a raw duplex half, until the server closes.
///
/// Bounded: an early answer that never reaches the client (or a connection
/// that waits for the body deadline instead of closing) fails the test rather
/// than hanging it.
async fn read_until_close(io: &mut tokio::io::ReadHalf<tokio::io::DuplexStream>) -> Vec<u8> {
    let mut out = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), io.read_to_end(&mut out))
        .await
        .expect("the early answer must close the connection, not wait for the upload")
        .expect("read the response");
    out
}

/// The status line of a raw response.
fn status_line(raw: &[u8]) -> String {
    let end = find(raw, b"\r\n").unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

#[tokio::test]
async fn a_declared_body_over_the_cap_is_refused_before_it_is_read() {
    // `Content-Length` is one over the cap: no stream is ever opened and no
    // body byte is buffered (`R12`).
    let rig = Rig::new(HttpConfig {
        max_body_bytes: 8,
        ..HttpConfig::default()
    });
    let (mut client, task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 9\r\n\r\n")
        .await;

    let response = client.read_to_end().await;
    assert_eq!(
        response,
        b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "an over-cap declaration is refused up front: {response:?}"
    );
    assert!(
        scheduler.next_within(QUIET).await.is_none(),
        "a refused declaration never becomes a stream"
    );
    let outcome = task.await.expect("the connection must not panic");
    assert!(outcome.is_ok(), "{outcome:?}");
}

#[tokio::test]
async fn a_scripted_ten_mib_upload_stops_at_the_early_413() {
    // A ten-mebibyte upload: the scheduler answers after the first frame, and
    // the front end must stop reading the client socket and close the
    // connection right after the `413` (`R13`).
    const TOTAL: usize = 10 * 1024 * 1024;

    let rig = Rig::new(HttpConfig::default());
    let server = Arc::clone(&rig.server);
    let mut scheduler = rig.scheduler;

    let (server_side, client_side) = tokio::io::duplex(1024 * 1024);
    let conn = tokio::spawn(async move { server.serve_connection(server_side, PEER).await });
    let (mut read, mut write) = tokio::io::split(client_side);

    write
        .write_all(
            format!("POST /upload HTTP/1.1\r\nHost: h\r\nContent-Length: {TOTAL}\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();

    let (id, _) = scheduler.head().await;
    // Answer before a single body byte is read. The client then tries to
    // upload ten mebibytes: the front end must close instead of reading them.
    scheduler.send(id, &empty_head(413)).await;
    scheduler.send(id, &Frame::RespEnd).await;

    let writer = tokio::spawn(async move {
        let chunk = vec![b'x'; 64 * 1024];
        let mut written = 0usize;
        while written < TOTAL {
            match tokio::time::timeout(std::time::Duration::from_secs(5), write.write_all(&chunk))
                .await
            {
                Ok(Ok(())) => written += chunk.len(),
                Ok(Err(_)) | Err(_) => break,
            }
        }
        written
    });

    let response = read_until_close(&mut read).await;
    assert!(
        status_line(&response).starts_with("HTTP/1.1 413 "),
        "{response:?}"
    );
    assert_eq!(
        header_value(&response, "content-length").as_deref(),
        Some("0")
    );
    assert!(
        header_has_token(&response, "connection", "close"),
        "an early answer must close the connection: {response:?}"
    );

    let written = writer.await.expect("the writer must not panic");
    assert!(
        written < TOTAL,
        "the front end read the whole upload after answering it"
    );

    // No body frame may reach the scheduler beyond the credit window already in
    // flight, and no `CANCEL`: the answer was on the wire, so the upload was
    // not read any further.
    let mut forwarded = 0usize;
    while let Some((_, frame)) = scheduler.next_within(QUIET).await {
        match frame {
            Frame::ReqBody(chunk) => forwarded += chunk.len(),
            Frame::Cancel => panic!("an early answer must not be followed by CANCEL"),
            _ => {}
        }
    }
    assert!(
        forwarded <= blastit_http::INITIAL_WINDOW as usize,
        "the front end forwarded more than one credit window after the 413: {forwarded} bytes"
    );

    let outcome = conn.await.expect("the connection must not panic");
    assert!(outcome.is_ok(), "{outcome:?}");
}

#[tokio::test]
async fn a_chunked_upload_stops_at_the_early_413() {
    // The same early answer, with the body framed chunked: the front end stops
    // de-chunking and closes with `Connection: close` (`R13`).
    let rig = Rig::new(HttpConfig::default());
    let server = Arc::clone(&rig.server);
    let mut scheduler = rig.scheduler;

    let (server_side, client_side) = tokio::io::duplex(1024 * 1024);
    let conn = tokio::spawn(async move { server.serve_connection(server_side, PEER).await });
    let (mut read, mut write) = tokio::io::split(client_side);

    write
        .write_all(b"POST /upload HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n")
        .await
        .unwrap();

    let chunk = vec![b'x'; 64 * 1024];
    let mut framed = format!("{:x}\r\n", chunk.len()).into_bytes();
    framed.extend_from_slice(&chunk);
    framed.extend_from_slice(b"\r\n");
    write.write_all(&framed).await.unwrap();

    // Keep trying to upload more: the front end must close rather than read it.
    let writer = tokio::spawn(async move {
        for _ in 0..160 {
            match tokio::time::timeout(std::time::Duration::from_secs(5), write.write_all(&framed))
                .await
            {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => break,
            }
        }
    });

    let (id, _) = scheduler.head().await;
    let (body_id, _) = scheduler.body().await;
    assert_eq!(body_id, id);
    scheduler.send(id, &empty_head(413)).await;
    scheduler.send(id, &Frame::RespEnd).await;

    let response = read_until_close(&mut read).await;
    assert!(
        status_line(&response).starts_with("HTTP/1.1 413 "),
        "{response:?}"
    );
    assert!(
        header_has_token(&response, "connection", "close"),
        "a chunked upload must be closed after the early 413: {response:?}"
    );
    let _ = writer.await;

    while let Some((_, frame)) = scheduler.next_within(QUIET).await {
        assert!(
            !matches!(frame, Frame::Cancel),
            "an early answer must not be followed by CANCEL: {frame:?}"
        );
    }
    let outcome = conn.await.expect("the connection must not panic");
    assert!(outcome.is_ok(), "{outcome:?}");
}

#[tokio::test]
async fn a_chunked_body_over_the_cap_is_413_and_closed() {
    // `R12`: the de-chunker caps the decoded body at `BLASTIT_MAX_BODY_BYTES`.
    // One byte over is a `413` with `Connection: close`.
    let rig = Rig::new(HttpConfig {
        max_body_bytes: 8,
        ..HttpConfig::default()
    });
    let (mut client, task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n")
        .await;
    let (id, _) = scheduler.head().await;
    client.write(b"9\r\n012345678\r\n").await;

    assert_eq!(
        client.read_to_end().await,
        b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "a chunked body one byte over the cap is refused and closed"
    );
    assert_eq!(
        scheduler.until_cancelled().await.0,
        id,
        "the scheduler is told the upload cannot continue"
    );
    let outcome = task.await.expect("the connection must not panic");
    assert!(outcome.is_ok(), "{outcome:?}");
}

#[tokio::test]
async fn a_normal_upload_still_relays_intact() {
    // Nothing early: a body the scheduler accepts arrives in order, and its
    // answer reaches the client unchanged.
    let mut rig = Rig::new(HttpConfig::default());
    let body: Vec<u8> = (0..128 * 1024).map(|index| (index % 251) as u8).collect();

    let (mut client, task) = rig.connect();
    let mut request = format!(
        "POST /upload HTTP/1.1\r\nHost: h\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(&body);
    client.write(&request).await;

    let (id, head) = rig.scheduler.head().await;
    assert_eq!(head.method, b"POST");
    assert_eq!(rig.scheduler.whole_body(id).await, body);
    rig.scheduler.respond(id, 200, &[], b"stored").await;

    let response = client.read_response().await;
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"), "{response:?}");
    assert!(
        response.ends_with(b"\r\n6\r\nstored\r\n0\r\n\r\n"),
        "{response:?}"
    );
    drop(client);
    let outcome = task.await.expect("the connection must not panic");
    assert!(outcome.is_ok(), "{outcome:?}");
}
