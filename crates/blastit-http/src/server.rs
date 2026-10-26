//! Accepting connections and running them (ARCHITECTURE.md §2.3).
//!
//! [`Server`] owns the `http.chan` client that every connection shares and the
//! configuration they read. [`serve`] is the shape init needs: the inherited
//! listener plus the inherited channel. [`serve_with`] and [`Acceptor`] are the
//! same thing without the TCP assumption, which is what the tests use.

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use blastit_events::RejectReason;
use blastit_frame::Limits;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::Emit;
use crate::chan::Chan;
use crate::config::HttpConfig;
use crate::conn::{Conn, emit_rejected};
use crate::error::HttpError;
use crate::response;

/// Something that hands out accepted client connections.
///
/// The front end only needs three things from a listener: the next connection,
/// a byte stream to talk over, and a string to call the client. That is little
/// enough to implement over anything — a `TcpListener`, a pre-queued pair of
/// duplex halves — which is what makes the connection loop testable.
pub trait Acceptor {
    /// The connection type this acceptor produces.
    type Io: AsyncRead + AsyncWrite + Send + Unpin + 'static;

    /// Wait for the next client connection and its address.
    ///
    /// The address is the string the app sees as `remote_addr` until a trusted
    /// `X-Forwarded-For` replaces it. It must be cancellation safe: the accept
    /// loop drops this future whenever the scheduler channel dies.
    ///
    /// # Errors
    ///
    /// An I/O error ends the server, the same way a broken `http.chan` does.
    fn accept(&mut self) -> impl Future<Output = io::Result<(Self::Io, String)>> + Send;
}

/// The listening socket init inherited (`0.0.0.0:$PORT`, §2.1).
pub struct TcpAcceptor {
    listener: TcpListener,
}

impl TcpAcceptor {
    /// Wrap an already-bound, already-listening socket.
    pub fn new(listener: TcpListener) -> Self {
        Self { listener }
    }

    /// The address this listener is bound to.
    ///
    /// # Errors
    ///
    /// Whatever the socket reports, exactly as [`TcpListener::local_addr`].
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }
}

impl Acceptor for TcpAcceptor {
    type Io = TcpStream;

    fn accept(&mut self) -> impl Future<Output = io::Result<(TcpStream, String)>> + Send {
        let listener = &mut self.listener;
        async move {
            let (stream, peer) = listener.accept().await?;
            Ok((stream, peer.to_string()))
        }
    }
}

/// The HTTP/1.1 front end.
///
/// One `http.chan` client, one configuration, one event sink, any number of
/// connections. Everything here is `Send + Sync`, so connections can be served
/// from several tasks.
pub struct Server {
    chan: Chan,
    cfg: HttpConfig,
    emit: Emit,
    /// Connections currently being served, for `max_connections`.
    live: Arc<AtomicUsize>,
}

impl Server {
    /// Build a front end talking over `channel`.
    ///
    /// `channel` is the front end's end of the `http.chan` socketpair; the
    /// scheduler holds the other end. Must be called from inside a Tokio
    /// runtime, because the channel reader and writer are tasks.
    pub fn new<C>(channel: C, cfg: HttpConfig, emit: Emit) -> Self
    where
        C: AsyncRead + AsyncWrite + Send + 'static,
    {
        let chan = Chan::with_stream_buffer(channel, cfg.frame_limits, cfg.stream_buffer_bytes);
        Self {
            chan,
            cfg,
            emit,
            live: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// The configuration this server was built with.
    pub fn config(&self) -> &HttpConfig {
        &self.cfg
    }

    /// The frame size limit of `http.chan`.
    pub fn limits(&self) -> Limits {
        self.chan.limits()
    }

    /// Whether the scheduler end of `http.chan` is gone.
    pub fn is_closed(&self) -> bool {
        self.chan.is_closed()
    }

    /// Serve one already-accepted connection until it ends.
    ///
    /// Returns `Err` only when `http.chan` has failed, which init treats as
    /// fatal. A client that disconnects, or misbehaves, ends the connection and
    /// returns `Ok(())`.
    pub async fn serve_connection<IO>(
        &self,
        io: IO,
        remote_addr: impl Into<String>,
    ) -> Result<(), HttpError>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        Conn::new(io, remote_addr.into(), &self.cfg, &self.emit, &self.chan)
            .run()
            .await
    }

    /// Accept connections forever, serving each one on its own task.
    ///
    /// # Errors
    ///
    /// [`HttpError::ChannelClosed`] as soon as the scheduler end of
    /// `http.chan` goes away, and [`HttpError::Accept`] when the listener
    /// itself fails. There is no graceful stop: init drops the process.
    pub async fn run<L>(&self, mut listener: L) -> Result<(), HttpError>
    where
        L: Acceptor,
    {
        loop {
            let accepted = tokio::select! {
                result = listener.accept() => result,
                () = self.chan.closed() => return Err(HttpError::ChannelClosed),
            };
            match accepted {
                Ok((io, peer)) => {
                    // Above the cap, one more client is still *answered* — a
                    // load balancer that gets a 503 can retry elsewhere, while
                    // one that gets a timeout can only count errors. The
                    // counting happens here so no connection is ever queued.
                    if self.live.fetch_add(1, Ordering::AcqRel) >= self.cfg.max_connections {
                        self.live.fetch_sub(1, Ordering::AcqRel);
                        let emit = self.emit.clone();
                        tokio::spawn(async move { refuse_over_capacity(io, &peer, &emit).await });
                        continue;
                    }
                    let cfg = self.cfg.clone();
                    let emit = self.emit.clone();
                    let chan = self.chan.clone();
                    let live = Arc::clone(&self.live);
                    tokio::spawn(async move {
                        let conn = Conn::new(io, peer, &cfg, &emit, &chan);
                        let _ = conn.run().await;
                        live.fetch_sub(1, Ordering::AcqRel);
                    });
                }
                Err(err) => return Err(HttpError::Accept(err)),
            }
        }
    }
}

/// Answer one connection the server has no room for, and close it.
async fn refuse_over_capacity<IO>(mut io: IO, peer: &str, emit: &Emit)
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    // The refusal names the client the same way every other one does: the
    // peer's address, with its port left in `peer_addr` where `REMOTE_PORT`
    // comes from (CGI).
    emit_rejected(
        emit,
        &blastit_frame::peer_address(peer),
        503,
        RejectReason::Other("too_many_connections".into()),
    );
    let _ = io.write_all(&response::simple(503)).await;
    // Half-close as well, so the client sees the answer end rather than waiting
    // on a connection this server will never use.
    let _ = io.shutdown().await;
}

