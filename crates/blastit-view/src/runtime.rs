//! The worker side of the runtime seam (design `docs/design/backend-runtimes.md`
//! §4.1, `ADR-0009`).
//!
//! A route worker process runs exactly one [`WorkerRuntime`]: the thing that
//! executes one route's application code, whatever language that is, on the
//! `view.chan` transport. On the scheduler's side of the same seam,
//! `blastit_backend::Upstream` (`ADR-0009`)
//! decides *when* to dispatch and how to charge the result; a
//! [`WorkerRuntime`] only answers a request once it has been dispatched.
//!
//! The two sides are deliberately independent types, in crates whose edges
//! (§4.2) do not meet: `blastit-view` depends on `blastit-frame` and nothing
//! that parses HTTP or emits events, and the scheduler holds framed sessions,
//! never a trait object of this one. What they share is the vocabulary —
//! [`blastit_frame::ReqHead`], [`RespHead`] and [`DoneOutcome`] are the wire's
//! own types, so a runtime cannot invent a second, subtly different request
//! shape that dispatch would then have to translate (the mismatch §4.1's review
//! closed).
//!
//! # What a runtime may be
//!
//! Threaded CPython over PyO3, NTS `libphp` behind a shim, and any future
//! in-process runtime are all implementations of [`WorkerRuntime`]. That is why
//! the trait's surface names **no language and no socket**: an interpreter
//! handle, a `libphp` pointer or a socket type in these signatures would leak
//! one implementation's private business into the contract every other
//! implementation has to satisfy. A runtime reaches the client through
//! [`RequestIo`], whose methods are the `view.chan` framing — the `wire`
//! transport and `flow` credit windows of `ARCHITECTURE.md` §2.5 and §3.1 —
//! and through nothing else.
//!
//! # One request, start to finish
//!
//! The transport's request thread builds a [`RequestIo`] for one `REQ_HEAD`,
//! calls [`WorkerRuntime::serve`], and then writes the frames that close the
//! request from the [`RequestEnd`] it gets back:
//!
//! ```text
//! serve(req, io)
//!   ├─ io.head()                    what the scheduler sent
//!   ├─ io.read_body(..)             paced by the request-body window
//!   ├─ io.write_head(RespHead)      at most once
//!   ├─ io.write_body(..)            paced by the response window
//!   ├─ io.output(..)                the runtime's stdout/stderr, never the response
//!   └─ io.flush_response()          everything written is on the wire
//! returns RequestEnd { outcome, cpu_ns }
//!   └─ the caller sends RESP_END (+ RESP_END_COST) and DONE
//! ```
//!
//! The runtime never sends `DONE` itself: it does not know the request's id on
//! the channel, and the frame must still be sent when a request panics
//! (`SlotGuard`, in `blastit-view-python`'s `serve`, owns that). The caller
//! which can see both is the one that closes the request.

use std::io;

use blastit_frame::{DoneOutcome, ReqHead, RespHead};

use crate::stream::OutputStream;

