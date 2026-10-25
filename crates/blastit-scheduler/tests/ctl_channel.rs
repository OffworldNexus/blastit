//! `UnixCtl`: the real `ctl.chan`, over a socketpair, moving the descriptor of
//! a new `view.chan` with `SCM_RIGHTS` (ARCHITECTURE.md §3, §4).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::fd::OwnedFd;

use blastit_frame::{Ctl, ExitStatus, Frame, Limits};
use blastit_scheduler::{CtlChannel, SchedulerError, UnixCtl};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// A socketpair wrapped as two `UnixCtl` ends.
fn pair() -> (UnixCtl, UnixCtl) {
    let (a, b) = UnixStream::pair().expect("socketpair");
    (UnixCtl::new(a), UnixCtl::new(b))
}

/// Polls `scheduler.recv()` and then drops it, the way the scheduler's
/// `select!` does when another channel is ready. Every poll must be pending.
async fn assert_pending_reads(scheduler: &mut UnixCtl, polls: usize) {
    for _ in 0..polls {
        tokio::select! {
            biased;
            result = scheduler.recv() => panic!("the read completed unexpectedly: {result:?}"),
            () = tokio::task::yield_now() => {}
        }
    }
}

/// Polls `scheduler.recv()`, dropping the future between polls, until it
/// completes.
async fn recv_after_polls(scheduler: &mut UnixCtl, budget: usize) -> (Ctl, Option<OwnedFd>) {
    for _ in 0..budget {
        tokio::select! {
            biased;
            result = scheduler.recv() => return result.expect("no protocol error"),
            () = tokio::task::yield_now() => {}
        }
    }
    panic!("the read never completed within {budget} polls");
}

#[tokio::test]
async fn messages_round_trip_without_descriptors() {
    let (mut init, mut scheduler) = pair();
    let messages = [
        Ctl::Spawn { route: 3 },
        Ctl::Retire {
            route: 3,
            pid: 4242,
            reason: "idle".to_string(),
        },
        Ctl::Exited {
            route: 3,
            pid: 4242,
            status: ExitStatus::Signal(9),
        },
        Ctl::Drain,
    ];
    for message in &messages {
        init.send(message.clone()).await.unwrap();
    }
    for message in &messages {
        let (received, fd) = scheduler.recv().await.unwrap();
        assert_eq!(&received, message);
        assert!(fd.is_none());
    }
}

#[tokio::test]
async fn spawned_carries_a_real_descriptor() {
    let (mut init, mut scheduler) = pair();
    // The "view.chan" pair: init keeps one end in the process and sends the
    // other to the scheduler, exactly as it does after a fork (§4).
    let (scheduler_end, view_end) = UnixStream::pair().expect("view socketpair");
    let fd: OwnedFd = scheduler_end.into_std().unwrap().into();

    init.send_with_fd(
        &Ctl::Spawned {
            route: 7,
            pid: 9001,
        },
        Some(fd),
    )
    .await
    .unwrap();

    let (received, fd) = scheduler.recv().await.unwrap();
    assert_eq!(
        received,
        Ctl::Spawned {
            route: 7,
            pid: 9001
        }
    );
    let fd = fd.expect("SPAWNED must carry a descriptor");

    // It is the same socket: bytes written on one end are read on the other.
    let std_stream = std::os::unix::net::UnixStream::from(fd);
    std_stream.set_nonblocking(true).unwrap();
    let mut received_end = UnixStream::from_std(std_stream).unwrap();
    let (mut view_read, mut view_write) = view_end.into_split();
    view_write.write_all(b"hello").await.unwrap();
    let mut buf = [0u8; 5];
    received_end.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hello");
    // ... in both directions.
    received_end.write_all(b"back").await.unwrap();
    let mut buf = [0u8; 4];
    view_read.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"back");
}