/// Serve HTTP/1.1 on the inherited listener until `http.chan` fails (§2.3).
///
/// `listener` is the socket init bound and `channel` is the front end's end of
/// `http.chan`. `cfg` comes from the environment (§8.2) and `emit` is the
/// events pipe.
///
/// # Errors
///
/// [`HttpError::ChannelClosed`] when the scheduler end goes away — init treats
/// that as fatal — and [`HttpError::Accept`] when the listener fails.
#[cfg(unix)]
pub async fn serve(
    listener: TcpListener,
    channel: tokio::net::UnixStream,
    cfg: HttpConfig,
    emit: Emit,
) -> Result<(), HttpError> {
    serve_with(TcpAcceptor::new(listener), channel, cfg, emit).await
}

/// [`serve`] with the listener and the channel left generic.
///
/// This is the same function: `serve` only fills in the two types init happens
/// to use. Tests drive it with a fake acceptor and a duplex stream standing in
/// for the scheduler.
///
/// # Errors
///
/// The same as [`serve`].
pub async fn serve_with<L, C>(
    listener: L,
    channel: C,
    cfg: HttpConfig,
    emit: Emit,
) -> Result<(), HttpError>
where
    L: Acceptor,
    C: AsyncRead + AsyncWrite + Send + 'static,
{
    Server::new(channel, cfg, emit).run(listener).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    /// A fake listener handing out pre-queued duplex halves.
    struct FakeAcceptor {
        queued: VecDeque<(tokio::io::DuplexStream, String)>,
    }

    impl Acceptor for FakeAcceptor {
        type Io = tokio::io::DuplexStream;

        fn accept(
            &mut self,
        ) -> impl Future<Output = io::Result<(tokio::io::DuplexStream, String)>> + Send {
            let next = self.queued.pop_front();
            async move {
                match next {
                    Some(connection) => Ok(connection),
                    None => Err(io::Error::other("no more fake connections")),
                }
            }
        }
    }

    #[tokio::test]
    async fn serve_with_accepts_from_a_fake_acceptor() {
        use blastit_frame::{Decoder, Frame, RespHead};

        let (server_side, mut client) = duplex(64 * 1024);
        let (chan_side, mut scheduler) = duplex(64 * 1024);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let emit: Emit = std::sync::Arc::new(move |event| {
            sink.lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push(event);
        });

        let acceptor = FakeAcceptor {
            queued: VecDeque::from([(server_side, "203.0.113.9:1234".to_string())]),
        };
        let cfg = HttpConfig::default();
        let task = tokio::spawn(async move { serve_with(acceptor, chan_side, cfg, emit).await });

        client
            .write_all(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n")
            .await
            .unwrap();

        // The request shows up on `http.chan`…
        let mut decoder = Decoder::new(Limits::default());
        let mut raw = vec![0u8; 4096];
        let (id, frame) = loop {
            let read = scheduler.read(&mut raw).await.unwrap();
            assert_ne!(read, 0, "the front end closed the channel");
            decoder.push(&raw[..read]);
            if let Some(frame) = decoder.next_frame().unwrap() {
                break frame;
            }
        };
        assert!(matches!(&frame, Frame::ReqHead(head) if head.target == b"/"));

        // … and the answer comes back to the client.
        let mut wire = Vec::new();
        Limits::default()
            .encode(
                &Frame::RespHead(RespHead {
                    status: 204,
                    reason: Vec::new(),
                    headers: Vec::new(),
                }),
                id,
                &mut wire,
            )
            .unwrap();
        Limits::default()
            .encode(&Frame::RespEnd, id, &mut wire)
            .unwrap();
        scheduler.write_all(&wire).await.unwrap();

        let mut response = vec![0u8; 64];
        let read = client.read(&mut response).await.unwrap();
        assert!(
            response[..read].starts_with(b"HTTP/1.1 204 No Content\r\n"),
            "{:?}",
            String::from_utf8_lossy(&response[..read])
        );

        // The fake acceptor has nothing left to hand out, so the server ends.
        assert!(matches!(task.await.unwrap(), Err(HttpError::Accept(_))));
        assert!(
            events
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn the_tcp_acceptor_reports_its_address() {
        // A listener bound to port 0 is always available, even in CI.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let acceptor = TcpAcceptor::new(listener);
            assert_ne!(acceptor.local_addr().unwrap().port(), 0);
        });
    }
}