/// What a worker runtime is, from the transport's point of view.
///
/// One implementation per language and linkage: threaded CPython
/// (`WsgiRuntime`), NTS `libphp` (`PhpEmbedRuntime`), and whatever a later
/// backend embeds. The scheduler-side `Upstream` implemented by `MB.9.3`
/// consumes this trait and serves both `Executor::View` and `Executor::Php`
/// without branching on [`kind`](WorkerRuntime::kind) in request dispatch; the
/// two provider kinds differ in ownership, `SeccompRole` and `max_inflight`,
/// not in the request path. Wiring the scheduler crate to `blastit-view` is
/// `MB.9.3`'s decision, not this node's.
///
/// # Implementing it from another crate
///
/// The trait is the seam, not a private convenience: an implementation written
/// outside this crate has everything it needs here, and this example is
/// compiled and run as its own crate by `cargo test`:
///
/// ```
/// use std::io;
///
/// use blastit_frame::{DoneOutcome, ReqHead, RespHead};
/// use blastit_view::{OutputStream, RequestEnd, RequestIo, WorkerRuntime};
///
/// /// A runtime that echoes the request body back, without an interpreter.
/// struct Echo;
///
/// impl WorkerRuntime for Echo {
///     fn kind(&self) -> &'static str {
///         "echo"
///     }
///
///     fn max_inflight(&self) -> u32 {
///         1
///     }
///
///     fn serve(&self, req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd {
///         let mut body = Vec::new();
///         let mut buf = [0u8; 1024];
///         loop {
///             match io.read_body(&mut buf) {
///                 Ok(0) | Err(_) => break,
///                 Ok(n) => body.extend_from_slice(&buf[..n]),
///             }
///         }
///         let _ = io.write_head(RespHead::default());
///         let _ = io.write_body(&body);
///         let _ = io.flush_response();
///         let _ = &req;
///         RequestEnd::new(DoneOutcome::Completed, None)
///     }
///
///     fn recycle(&self) -> bool {
///         false
///     }
/// }
///
/// /// What the transport gives one request: here, a recording stand-in for
/// /// `view.chan`.
/// struct Record {
///     head: ReqHead,
/// }
///
/// impl RequestIo for Record {
///     fn head(&self) -> &ReqHead {
///         &self.head
///     }
///
///     fn read_body(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
///         Ok(0)
///     }
///
///     fn write_head(&mut self, _head: RespHead) -> io::Result<()> {
///         Ok(())
///     }
///
///     fn write_body(&mut self, _data: &[u8]) -> io::Result<()> {
///         Ok(())
///     }
///
///     fn output(&mut self, _stream: OutputStream, _line: &str) {}
///
///     fn flush_response(&mut self) -> io::Result<()> {
///         Ok(())
///     }
/// }
///
/// fn main() {
///     // Dispatch holds the implementation as a trait object, never the type.
///     let runtime: Box<dyn WorkerRuntime + Send> = Box::new(Echo);
///     let mut io = Record {
///         head: ReqHead::default(),
///     };
///
///     let end = runtime.serve(ReqHead::default(), &mut io);
///     assert_eq!(end.outcome, DoneOutcome::Completed);
///     assert!(!runtime.recycle());
/// }
/// ```
pub trait WorkerRuntime: Send + Sync + 'static {
    /// `"wsgi"` or `"php"` — for diagnostics and conformance keys, and never
    /// for per-request dispatch.
    ///
    /// The two provider kinds answer the same request path; a scheduler that
    /// matched on this string would have to grow a branch per language, which
    /// is exactly the coupling `ADR-0009`'s dual seam removes.
    fn kind(&self) -> &'static str;

    /// How many requests this worker process runs at once.
    ///
    /// The per-route concurrency the scheduler admits, not a thread count:
    /// threaded WSGI gets `view_worker_cap`, an NTS PHP process gets `1`
    /// because it can run one request per OS process. This is a ceiling, not an
    /// admission queue: the scheduler's `provider_ceiling` must cover
    /// `route_allocation`, and this value is the one each `ViewSet` charges
    /// `inflight` against (`provider_ceiling >= route_allocation`, §4 R4).
    fn max_inflight(&self) -> u32;

    /// Run one dispatched request to completion.
    ///
    /// Blocking, on the transport's request thread, with `&self`: a WSGI
    /// runtime attaches the interpreter for the call, and an NTS PHP runtime
    /// takes its process-wide handle behind a lock. Both are synchronous by
    /// construction — `libphp` is not re-entrant and CPython's GIL is not a
    /// future — so the contract is a call, not an `async fn`. An async caller
    /// wraps it in `spawn_blocking`; the `Send + Sync + 'static` bounds and
    /// [`RequestEnd`] being `Send` are what make that task movable between
    /// threads.
    ///
    /// The response is written through `io` before this returns. The caller —
    /// which knows the request's id on the channel — writes `RESP_END`,
    /// `RESP_END_COST` and `DONE` from the [`RequestEnd`].
    ///
    /// # Errors
    ///
    /// Nothing here is a `Result`: a broken channel is the [`RequestIo`]'s to
    /// report on each call, and a request that fails is still a request whose
    /// outcome the scheduler has to hear about. A transport error the runtime
    /// sees while writing is folded into the returned
    /// [`outcome`](RequestEnd::outcome).
    fn serve(&self, req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd;

    /// Whether the worker must exit now instead of taking another request.
    ///
    /// Set by `max_requests` being reached, by a fatal Zend bailout that left
    /// the interpreter unusable, or by anything else the runtime knows it
    /// cannot come back from. The transport retires the process after
    /// [`serve`](WorkerRuntime::serve) returns; the scheduler respawns lazily.
    fn recycle(&self) -> bool;
}