#[tokio::test]
async fn a_spawned_without_a_descriptor_is_a_protocol_error() {
    let (mut init, mut scheduler) = pair();
    init.send(Ctl::Spawned { route: 1, pid: 2 }).await.unwrap();
    match scheduler.recv().await {
        Err(SchedulerError::Protocol(detail)) => {
            assert!(detail.contains("file descriptors"), "{detail}");
        }
        other => panic!("expected a protocol error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_descriptor_on_a_message_that_carries_none_is_a_protocol_error() {
    let (mut init, mut scheduler) = pair();
    let (extra, _keep_alive) = UnixStream::pair().expect("socketpair");
    let fd: OwnedFd = extra.into_std().unwrap().into();
    init.send_with_fd(
        &Ctl::Retire {
            route: 1,
            pid: 2,
            reason: "x".to_string(),
        },
        Some(fd),
    )
    .await
    .unwrap();
    match scheduler.recv().await {
        Err(SchedulerError::Protocol(detail)) => {
            assert!(detail.contains("RETIRE"), "{detail}");
        }
        other => panic!("expected a protocol error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_frame_that_is_not_a_ctl_message_is_refused() {
    let (mut raw_init, scheduler_end) = UnixStream::pair().expect("socketpair");
    let mut scheduler = UnixCtl::new(scheduler_end);
    // `READY` belongs to view.chan, not ctl.chan.
    let mut wire = Vec::new();
    Limits::default()
        .encode(&Frame::Ready, 0, &mut wire)
        .unwrap();
    raw_init.write_all(&wire).await.unwrap();
    match scheduler.recv().await {
        Err(SchedulerError::Protocol(detail)) => assert!(detail.contains("non-ctl"), "{detail}"),
        other => panic!("expected a protocol error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_message_larger_than_the_channel_limit_is_refused() {
    let (mut raw_init, scheduler_end) = UnixStream::pair().expect("socketpair");
    let mut scheduler = UnixCtl::with_limits(scheduler_end, Limits::new(16).unwrap());
    let mut wire = Vec::new();
    Limits::default()
        .encode(
            &Frame::Ctl(Ctl::Retire {
                route: 1,
                pid: 2,
                reason: "x".repeat(64),
            }),
            0,
            &mut wire,
        )
        .unwrap();
    raw_init.write_all(&wire).await.unwrap();
    match scheduler.recv().await {
        Err(SchedulerError::Frame(_)) => {}
        other => panic!("expected a frame error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_closed_peer_is_an_error_not_a_panic() {
    let (mut init, mut scheduler) = pair();
    init.send(Ctl::Drain).await.unwrap();
    drop(init);
    let first = scheduler.recv().await.unwrap();
    assert_eq!(first.0, Ctl::Drain);
    match scheduler.recv().await {
        Err(SchedulerError::Closed) => {}
        other => panic!("expected a closed channel, got {other:?}"),
    }
}

#[tokio::test]
async fn a_cancelled_read_keeps_the_bytes_it_had() {
    // The scheduler polls `recv` inside a `select!`, so the future is dropped
    // whenever anything else is ready. A half-read message must survive that.
    let (mut raw_init, scheduler_end) = UnixStream::pair().expect("socketpair");
    let mut scheduler = UnixCtl::new(scheduler_end);
    let mut wire = Vec::new();
    Limits::default()
        .encode(&Frame::Ctl(Ctl::Drain), 0, &mut wire)
        .unwrap();

    // Three reads with nothing to read: each is dropped mid-flight.
    assert_pending_reads(&mut scheduler, 3).await;
    // A half-written frame: the read that finds the length is dropped too.
    raw_init.write_all(&wire[..4]).await.unwrap();
    assert_pending_reads(&mut scheduler, 3).await;
    // The rest arrives; more dropped reads, and the message is still whole.
    raw_init.write_all(&wire[4..]).await.unwrap();
    let (message, fd) = recv_after_polls(&mut scheduler, 4).await;
    assert_eq!(message, Ctl::Drain);
    assert!(fd.is_none());
}

#[tokio::test]
async fn a_cancelled_read_still_collects_the_descriptor() {
    let (mut init, mut scheduler) = pair();
    let (scheduler_end, _view_end) = UnixStream::pair().expect("view socketpair");
    let fd: OwnedFd = scheduler_end.into_std().unwrap().into();
    assert_pending_reads(&mut scheduler, 3).await;
    init.send_with_fd(&Ctl::Spawned { route: 2, pid: 3 }, Some(fd))
        .await
        .unwrap();
    let (message, fd) = recv_after_polls(&mut scheduler, 4).await;
    assert_eq!(message, Ctl::Spawned { route: 2, pid: 3 });
    assert!(fd.is_some());
}
