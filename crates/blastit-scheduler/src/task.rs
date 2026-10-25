//! The per-channel reader and writer tasks.
//!
//! The scheduler itself is one task: it owns the routing decisions, the
//! budgets and the queue, and it must never block on one channel while another
//! has work. So every channel gets two small tasks — one that turns bytes into
//! frames, one that turns frames back into bytes — and the scheduler talks to
//! them over bounded `mpsc` channels. A bounded channel is also the
//! backpressure: a peer that stops reading stops being written to, and a peer
//! that floods stops being read (§2.4, §3.2).

use blastit_frame::{Decoder, Frame, Limits};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

/// Reads frames until the channel closes or a frame does not decode, then
/// reports that with one `close()` message.
pub(crate) async fn read_frames<R, M, Map, Close>(
    mut read: R,
    limits: Limits,
    tx: mpsc::Sender<M>,
    map: Map,
    close: Close,
) where
    R: AsyncRead + Unpin,
    Map: Fn(u32, Frame) -> M,
    Close: Fn() -> M,
{
    let mut decoder = Decoder::with_capacity(limits, 8 * 1024);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read_bytes = match read.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        decoder.push(&buf[..read_bytes]);
        loop {
            match decoder.next_frame() {
                Ok(Some((id, frame))) => {
                    if tx.send(map(id, frame)).await.is_err() {
                        return;
                    }
                }
                Ok(None) => break,
                // The channel can no longer be resynchronised: treat what is
                // left like a close, which is what both peers mean by it.
                Err(_) => {
                    let _ = tx.send(close()).await;
                    return;
                }
            }
        }
    }
    let _ = tx.send(close()).await;
}

/// Writes frames until the scheduler drops its end of the channel.
pub(crate) async fn write_frames<W: AsyncWrite + Unpin>(
    mut write: W,
    limits: Limits,
    mut rx: mpsc::Receiver<(u32, Frame)>,
) {
    let mut wire = Vec::new();
    while let Some((id, frame)) = rx.recv().await {
        wire.clear();
        if limits.encode(&frame, id, &mut wire).is_err() {
            break;
        }
        if write.write_all(&wire).await.is_err() {
            break;
        }
        if write.flush().await.is_err() {
            break;
        }
    }
    let _ = write.shutdown().await;
}