/// How one dispatched request ended.
///
/// The same two facts `RESP_END`/`RESP_END_COST` and `DONE` carry: how the
/// request ended — completed, abandoned, errored, or bailed out of a handler
/// that never returned (`DoneOutcome::Bailed`, `MB.3.41`) — and what the
/// request cost in CPU time. The wall time on the wire is the transport's
/// business, and the scheduler ignores it (`R18`), so it is not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestEnd {
    /// What to report in `DONE`, which is what frees the scheduler's slot.
    pub outcome: DoneOutcome,
    /// The request's CPU time, or `None` when the runtime could not measure
    /// it.
    ///
    /// Absent is not zero: a measurement of zero is a claim about the request,
    /// and a runtime with no readable clock (`CLOCK_THREAD_CPUTIME_ID` for
    /// WSGI, `CLOCK_PROCESS_CPUTIME_ID` for embedded PHP, §4.3) makes no claim
    /// at all. The scheduler charges a `None` as its kernel floor, never as
    /// zero (`ADR-0009`).
    pub cpu_ns: Option<u64>,
}

impl RequestEnd {
    /// A request that ended with `outcome` and cost `cpu_ns`.
    #[must_use]
    pub const fn new(outcome: DoneOutcome, cpu_ns: Option<u64>) -> Self {
        Self { outcome, cpu_ns }
    }
}

/// The transport, as one request sees it: the `view.chan` framing and nothing
/// underneath it.
///
/// Implemented by the transport that owns the channel; used by
/// [`WorkerRuntime::serve`]. Every method is a frame or a flow-control
/// operation on `view.chan` — `REQ_BODY`/`WINDOW` for the body, `RESP_HEAD`,
/// `RESP_BODY` and the window for the response — so a runtime never holds a
/// socket, a file descriptor or a scheduler handle, and cannot write to another
/// request's stream.
///
/// The methods are `&mut self` because a request owns its stream for its
/// lifetime; the implementation is free to share the process-wide writer behind
/// it, as `view.chan` requires one writer per frame.
pub trait RequestIo {
    /// The head the scheduler dispatched.
    ///
    /// The same value `serve` was given; it is here for code that builds a
    /// request environment from the transport's view of the request.
    fn head(&self) -> &ReqHead;

    /// Read request-body bytes.
    ///
    /// Returns the number of bytes read, or `Ok(0)` when the body has ended.
    /// Paced by the request-body credit window (§3.1): a runtime that reads
    /// slowly is granted more credit slowly, so a slow application slows its
    /// own client and no one else. Consuming bytes is what returns the credit.
    ///
    /// # Errors
    ///
    /// The channel could not be read, the stream was cancelled, or the body
    /// exceeded its window.
    fn read_body(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Send the response head.
    ///
    /// At most once per request: the head must precede the body, and a second
    /// head is a protocol error the transport refuses.
    ///
    /// # Errors
    ///
    /// The frame could not be encoded or written, or the stream was cancelled.
    fn write_head(&mut self, head: RespHead) -> io::Result<()>;

    /// Write response-body bytes.
    ///
    /// Paced by the response credit window (§3.1): a runtime that has spent its
    /// credit blocks here until the scheduler grants more, as it would on a
    /// slow client's socket, rather than buffering without bound. A cancelled
    /// stream wakes the write and reports it as an error.
    ///
    /// # Errors
    ///
    /// The channel could not be written, or the stream was cancelled.
    fn write_body(&mut self, data: &[u8]) -> io::Result<()>;

    /// Record one line of the runtime's own output.
    ///
    /// Not the response: this is the runtime's `STDOUT`/`STDERR` —
    /// `wsgi.errors`, PHP's output outside the response — where a traceback or
    /// a Zend notice goes. Implementations keep at most [`crate::OUTPUT_LINE_CAP`]
    /// bytes of each line, so one huge `print` cannot bury the log it lands in,
    /// and they never fail the request for an output error: losing a diagnostic
    /// line is better than failing a response that was otherwise fine.
    fn output(&mut self, stream: OutputStream, line: &str);

    /// Push everything written so far onto the channel.
    ///
    /// Called once, at the end of [`WorkerRuntime::serve`], so the response a
    /// runtime reports is on the wire before the caller closes the request with
    /// `RESP_END`/`DONE`.
    ///
    /// # Errors
    ///
    /// The channel could not be written, or the stream was cancelled.
    fn flush_response(&mut self) -> io::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use blastit_frame::ReqHead;

    /// A runtime that echoes the body back, without a transport behind it.
    struct Echo;

    impl WorkerRuntime for Echo {
        fn kind(&self) -> &'static str {
            "echo"
        }

        fn max_inflight(&self) -> u32 {
            1
        }

        fn serve(&self, _req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd {
            let mut body = Vec::new();
            let mut buf = [0u8; 8];
            loop {
                match io.read_body(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => body.extend_from_slice(&buf[..n]),
                }
            }
            if io.write_head(RespHead::default()).is_err() || io.write_body(&body).is_err() {
                return RequestEnd::new(DoneOutcome::Errored, None);
            }
            io.output(OutputStream::Stderr, "echoed");
            let outcome = match io.flush_response() {
                Ok(()) => DoneOutcome::Completed,
                Err(_) => DoneOutcome::Aborted,
            };
            RequestEnd::new(outcome, Some(7))
        }

        fn recycle(&self) -> bool {
            false
        }
    }

    /// What a request writes, in the order it wrote it.
    #[derive(Debug, PartialEq, Eq)]
    enum Frame {
        Head,
        Body(Vec<u8>),
        Line(OutputStream, String),
        Flush,
    }

    /// A [`RequestIo`] that records frames, with a body supplied up front.
    struct Recorder {
        head: ReqHead,
        body: Vec<u8>,
        frames: Vec<Frame>,
    }

    impl RequestIo for Recorder {
        fn head(&self) -> &ReqHead {
            &self.head
        }

        fn read_body(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.body.len());
            buf[..n].copy_from_slice(&self.body[..n]);
            self.body.drain(..n);
            Ok(n)
        }

        fn write_head(&mut self, _head: RespHead) -> io::Result<()> {
            self.frames.push(Frame::Head);
            Ok(())
        }

        fn write_body(&mut self, data: &[u8]) -> io::Result<()> {
            self.frames.push(Frame::Body(data.to_vec()));
            Ok(())
        }

        fn output(&mut self, stream: OutputStream, line: &str) {
            self.frames.push(Frame::Line(stream, line.to_string()));
        }

        fn flush_response(&mut self) -> io::Result<()> {
            self.frames.push(Frame::Flush);
            Ok(())
        }
    }

    fn assert_send<T: Send>() {}

    #[test]
    fn the_seam_is_object_safe_and_its_result_can_cross_a_thread() {
        // The contract `MB.9.3`/`MB.9.5` need: a `ViewUpstream` holds a boxed
        // runtime and moves the whole thing into a `spawn_blocking` task, so no
        // downstream code can name the concrete type or the language.
        let runtime: Option<Box<dyn WorkerRuntime + Send + Sync>> = None;
        let io: Option<&mut dyn RequestIo> = None;
        assert!(runtime.is_none() && io.is_none());

        assert_send::<RequestEnd>();
        assert_send::<Box<dyn WorkerRuntime + Send + Sync>>();
    }

    #[test]
    fn a_request_writes_head_then_body_then_closes_before_reporting() {
        // The ordering the transport relies on: everything the runtime produces
        // is on the wire by the time `serve` returns, so the caller can close
        // the request with `RESP_END`/`DONE` from the run's `RequestEnd`.
        let runtime: Box<dyn WorkerRuntime + Send> = Box::new(Echo);
        let mut io = Recorder {
            head: ReqHead::default(),
            body: b"hello".to_vec(),
            frames: Vec::new(),
        };

        let end = runtime.serve(ReqHead::default(), &mut io);

        assert_eq!(
            io.frames,
            vec![
                Frame::Head,
                Frame::Body(b"hello".to_vec()),
                Frame::Line(OutputStream::Stderr, "echoed".to_string()),
                Frame::Flush,
            ]
        );
        assert_eq!(end, RequestEnd::new(DoneOutcome::Completed, Some(7)));
        assert!(!runtime.recycle(), "a served request keeps its worker");
        assert_eq!(runtime.kind(), "echo");
        assert_eq!(runtime.max_inflight(), 1);
    }
}
