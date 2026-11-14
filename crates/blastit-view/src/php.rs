//! Embedded PHP: [`PhpEmbedRuntime`] over `view.chan` (design §4.1, §4.3,
//! ADR-0010 §1).
//!
//! The PHP worker is an ordinary view process. It runs one [`WorkerRuntime`] —
//! this one — and it is the *only* place a `libphp` symbol is reachable: the
//! crate stays `#![forbid(unsafe_code)]`, and the `dlopen`/`dlsym` and the
//! `extern "C"` sink trampolines live in `blastit-php-sapi`'s `loader`.
//!
//! # One request at a time
//!
//! NTS `libphp` runs one request per OS process, so [`PhpEmbedRuntime::max_inflight`]
//! is `1` and `serve` takes the process-wide handle behind a `Mutex`: the trait
//! is `&self`, and two concurrent calls would corrupt PHP's globals. The
//! scheduler forks `1..=workers` such processes instead of threading one.
//!
//! # The transport is single-threaded, and that is why `Credits` has `try_take`
//!
//! [`serve_forever`] reads `REQ_HEAD`, runs exactly one request, and writes
//! `RESP_END_COST` + `DONE` from the [`RequestEnd`] — the shape of the WSGI
//! loop, without its worker pool. The request body is read in full before PHP
//! starts (the C ABI takes one `body` slice), so during the request the only
//! frames left to arrive are `WINDOW` and `CANCEL`. A response body larger than
//! the initial credit therefore has to *read* while it waits, which is what
//! [`RequestIo::write_body`] does here: it asks the credit window for what it
//! has, and reads the next frame when it has nothing. No second thread, no
//! deadlock.
//!
//! # What a request costs
//!
//! The shim measures `CLOCK_PROCESS_CPUTIME_ID` around
//! `php_request_startup`..`php_request_shutdown` outside PHP userland, so the
//! number this runtime reports cannot be forged by a script (`ADR-0009`: the
//! `"php"` kind is `ProcessExact`). A zero reading is reported as `None` — no
//! claim at all — never as "free".

use std::ffi::CString;
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use blastit_frame::{
    Decoder, DoneOutcome, Frame, HEADER_LEN, Limits, ReqHead, RequestCost, RespHead,
};
use blastit_php_sapi::abi::{OUTCOME_BAILOUT, OUTCOME_OK, RequestV1, ServerVarV1};
use blastit_php_sapi::{LoadError, Loaded, ServerVars, SharedObject, Sink};

use crate::error::ViewError;
use crate::flow::Flow;
use crate::runtime::{OUTPUT_LINE_CAP, OutputStream, RequestEnd, RequestIo, WorkerRuntime};
use crate::sync::lock;
use crate::wire::Writer;

/// How much of the channel is read at once.
const READ_SIZE: usize = 64 * 1024;

/// How much request body is read per `read_body` call.
const BODY_READ: usize = 64 * 1024;

/// What a PHP view process is told about itself; fixed for its lifetime.
#[derive(Debug, Clone)]
pub struct PhpRuntimeConfig {
    /// Absolute path of the script `SCRIPT_FILENAME` is pinned to (`[route.php]
    /// entry`). Never request-derived (ADR-0010 §2).
    pub entry: PathBuf,
    /// The application root PHP sees as `DOCUMENT_ROOT` and `open_basedir`.
    pub document_root: PathBuf,
    /// The route's mount point, reported as `SCRIPT_NAME`; empty at the root.
    pub script_name: String,
    /// The `ZEND_INI_SYSTEM` blob [`blastit_php_sapi::IniPolicy`] rendered.
    pub ini: String,
    /// The view's private scratch directory (`open_basedir` allows it too;
    /// it is the process's `HOME`/`TMPDIR` and PHP's `sys_temp_dir`).
    pub view_tmp: PathBuf,
    /// Requests this process serves before it retires (`max_requests`).
    pub max_requests: u32,
    /// The largest request body this worker buffers, in bytes.
    ///
    /// The same number the route's INI pins as `post_max_size` / upload size.
    /// The C ABI takes one `body` slice, so the whole body is buffered before
    /// `php_request_startup`; without this cap a client could make one worker
    /// allocate without bound (the scheduler's own body limit is the first
    /// line, this is the worker's).
    pub max_body_bytes: usize,
}

impl PhpRuntimeConfig {
    /// Refuses a configuration the worker could not serve, at boot.
    ///
    /// # Errors
    ///
    /// A message naming the field and the fix — never a secret. Everything here
    /// would otherwise become a first-request fault, and the rule is that a PHP
    /// worker fails at boot or not at all.
    pub fn validate(&self) -> Result<(), String> {
        if !self.entry.is_absolute() {
            return Err(format!(
                "entry {} must be an absolute path ([route.php] entry)",
                self.entry.display()
            ));
        }
        if !self.entry.is_file() {
            return Err(format!(
                "entry {} is not a file: SCRIPT_FILENAME is pinned to it",
                self.entry.display()
            ));
        }
        if !self.document_root.is_absolute() || !self.document_root.is_dir() {
            return Err(format!(
                "the application root {} must be an existing absolute directory",
                self.document_root.display()
            ));
        }
        if !self.view_tmp.is_absolute() || !self.view_tmp.is_dir() {
            return Err(format!(
                "view_tmp {} must be an existing absolute directory: it is the worker's \
                 HOME, TMPDIR and PHP's sys_temp_dir",
                self.view_tmp.display()
            ));
        }
        if !self.script_name.is_empty()
            && (!self.script_name.starts_with('/') || self.script_name.ends_with('/'))
        {
            return Err(format!(
                "script_name {:?} must be empty or a mount point like \"/app\"",
                self.script_name
            ));
        }
        if self.ini.is_empty() {
            return Err(
                "the INI lockdown is empty; render it with IniPolicy::new() instead of \
                 inheriting the host's php.ini"
                    .to_string(),
            );
        }
        if self.max_requests == 0 {
            return Err("max_requests must be at least 1".to_string());
        }
        if self.max_body_bytes == 0 {
            return Err(
                "max_body_bytes must be at least 1: the worker buffers the whole body \
                 before PHP runs"
                    .to_string(),
            );
        }
        Ok(())
    }
}

/// Why a worker could not start. Every variant is a boot refusal, never a
/// degraded request.
#[derive(Debug, thiserror::Error)]
pub enum PhpError {
    /// The route's configuration cannot be served.
    #[error("invalid PHP route configuration: {0}")]
    Config(String),

    /// The `libphp`/shim load was refused (see [`LoadError`]).
    #[error(transparent)]
    Load(#[from] LoadError),
}

/// The state of the one PHP process this runtime owns.
struct PhpWorker {
    /// The loaded `libphp` + shim and its vtable.
    handle: Loaded,
    /// Requests served so far, for `max_requests`.
    served: u32,
    /// Set when a request bailed out: the interpreter is suspect, so the worker
    /// retires instead of serving another.
    bailed: bool,
}

/// `[route.php]`'s [`WorkerRuntime`]: NTS `libphp` behind the shim's vtable.
pub struct PhpEmbedRuntime {
    /// The process-wide handle. `Mutex` because the trait's `serve` is `&self`
    /// and `libphp` is not re-entrant.
    worker: Mutex<PhpWorker>,
    /// What every request is built from.
    cfg: PhpRuntimeConfig,
}

impl PhpEmbedRuntime {
    /// Loads `libphp` and the shim, runs `php_module_startup`, and returns the
    /// ready runtime — or a boot refusal.
    ///
    /// # Errors
    ///
    /// [`PhpError::Config`] for a configuration [`PhpRuntimeConfig::validate`]
    /// refuses, and [`PhpError::Load`] for every linkage/ABI refusal. Nothing
    /// here touches the network, and a failure leaves no module started.
    pub fn load(
        libphp: SharedObject,
        shim: SharedObject,
        cfg: PhpRuntimeConfig,
    ) -> Result<Self, PhpError> {
        cfg.validate().map_err(PhpError::Config)?;
        let mut handle = Loaded::load(libphp, shim)?;
        handle.worker_init(&cfg.ini, &cfg.view_tmp)?;
        Ok(Self {
            worker: Mutex::new(PhpWorker {
                handle,
                served: 0,
                bailed: false,
            }),
            cfg,
        })
    }

    /// The host `libphp`'s version string (`"8.4.27"`), for the boot log.
    #[must_use]
    pub fn php_version(&self) -> String {
        lock(&self.worker).handle.php_version().to_owned()
    }
}

impl WorkerRuntime for PhpEmbedRuntime {
    fn kind(&self) -> &'static str {
        "php"
    }

    fn max_inflight(&self) -> u32 {
        1
    }

    fn serve(&self, req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd {
        let mut worker = lock(&self.worker);
        worker.served = worker.served.saturating_add(1);
        let prepared = match Prepared::new(&self.cfg, &req) {
            Ok(prepared) => prepared,
            Err(message) => {
                io.output(OutputStream::Stderr, &message);
                return RequestEnd::new(DoneOutcome::Errored, None);
            }
        };

        // The C ABI takes one body slice, so the whole body is read first. The
        // request-body window (plus the `WINDOW` frames `read_body` sends as it
        // consumes) paces the sender; `max_body_bytes` is the worker's own
        // bound, checked before a byte is appended.
        let body = match read_body(io, self.cfg.max_body_bytes) {
            Ok(body) => body,
            Err(BodyError::TooLarge) => {
                io.output(
                    OutputStream::Stderr,
                    &format!(
                        "php: request body is larger than {} bytes",
                        self.cfg.max_body_bytes
                    ),
                );
                worker.bailed = true;
                return RequestEnd::new(DoneOutcome::Errored, None);
            }
            Err(BodyError::Io(err)) => {
                io.output(
                    OutputStream::Stderr,
                    &format!("php: reading the body: {err}"),
                );
                return RequestEnd::new(DoneOutcome::Aborted, None);
            }
        };

        let entries = prepared.entries();
        let mut sink = PhpSink {
            io,
            headers: Vec::new(),
            header_bytes: 0,
            head_sent: false,
            abort: false,
        };
        let result = worker
            .handle
            .request(&prepared.view(&entries, &body), &mut sink);
        let head_sent = sink.head_sent;
        drop(sink);
        // The contract says `flush_response` runs once, at the end, before the
        // caller closes the request; a flush that fails means the response did
        // not reach the wire.
        let flushed = io.flush_response();

        let (outcome, bailout, cpu_ns) = match &result {
            Ok(result) if result.outcome == OUTCOME_OK && head_sent => {
                (DoneOutcome::Completed, false, cpu(result))
            }
            Ok(result) if result.outcome == OUTCOME_BAILOUT => {
                (DoneOutcome::Errored, true, cpu(result))
            }
            Ok(result) => (DoneOutcome::Errored, false, cpu(result)),
            Err(err) => {
                io.output(OutputStream::Stderr, &format!("php: {err}"));
                (DoneOutcome::Errored, true, None)
            }
        };
        let outcome = if flushed.is_err() {
            DoneOutcome::Aborted
        } else {
            outcome
        };
        worker.bailed |= bailout;
        RequestEnd::new(outcome, cpu_ns)
    }

    fn recycle(&self) -> bool {
        let worker = lock(&self.worker);
        should_recycle(worker.bailed, worker.served, self.cfg.max_requests)
    }
}

/// The worker's retirement rule: a fatal bailout, or `max_requests` served.
///
/// Split out so the rule is testable without a loaded `libphp`: the two
/// reasons the process must exit are the interpreter's state and its request
/// count, and nothing else.
fn should_recycle(bailed: bool, served: u32, max_requests: u32) -> bool {
    bailed || served >= max_requests
}

/// Why [`read_body`] stopped early.
#[derive(Debug)]
enum BodyError {
    /// The body is larger than the worker's budget.
    TooLarge,
    /// The channel failed while reading the body.
    Io(io::Error),
}

/// Reads the whole request body into one buffer, refusing more than `max`.
///
/// The C ABI takes a single `body` slice, so buffering is not optional; this is
/// the one place that bounds it.
fn read_body(io: &mut dyn RequestIo, max: usize) -> Result<Vec<u8>, BodyError> {
    let mut body = Vec::new();
    let mut buf = vec![0u8; BODY_READ];
    loop {
        match io.read_body(&mut buf) {
            Ok(0) => return Ok(body),
            Ok(n) => {
                if body.len().saturating_add(n) > max {
                    return Err(BodyError::TooLarge);
                }
                body.extend_from_slice(&buf[..n]);
            }
            Err(err) => return Err(BodyError::Io(err)),
        }
    }
}

/// The CPU time a request claims, or `None` when the shim could not read the
/// clock (a zero is "no report", never "free").
fn cpu(result: &blastit_php_sapi::abi::ResultV1) -> Option<u64> {
    (result.cpu_ns > 0).then_some(result.cpu_ns)
}

/// Serve one PHP view process on `chan` until the scheduler's end closes.
///
/// Sends `READY` before reading anything, serves one request at a time, and
/// returns when the channel ends, when `runtime.recycle()` says the process
/// must retire (`max_requests`, a fatal bailout), or on a channel/protocol
/// error. The caller owns the process's exit code: a `max_requests` exit is a
/// clean `0`, which is what the supervisor's respawn expects.
///
/// # Errors
///
/// [`ViewError`]: the channel could not be read or written, the bytes are not a
/// frame stream, or the scheduler sent a frame a request may not receive.
pub fn serve_forever(
    mut chan: UnixStream,
    runtime: Arc<dyn WorkerRuntime>,
    limits: Limits,
) -> Result<(), ViewError> {
    let writer = Arc::new(Writer::new(chan.try_clone()?, limits));
    writer.write_frame(&Frame::Ready, 0)?;
    let mut decoder = Decoder::with_capacity(limits, limits.max_frame() + HEADER_LEN);
    let mut buf = vec![0u8; READ_SIZE];

    loop {
        let (id, head) = match read_frame(&mut chan, &mut decoder, &mut buf)? {
            None => return Ok(()),
            Some((id, Frame::ReqHead(head))) if id != 0 => (id, head),
            // A grant for a request that already ended: the scheduler may still
            // be spending its response credit when the last frame went out.
            // There is no window to add it to, so it is dropped.
            Some((_, Frame::Window { .. })) => continue,
            // A cancel for a request that already ended is stale scheduling
            // bookkeeping, not a channel failure: `DONE` already went out, so
            // there is nothing left to abort.
            Some((_, Frame::Cancel)) => continue,
            Some((_, other)) => {
                return Err(ViewError::Protocol(format!(
                    "a PHP view may not receive {} before a REQ_HEAD",
                    crate::frame_kind(&other)
                )));
            }
        };
        let flow = Flow::new();
        let mut io = ChanIo {
            head,
            id,
            writer: &writer,
            flow,
            chan: &mut chan,
            decoder: &mut decoder,
            buf: &mut buf,
            pending: Vec::new(),
            ended: false,
            head_sent: false,
            cancelled: false,
            error: None,
        };
        let request = io.head.clone();
        let started = Instant::now();
        let mut end = runtime.serve(request, &mut io);
        let wall_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        // A runtime that did not read the whole body (a PHP script killed
        // before it looked at it) must not leave `REQ_BODY`/`REQ_END` queued:
        // the next frame on the channel has to be the next `REQ_HEAD`.
        io.drain_body();
        if let Some(err) = io.error.take() {
            return Err(err);
        }
        // A client disconnect is not the view's failure. The scheduler has
        // already abandoned this stream and is waiting for `DONE` to release
        // the slot; the response is reported as aborted and the process goes on
        // to the next request. Only a broken channel ends the loop (above).
        if io.cancelled {
            end.outcome = DoneOutcome::Aborted;
        }
        writer.write_frame(
            &Frame::RespEndCost {
                cost: RequestCost::new(wall_ns, end.cpu_ns),
            },
            id,
        )?;
        writer.write_frame(
            &Frame::Done {
                outcome: end.outcome,
            },
            id,
        )?;
        if runtime.recycle() {
            return Ok(());
        }
    }
}

/// Read one frame from `chan`, or `None` at a clean end of channel.
fn read_frame(
    chan: &mut UnixStream,
    decoder: &mut Decoder,
    buf: &mut [u8],
) -> Result<Option<(u32, Frame)>, ViewError> {
    loop {
        if let Some(frame) = decoder.next_frame()? {
            return Ok(Some(frame));
        }
        match chan.read(buf) {
            Ok(0) => return Ok(None),
            Ok(n) => decoder.push(&buf[..n]),
            Err(err)
                if matches!(
                    err.kind(),
                    ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(err)
                if matches!(
                    err.kind(),
                    ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
                ) =>
            {
                return Ok(None);
            }
            Err(err) => return Err(ViewError::Io(err)),
        }
    }
}

/// One request's view of `view.chan`.
///
/// The response is written through the process's one [`Writer`], one frame at
/// a time. Reads and writes are sequential — one request is in flight — so a
/// write that runs out of credit reads the next `WINDOW` itself.
struct ChanIo<'a> {
    head: ReqHead,
    id: u32,
    writer: &'a Writer,
    flow: Arc<Flow>,
    chan: &'a mut UnixStream,
    decoder: &'a mut Decoder,
    buf: &'a mut [u8],
    /// Request body bytes received but not yet handed to the runtime.
    pending: Vec<u8>,
    /// `REQ_END` arrived.
    ended: bool,
    /// `RESP_HEAD` went out.
    head_sent: bool,
    /// The scheduler sent `CANCEL`: the client is gone. The rest of this
    /// request's response is dropped, but the process keeps serving — a
    /// disconnect must not cost a fork and a `php_module_startup`.
    cancelled: bool,
    /// The first channel/protocol error, reported once `serve` returns: the
    /// runtime has no way to return it, and a broken channel ends the view.
    error: Option<ViewError>,
}

impl ChanIo<'_> {
    /// The next frame, remembering a broken channel as the request's error.
    fn next(&mut self) -> io::Result<(u32, Frame)> {
        loop {
            match self.decoder.next_frame() {
                Ok(Some(frame)) => return Ok(frame),
                Ok(None) => {}
                Err(err) => return Err(self.fail(ViewError::Frame(err))),
            }
            match self.chan.read(self.buf) {
                Ok(0) => {
                    return Err(self.fail(ViewError::Io(io::Error::from(ErrorKind::UnexpectedEof))));
                }
                Ok(n) => self.decoder.push(&self.buf[..n]),
                Err(err)
                    if matches!(
                        err.kind(),
                        ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
                    ) => {}
                Err(err) => return Err(self.fail(ViewError::Io(err))),
            }
        }
    }

    /// Records `err` as the request's outcome and returns the `io::Error` the
    /// runtime sees. The first error wins.
    fn fail(&mut self, err: ViewError) -> io::Error {
        let message = err.to_string();
        self.error.get_or_insert(err);
        io::Error::other(message)
    }

    /// Credits `bytes` of already-received request body back to the scheduler,
    /// refusing more than the window allows.
    fn credit_back(&mut self, bytes: usize) -> io::Result<()> {
        if !self.flow.body_received(bytes) {
            let message = format!("request {} sent more body than its credit window", self.id);
            return Err(self.fail(ViewError::Protocol(message)));
        }
        self.flow.body_granted(bytes);
        let granted = u32::try_from(bytes).unwrap_or(u32::MAX);
        self.writer
            .write_frame(&Frame::Window { bytes: granted }, self.id)
            .map_err(|err| self.fail(err.into()))
    }

    /// Consume whatever request body the runtime left unread, so the next frame
    /// on the channel is the next `REQ_HEAD`.
    ///
    /// A PHP script that dies before reading its input leaves `REQ_BODY` and
    /// `REQ_END` queued; without this the next request's dispatch would read
    /// them as its own and the whole view would fail. The bytes are credited
    /// back as they are dropped, exactly as a reader would, so the scheduler's
    /// sender is never left waiting.
    fn drain_body(&mut self) {
        while !self.ended && self.error.is_none() {
            let (id, frame) = match self.next() {
                Ok(frame) => frame,
                Err(_) => return,
            };
            if id != self.id {
                // A cancel for a request that already ended is stale scheduling
                // bookkeeping; every other foreign frame is a protocol violation.
                if matches!(frame, Frame::Cancel) {
                    continue;
                }
                let message = format!(
                    "a frame for request {id} arrived while request {} was running",
                    self.id
                );
                self.fail(ViewError::Protocol(message));
                return;
            }
            match frame {
                Frame::ReqBody(bytes) => {
                    if self.credit_back(bytes.len()).is_err() {
                        return;
                    }
                }
                Frame::ReqEnd => self.ended = true,
                Frame::Cancel => {
                    // The stream is abandoned; nothing more of its body will be
                    // read, and the next frame is the next request's.
                    let _ = self.cancel();
                    return;
                }
                Frame::Window { bytes } => self.flow.response().grant(bytes),
                other => {
                    let message = format!(
                        "unexpected {} while draining a request body",
                        crate::frame_kind(&other)
                    );
                    self.fail(ViewError::Protocol(message));
                    return;
                }
            }
        }
    }

    /// Records the scheduler's `CANCEL`: the client is gone, so the response is
    /// abandoned but the view stays alive.
    ///
    /// This is deliberately **not** [`ChanIo::fail`]: a cancel is an ordinary
    /// event in a request's life, while `error` means the channel itself is
    /// broken and the process must retire.
    fn cancel(&mut self) -> io::Error {
        self.flow.response().close();
        self.cancelled = true;
        self.ended = true;
        io::Error::other("the scheduler cancelled the request")
    }

    /// Reads one inbound frame and applies it while waiting for response credit.
    fn pump(&mut self) -> io::Result<()> {
        let (id, frame) = self.next()?;
        if id != self.id {
            if matches!(frame, Frame::Cancel) {
                return Ok(());
            }
            return Err(self.fail(ViewError::Protocol(format!(
                "a frame for request {id} arrived while request {} was running",
                self.id
            ))));
        }
        match frame {
            Frame::Window { bytes } => {
                self.flow.response().grant(bytes);
                Ok(())
            }
            Frame::ReqBody(bytes) => self.credit_back(bytes.len()),
            Frame::ReqEnd => {
                self.ended = true;
                Ok(())
            }
            Frame::Cancel => Err(self.cancel()),
            other => Err(self.fail(ViewError::Protocol(format!(
                "unexpected {} during a PHP request",
                crate::frame_kind(&other)
            )))),
        }
    }
}

impl RequestIo for ChanIo<'_> {
    fn head(&self) -> &ReqHead {
        &self.head
    }

    fn read_body(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pending.is_empty() && !self.ended {
            let (id, frame) = self.next()?;
            if id != self.id {
                if matches!(frame, Frame::Cancel) {
                    continue;
                }
                return Err(self.fail(ViewError::Protocol(format!(
                    "a frame for request {id} arrived while request {} was running",
                    self.id
                ))));
            }
            match frame {
                Frame::ReqBody(bytes) => {
                    if !self.flow.body_received(bytes.len()) {
                        return Err(self.fail(ViewError::Protocol(format!(
                            "request {} sent more body than its credit window",
                            self.id
                        ))));
                    }
                    self.pending.extend_from_slice(&bytes);
                }
                Frame::ReqEnd => self.ended = true,
                Frame::Window { bytes } => self.flow.response().grant(bytes),
                Frame::Cancel => return Err(self.cancel()),
                other => {
                    return Err(self.fail(ViewError::Protocol(format!(
                        "unexpected {} while reading a request body",
                        crate::frame_kind(&other)
                    ))));
                }
            }
        }
        let n = buf.len().min(self.pending.len());
        if n == 0 {
            return Ok(0);
        }
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        // Consuming bytes is what returns the credit (§3.1).
        self.flow.body_granted(n);
        let granted = u32::try_from(n).unwrap_or(u32::MAX);
        self.writer
            .write_frame(&Frame::Window { bytes: granted }, self.id)
            .map_err(|err| self.fail(err.into()))?;
        Ok(n)
    }

    fn write_head(&mut self, head: RespHead) -> io::Result<()> {
        // A cancelled client has nowhere to receive a head; the shim is told
        // the write succeeded so it finishes tidily instead of panicking the
        // request path.
        if self.cancelled {
            return Ok(());
        }
        if self.head_sent {
            return Err(self.fail(ViewError::Protocol(
                "RESP_HEAD was written twice for one request".to_string(),
            )));
        }
        self.writer
            .write_frame(&Frame::RespHead(head), self.id)
            .map_err(|err| self.fail(err.into()))?;
        self.head_sent = true;
        Ok(())
    }

    fn write_body(&mut self, data: &[u8]) -> io::Result<()> {
        // Nothing of an abandoned response reaches the channel; the runtime
        // still gets `Ok` so PHP can unwind normally.
        if self.cancelled {
            return Ok(());
        }
        // The head must precede the body (the same rule `RESP_HEAD` documents
        // on the wire); a runtime that writes a body first is a bug, and the
        // scheduler would refuse the frame anyway.
        if !self.head_sent {
            return Err(self.fail(ViewError::Protocol(
                "RESP_BODY before RESP_HEAD".to_string(),
            )));
        }
        let mut sent = 0;
        while sent < data.len() {
            if self.cancelled {
                return Ok(());
            }
            let want = (data.len() - sent).min(self.writer.max_payload());
            match self.flow.response().try_take(want) {
                Some(0) => {
                    if self.cancelled {
                        return Ok(());
                    }
                    return Err(self.fail(ViewError::Protocol(
                        "the response window closed before the body was written".to_string(),
                    )));
                }
                Some(taken) => {
                    let chunk = data[sent..sent + taken].to_vec();
                    self.writer
                        .write_frame(&Frame::RespBody(chunk), self.id)
                        .map_err(|err| self.fail(err.into()))?;
                    sent += taken;
                }
                // No credit yet: read the next WINDOW (or CANCEL).
                None => {
                    if let Err(err) = self.pump() {
                        // A cancel is not a failed write: the client is gone,
                        // and the shim must finish the request tidily rather
                        // than see an aborted output (which would look like a
                        // fatal bailout and retire the worker).
                        if self.cancelled {
                            return Ok(());
                        }
                        return Err(err);
                    }
                }
            }
        }
        Ok(())
    }

    fn output(&mut self, stream: OutputStream, line: &str) {
        // The shim already bounds a log line; the contract is the cap, and one
        // huge `echo` outside the response must not bury the log either.
        let capped = capped_line(line);
        let _ = match stream {
            OutputStream::Stdout => writeln!(io::stdout(), "{capped}"),
            OutputStream::Stderr => writeln!(io::stderr(), "{capped}"),
        };
    }

    fn flush_response(&mut self) -> io::Result<()> {
        // Every frame is written synchronously, so there is nothing buffered:
        // the response is on the wire before `serve` returns.
        Ok(())
    }
}

/// A `ResponseSink` for the shim: headers accumulate until the end-of-head
/// marker, the body streams straight through `RequestIo`, and logs become the
/// worker's stderr.
struct PhpSink<'a> {
    io: &'a mut dyn RequestIo,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// Bytes the accumulated header lines hold, for the cap.
    header_bytes: usize,
    head_sent: bool,
    abort: bool,
}

/// The most response headers one request may send.
///
/// A script in a loop can emit headers without bound; each one is a `Vec` pair
/// held until the end-of-head marker. 256 is far above any real framework and
/// small enough that the buffer is bounded by construction.
const MAX_HEADERS: usize = 256;

/// The most bytes the accumulated response headers may hold.
const MAX_HEADER_BYTES: usize = 64 * 1024;

impl Sink for PhpSink<'_> {
    fn head(&mut self, status: i32, name: Option<&[u8]>, value: &[u8]) -> bool {
        let Some(name) = name else {
            // The end-of-head marker: always sent, always carries the status.
            if self.head_sent {
                return true;
            }
            let status = u16::try_from(status).unwrap_or(500);
            let status = if (100..=999).contains(&status) {
                status
            } else {
                500
            };
            let head = RespHead {
                status,
                reason: Vec::new(),
                headers: std::mem::take(&mut self.headers),
            };
            if self.io.write_head(head).is_err() {
                self.abort = true;
            }
            self.head_sent = true;
            return !self.abort;
        };
        if self.head_sent {
            return true;
        }
        // Cap the buffer: past either limit the response is abandoned rather
        // than grown, and the C side stops sending headers.
        if self.headers.len() >= MAX_HEADERS
            || self
                .header_bytes
                .saturating_add(name.len())
                .saturating_add(value.len())
                > MAX_HEADER_BYTES
        {
            self.abort = true;
            return false;
        }
        self.header_bytes += name.len() + value.len();
        self.headers.push((name.to_vec(), value.to_vec()));
        true
    }

    fn body(&mut self, data: &[u8]) -> bool {
        if !data.is_empty() && self.io.write_body(data).is_err() {
            self.abort = true;
        }
        !self.abort
    }

    fn log(&mut self, _level: i32, line: &str) {
        self.io.output(OutputStream::Stderr, line);
    }
}

/// One request's `RequestV1` and the C strings it borrows.
struct Prepared {
    entry: CString,
    document_root: CString,
    script_name: CString,
    request_uri: CString,
    query_string: Option<CString>,
    method: CString,
    content_type: Option<CString>,
    cookie: Option<CString>,
    vars: ServerVars,
}

impl Prepared {
    /// Builds the request environment from `head` and the route's config.
    ///
    /// The CGI contract, pinned here so the three variables cannot disagree:
    /// `REQUEST_URI` is the target exactly as routed (query string included),
    /// `SCRIPT_NAME` is the route's mount point (`/` at the root), and
    /// `PATH_INFO` is the path with that mount point stripped at a segment
    /// boundary — `/app` + `/app/list` gives `SCRIPT_NAME=/app`,
    /// `PATH_INFO=/list`, and `/application` is left whole because it is not
    /// inside the mount.
    fn new(cfg: &PhpRuntimeConfig, head: &ReqHead) -> Result<Self, String> {
        let target = String::from_utf8_lossy(&head.target);
        let (path, query) = match target.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (&target[..], None),
        };
        let script = script_name(cfg);
        let path_info = strip_mount(script, path);
        let mut vars = ServerVars::new();
        let insert = |vars: &mut ServerVars, name: &str, value: &str| {
            if let Err(err) = vars.insert(name, value) {
                return Err(format!("{name}: {err}"));
            }
            Ok(())
        };
        insert(
            &mut vars,
            "REQUEST_METHOD",
            &String::from_utf8_lossy(&head.method),
        )?;
        insert(&mut vars, "REQUEST_URI", &target)?;
        // `SCRIPT_FILENAME` is the route's entry, never a request-derived
        // path: this is the `php2shell` invariant in its positive form
        // (ADR-0010 §2).
        insert(&mut vars, "SCRIPT_FILENAME", &cfg.entry.to_string_lossy())?;
        insert(
            &mut vars,
            "DOCUMENT_ROOT",
            &cfg.document_root.to_string_lossy(),
        )?;
        insert(&mut vars, "SCRIPT_NAME", script)?;
        insert(&mut vars, "PATH_INFO", path_info)?;
        insert(&mut vars, "QUERY_STRING", query.unwrap_or(""))?;
        insert(&mut vars, "SERVER_PROTOCOL", "HTTP/1.1")?;
        insert(&mut vars, "GATEWAY_INTERFACE", "CGI/1.1")?;
        insert(&mut vars, "SERVER_NAME", &head.server_name)?;
        insert(&mut vars, "SERVER_PORT", &head.server_port.to_string())?;
        insert(&mut vars, "REMOTE_ADDR", &head.remote_addr)?;
        insert(
            &mut vars,
            "HTTPS",
            if matches!(head.scheme, blastit_frame::Scheme::Https) {
                "on"
            } else {
                "off"
            },
        )?;
        for (name, value) in &head.headers {
            // `push_header` drops `Proxy:` (httpoxy) and anything that would
            // derive a reserved name; it can never make the boot fail.
            let _ = vars.push_header(
                &String::from_utf8_lossy(name),
                &String::from_utf8_lossy(value),
            );
        }
        Ok(Self {
            entry: c_string(&cfg.entry.to_string_lossy(), "entry")?,
            document_root: c_string(&cfg.document_root.to_string_lossy(), "document_root")?,
            script_name: c_string(script_name(cfg), "script_name")?,
            request_uri: c_string(&target, "request_uri")?,
            query_string: query
                .map(|query| c_string(query, "query_string"))
                .transpose()?,
            method: c_string(&String::from_utf8_lossy(&head.method), "method")?,
            content_type: header(head, b"content-type")
                .map(|value| c_string(&String::from_utf8_lossy(value), "content-type"))
                .transpose()?,
            cookie: header(head, b"cookie")
                .map(|value| c_string(&String::from_utf8_lossy(value), "cookie"))
                .transpose()?,
            vars,
        })
    }

    /// The `blastit_php_server_var_v1` array, borrowing `vars`'s strings.
    fn entries(&self) -> Vec<ServerVarV1> {
        self.vars
            .iter()
            .map(|var| ServerVarV1 {
                name: var.name().as_ptr().cast(),
                name_len: var.name().len(),
                value: var.value().as_ptr().cast(),
                value_len: var.value().len(),
            })
            .collect()
    }

    /// The request the shim runs. Every pointer borrows `self`, `entries` or
    /// `body` for the call.
    fn view<'a>(&'a self, entries: &'a [ServerVarV1], body: &'a [u8]) -> RequestV1 {
        RequestV1 {
            entry: self.entry.as_ptr(),
            document_root: self.document_root.as_ptr(),
            script_name: self.script_name.as_ptr(),
            request_uri: self.request_uri.as_ptr(),
            query_string: self
                .query_string
                .as_ref()
                .map_or(std::ptr::null(), |q| q.as_ptr()),
            request_method: self.method.as_ptr(),
            content_type: self
                .content_type
                .as_ref()
                .map_or(std::ptr::null(), |c| c.as_ptr()),
            content_length: i64::try_from(body.len()).unwrap_or(i64::MAX),
            cookie: self
                .cookie
                .as_ref()
                .map_or(std::ptr::null(), |c| c.as_ptr()),
            server_vars: entries.as_ptr(),
            server_var_count: entries.len(),
            body: if body.is_empty() {
                std::ptr::null()
            } else {
                body.as_ptr()
            },
            body_len: body.len(),
        }
    }
}

/// The first [`OUTPUT_LINE_CAP`] bytes of a log line, cut on a character
/// boundary so a multi-byte character is never split.
fn capped_line(line: &str) -> &str {
    let mut end = 0;
    for ch in line.chars() {
        let next = end + ch.len_utf8();
        if next > OUTPUT_LINE_CAP {
            break;
        }
        end = next;
    }
    &line[..end]
}

/// The mount point PHP sees as `SCRIPT_NAME`; `/` when mounted at the root.
fn script_name(cfg: &PhpRuntimeConfig) -> &str {
    if cfg.script_name.is_empty() {
        "/"
    } else {
        cfg.script_name.as_str()
    }
}

/// `path` with the mount point `script` stripped at a segment boundary.
///
/// `/app` matches `/app` (→ `/`) and `/app/list` (→ `/list`) but not
/// `/application`, which is not inside the mount and is returned unchanged. A
/// route mounted at `/` strips nothing. The same rule the WSGI view and the
/// scheduler's router apply, so `PATH_INFO` cannot mean two things.
fn strip_mount<'a>(script: &str, path: &'a str) -> &'a str {
    if script.is_empty() || script == "/" {
        return path;
    }
    let Some(rest) = path.strip_prefix(script) else {
        return path;
    };
    if rest.is_empty() {
        return "/";
    }
    if rest.starts_with('/') {
        return rest;
    }
    path
}

/// A header value, case-insensitively.
fn header<'a>(head: &'a ReqHead, name: &[u8]) -> Option<&'a [u8]> {
    head.headers
        .iter()
        .find(|(held, _)| held.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_slice())
}

/// A C string, naming the field in the refusal.
fn c_string(text: &str, what: &str) -> Result<CString, String> {
    CString::new(text).map_err(|_| format!("{what} holds a NUL byte"))
}

/// Whether `path` is an existing absolute file (used by the spawn path's
/// boot-time check).
#[must_use]
pub fn is_serveable_entry(path: &Path) -> bool {
    path.is_absolute() && path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use blastit_frame::INITIAL_WINDOW;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    /// A runtime that answers every request with the body it read.
    struct Echo {
        requests: Mutex<u32>,
    }

    impl WorkerRuntime for Echo {
        fn kind(&self) -> &'static str {
            "php"
        }
        fn max_inflight(&self) -> u32 {
            1
        }
        fn serve(&self, req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd {
            *lock(&self.requests) += 1;
            let mut buf = [0u8; 16];
            let mut got = Vec::new();
            while let Ok(n) = io.read_body(&mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            let head = RespHead {
                status: 200,
                reason: Vec::new(),
                headers: vec![(b"X-Method".to_vec(), req.method.clone())],
            };
            if io.write_head(head).is_err() || io.write_body(&got).is_err() {
                return RequestEnd::new(DoneOutcome::Errored, None);
            }
            io.output(OutputStream::Stderr, "echoed");
            RequestEnd::new(DoneOutcome::Completed, Some(7))
        }
        fn recycle(&self) -> bool {
            false
        }
    }

    /// The scheduler's end of a `view.chan`, decoding what the worker writes.
    struct Peer {
        stream: UnixStream,
        decoder: Decoder,
        limits: Limits,
    }

    impl Peer {
        fn send(&mut self, id: u32, frame: Frame) {
            let mut bytes = Vec::new();
            self.limits.encode(&frame, id, &mut bytes).expect("encode");
            self.stream.write_all(&bytes).expect("write");
        }

        fn drain(&mut self) -> Vec<(u32, Frame)> {
            let mut buf = [0u8; 4096];
            loop {
                match self.stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => self.decoder.push(&buf[..n]),
                    Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                    Err(err) => panic!("read: {err}"),
                }
            }
            let mut frames = Vec::new();
            while let Some(frame) = self.decoder.next_frame().expect("decode") {
                frames.push(frame);
            }
            frames
        }
    }

    fn pair(limits: Limits) -> (UnixStream, Peer) {
        let (worker, scheduler) = UnixStream::pair().expect("socketpair");
        scheduler.set_nonblocking(true).expect("non-blocking");
        (
            worker,
            Peer {
                stream: scheduler,
                decoder: Decoder::new(limits),
                limits,
            },
        )
    }

    /// Runs `serve_forever` on its own thread and returns the join handle.
    fn serve_in_background(
        worker: UnixStream,
        runtime: Arc<dyn WorkerRuntime>,
        limits: Limits,
    ) -> std::thread::JoinHandle<Result<(), ViewError>> {
        std::thread::spawn(move || serve_forever(worker, runtime, limits))
    }

    fn wait_for(peer: &mut Peer, wanted: usize) -> Vec<(u32, Frame)> {
        for _ in 0..200 {
            let frames = peer.drain();
            if frames.len() >= wanted {
                return frames;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the worker never wrote {wanted} frames");
    }

    #[test]
    fn a_socketpair_request_is_answered_and_closed_in_order() {
        // The whole transport: READY, REQ_HEAD/REQ_BODY/REQ_END in, RESP_HEAD,
        // RESP_BODY, RESP_END_COST and DONE out — with the CPU number the
        // runtime returned riding on the frame that ends the response.
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, runtime, limits);

        let ready = wait_for(&mut peer, 1);
        assert!(matches!(ready.as_slice(), [(0, Frame::Ready)]), "{ready:?}");

        peer.send(7, Frame::ReqHead(ReqHead::default()));
        peer.send(7, Frame::ReqBody(b"hello".to_vec()));
        peer.send(7, Frame::ReqEnd);
        let frames = wait_for(&mut peer, 5);

        let kinds: Vec<&str> = frames
            .iter()
            .map(|(_, frame)| match frame {
                Frame::Window { .. } => "window",
                Frame::RespHead(_) => "head",
                Frame::RespBody(_) => "body",
                Frame::RespEndCost { .. } => "end",
                Frame::Done { .. } => "done",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            ["window", "head", "body", "end", "done"],
            "the request-body credit goes back before the response: {frames:?}"
        );
        assert!(matches!(frames[0].1, Frame::Window { bytes: 5 }));
        assert!(matches!(
            frames[2].1,
            Frame::RespBody(ref body) if body == b"hello"
        ));
        assert!(matches!(
            frames[3].1,
            Frame::RespEndCost { cost } if cost.cpu_ns == Some(7) && cost.wall_ns > 0
        ));
        assert!(matches!(
            frames[4].1,
            Frame::Done {
                outcome: DoneOutcome::Completed
            }
        ));
        assert_eq!(
            frames[4].0, 7,
            "every response frame carries the request id"
        );

        // The channel ends: the worker returns cleanly and does not hang.
        drop(peer.stream);
        handle.join().expect("worker thread").expect("clean end");
    }

    #[test]
    fn a_large_response_waits_for_window_frames_instead_of_deadlocking() {
        // The transport is single-threaded, so a body past the initial credit
        // must read its own WINDOW: one byte more than the window is the
        // smallest honest version of that path.
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let body = vec![0x5au8; INITIAL_WINDOW as usize + 4096];
        struct Big(Vec<u8>);
        impl WorkerRuntime for Big {
            fn kind(&self) -> &'static str {
                "php"
            }
            fn max_inflight(&self) -> u32 {
                1
            }
            fn serve(&self, _req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd {
                let _ = io.write_head(RespHead::default());
                if io.write_body(&self.0).is_err() {
                    return RequestEnd::new(DoneOutcome::Aborted, None);
                }
                RequestEnd::new(DoneOutcome::Completed, None)
            }
            fn recycle(&self) -> bool {
                false
            }
        }
        let runtime = Arc::new(Big(body.clone()));
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);

        peer.send(1, Frame::ReqHead(ReqHead::default()));
        peer.send(1, Frame::ReqEnd);
        // Wait for the head, then spend the window down to nothing and grant.
        let mut rebuilt = Vec::new();
        for _ in 0..200 {
            for (_, frame) in peer.drain() {
                match frame {
                    Frame::RespBody(chunk) => rebuilt.extend_from_slice(&chunk),
                    Frame::Done { .. } => {}
                    _ => {}
                }
            }
            if rebuilt.len() == body.len() {
                break;
            }
            peer.send(1, Frame::Window { bytes: 65536 });
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(rebuilt.len(), body.len(), "the body never completed");
        drop(peer.stream);
        handle.join().expect("worker thread").expect("clean end");
    }

    #[test]
    fn a_cancelled_request_aborts_but_the_worker_serves_the_next_one() {
        // A client disconnect (CANCEL) is an ordinary event: the response is
        // reported aborted, `RESP_END_COST` + `DONE` still go out so the
        // scheduler frees the slot, a stale `CANCEL` arriving after `DONE`
        // (no request in flight) is dropped as bookkeeping, and the *same*
        // worker serves the next request. Retiring on either path would cost a
        // fork and a `php_module_startup` per disconnect.
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(
            worker,
            Arc::clone(&runtime) as Arc<dyn WorkerRuntime>,
            limits,
        );
        let _ = wait_for(&mut peer, 1);
        peer.send(9, Frame::ReqHead(ReqHead::default()));
        peer.send(9, Frame::Cancel);

        let mut frames = Vec::new();
        for _ in 0..200 {
            frames.extend(peer.drain());
            let dones = frames
                .iter()
                .filter(|(_, frame)| matches!(frame, Frame::Done { .. }))
                .count();
            if dones >= 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            !frames
                .iter()
                .any(|(id, frame)| *id == 9 && matches!(frame, Frame::RespHead(_))),
            "an abandoned response must not reach the channel: {frames:?}"
        );
        assert!(
            matches!(
                frames.as_slice(),
                [
                    (9, Frame::RespEndCost { .. }),
                    (
                        9,
                        Frame::Done {
                            outcome: DoneOutcome::Aborted
                        }
                    ),
                ]
            ),
            "RESP_END_COST must precede DONE(Aborted) for the cancelled request: {frames:?}"
        );

        // Now no request is in flight: a stale `CANCEL` for the already-DONE
        // request (the scheduler's `complete` view lagged) is bookkeeping, not
        // a protocol error.
        peer.send(9, Frame::Cancel);

        // The same worker (the thread never exited) answers a second request
        // with a full `RESP_HEAD`..`DONE(Completed)` sequence — even if a
        // stale `CANCEL(9)` arrives *after* `REQ_HEAD(10)` while request 10 is
        // reading its body.
        peer.send(10, Frame::ReqHead(ReqHead::default()));
        peer.send(9, Frame::Cancel);
        peer.send(10, Frame::ReqBody(b"still alive".to_vec()));
        peer.send(10, Frame::ReqEnd);
        let mut second = Vec::new();
        for _ in 0..200 {
            second.extend(peer.drain());
            if second
                .iter()
                .any(|(id, frame)| *id == 10 && matches!(frame, Frame::Done { .. }))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            matches!(
                second.as_slice(),
                [
                    (10, Frame::Window { bytes: 11 }),
                    (10, Frame::RespHead(head)),
                    (10, Frame::RespBody(body)),
                    (10, Frame::RespEndCost { cost }),
                    (
                        10,
                        Frame::Done {
                            outcome: DoneOutcome::Completed
                        }
                    ),
                ] if head.status == 200 && body == b"still alive" && cost.cpu_ns == Some(7)
            ),
            "the next request was not served in full after a stale CANCEL: {second:?}"
        );
        assert_eq!(*lock(&runtime.requests), 2);

        drop(peer.stream);
        handle.join().expect("worker thread").expect("clean end");
    }

    #[test]
    fn a_request_without_a_stream_is_a_protocol_error() {
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(0, Frame::ReqHead(ReqHead::default()));
        drop(peer.stream);
        assert!(handle.join().expect("worker thread").is_err());
    }

    #[test]
    fn a_stray_body_frame_names_only_its_kind_and_never_leaks_bytes() {
        // Security hygiene: protocol-error strings reach the view's stderr, so
        // a stray frame carrying payload bytes must name `frame_kind` only and
        // never interpolate `Debug` bytes (passwords/PII). Non-`Cancel` foreign
        // frames also remain strict protocol errors.
        let secret = b"s3cr3t-client-password".to_vec();
        let debug_bytes = format!("{secret:?}");
        let limits = Limits::default();

        // 1. Stray `REQ_BODY` while idle (before `REQ_HEAD`).
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(1, Frame::ReqBody(secret.clone()));
        drop(peer.stream);
        let idle_err = handle
            .join()
            .expect("worker thread")
            .expect_err("stray REQ_BODY before REQ_HEAD must fail")
            .to_string();
        assert!(idle_err.contains("REQ_BODY"), "{idle_err}");
        assert!(!idle_err.contains("s3cr3t"), "leaked text: {idle_err}");
        assert!(!idle_err.contains(&debug_bytes), "leaked bytes: {idle_err}");
        assert!(!idle_err.contains("115"), "leaked byte value: {idle_err}");

        // 2. Stray `RESP_BODY` while reading a request body.
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(2, Frame::ReqHead(ReqHead::default()));
        peer.send(2, Frame::RespBody(secret.clone()));
        drop(peer.stream);
        let body_err = handle
            .join()
            .expect("worker thread")
            .expect_err("stray RESP_BODY during request must fail")
            .to_string();
        assert!(body_err.contains("RESP_BODY"), "{body_err}");
        assert!(!body_err.contains("s3cr3t"), "leaked text: {body_err}");
        assert!(!body_err.contains(&debug_bytes), "leaked bytes: {body_err}");
        assert!(!body_err.contains("115"), "leaked byte value: {body_err}");

        // 3. Denied twin for stale-`CANCEL` exemption: a foreign `REQ_BODY`
        // (`id != self.id`) is still a strict protocol error (and leaks no bytes).
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(2, Frame::ReqHead(ReqHead::default()));
        peer.send(99, Frame::ReqBody(secret));
        drop(peer.stream);
        let foreign_err = handle
            .join()
            .expect("worker thread")
            .expect_err("foreign REQ_BODY must remain a protocol error")
            .to_string();
        assert!(
            foreign_err.contains("request 99") && foreign_err.contains("request 2"),
            "{foreign_err}"
        );
        assert!(!foreign_err.contains("s3cr3t"), "{foreign_err}");
        assert!(!foreign_err.contains(&debug_bytes), "{foreign_err}");
    }

    #[test]
    fn a_configuration_that_could_not_serve_is_refused_both_ways() {
        let dir = std::env::temp_dir().join(format!("blastit-php-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let entry = dir.join("index.php");
        std::fs::write(&entry, b"<?php echo 'x';").expect("entry");
        let good = PhpRuntimeConfig {
            entry: entry.clone(),
            document_root: dir.clone(),
            script_name: String::new(),
            ini: "memory_limit=256M\n".to_string(),
            view_tmp: dir.clone(),
            max_requests: 100,
            max_body_bytes: 1024 * 1024,
        };
        assert!(good.validate().is_ok(), "the accepted twin must pass");

        let missing = PhpRuntimeConfig {
            entry: dir.join("nope.php"),
            ..good.clone()
        };
        assert!(missing.validate().is_err(), "a missing entry is refused");
        let relative = PhpRuntimeConfig {
            entry: PathBuf::from("public/index.php"),
            ..good.clone()
        };
        assert!(relative.validate().is_err(), "a relative entry is refused");
        let no_ini = PhpRuntimeConfig {
            ini: String::new(),
            ..good.clone()
        };
        assert!(no_ini.validate().is_err(), "an empty INI is refused");
        let zero = PhpRuntimeConfig {
            max_requests: 0,
            ..good.clone()
        };
        assert!(zero.validate().is_err(), "max_requests = 0 is refused");
        let no_body_budget = PhpRuntimeConfig {
            max_body_bytes: 0,
            ..good.clone()
        };
        assert!(
            no_body_budget.validate().is_err(),
            "a zero body budget is refused"
        );
        let no_tmp = PhpRuntimeConfig {
            view_tmp: dir.join("missing-tmp"),
            ..good
        };
        assert!(
            no_tmp.validate().is_err(),
            "a missing view_tmp directory is refused"
        );
        let relative_tmp = PhpRuntimeConfig {
            view_tmp: PathBuf::from("tmp"),
            ..no_tmp
        };
        assert!(
            relative_tmp.validate().is_err(),
            "a relative view_tmp is refused"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_non_library_is_a_boot_refusal_and_never_a_request_fault() {
        let dir = std::env::temp_dir().join(format!("blastit-php-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let entry = dir.join("index.php");
        std::fs::write(&entry, b"<?php echo 'x';").expect("entry");
        let fake = dir.join("libphp.so");
        std::fs::write(&fake, b"not a library").expect("fake");
        let err = PhpEmbedRuntime::load(
            SharedObject::Path(fake),
            SharedObject::Path(dir.join("shim.so")),
            PhpRuntimeConfig {
                entry,
                document_root: dir.clone(),
                script_name: String::new(),
                ini: "memory_limit=256M\n".to_string(),
                view_tmp: dir.clone(),
                max_requests: 1,
                max_body_bytes: 1024,
            },
        )
        .err()
        .expect("a text file cannot load");
        assert!(
            matches!(err, PhpError::Load(LoadError::NotALibrary { .. })),
            "{err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_output_line_is_capped_on_a_character_boundary() {
        // A `echo str_repeat('é', 5000)` outside the response must not split a
        // character (a panic) or bury the log (the contract's 2048 bytes).
        let ascii = "x".repeat(OUTPUT_LINE_CAP + 10);
        assert_eq!(capped_line(&ascii).len(), OUTPUT_LINE_CAP);
        let wide = "é".repeat(OUTPUT_LINE_CAP);
        let capped = capped_line(&wide);
        assert!(capped.len() <= OUTPUT_LINE_CAP);
        assert_eq!(capped.len() % 2, 0, "a two-byte character was split");
        assert_eq!(capped_line("short"), "short");
    }

    #[test]
    fn the_php_environment_never_carries_http_proxy() {
        // httpoxy, at the seam that builds `$_SERVER`: a `Proxy:` header is
        // dropped before it could ever become `HTTP_PROXY`.
        let head = ReqHead {
            method: b"GET".to_vec(),
            target: b"/app/x?y=1".to_vec(),
            headers: vec![
                (b"Proxy".to_vec(), b"http://attacker.example".to_vec()),
                (b"Host".to_vec(), b"app.test".to_vec()),
            ],
            ..ReqHead::default()
        };
        let cfg = PhpRuntimeConfig {
            entry: PathBuf::from("/srv/app/index.php"),
            document_root: PathBuf::from("/srv/app"),
            script_name: "/app".to_string(),
            ini: "memory_limit=256M\n".to_string(),
            view_tmp: PathBuf::from("/tmp"),
            max_requests: 1,
            max_body_bytes: 1024,
        };
        let prepared = Prepared::new(&cfg, &head).expect("prepared");
        let names: Vec<&str> = prepared.vars.iter().map(|var| var.name()).collect();
        assert!(names.contains(&"REQUEST_METHOD"), "{names:?}");
        assert!(names.contains(&"QUERY_STRING"), "{names:?}");
        assert!(names.contains(&"HTTP_HOST"), "{names:?}");
        assert!(!names.contains(&"HTTP_PROXY"), "httpoxy: {names:?}");
        assert_eq!(
            prepared.query_string.as_ref().map(|q| q.to_str().unwrap()),
            Some("y=1")
        );
        // The CGI triple, from the same request: the URI stays whole, the
        // mount point is `SCRIPT_NAME`, and `PATH_INFO` is the remainder.
        let value = |name: &str| {
            prepared
                .vars
                .iter()
                .find(|var| var.name() == name)
                .map(|var| var.value().to_owned())
                .unwrap_or_else(|| panic!("{name} is missing"))
        };
        assert_eq!(value("REQUEST_URI"), "/app/x?y=1");
        assert_eq!(value("SCRIPT_NAME"), "/app");
        assert_eq!(value("PATH_INFO"), "/x");
        assert_eq!(value("QUERY_STRING"), "y=1");
    }

    #[test]
    fn the_mount_point_is_stripped_only_at_a_segment_boundary() {
        // The WSGI view and the router apply this rule; PHP's `PATH_INFO` must
        // not disagree with them.
        assert_eq!(strip_mount("/app", "/app"), "/");
        assert_eq!(strip_mount("/app", "/app/list"), "/list");
        assert_eq!(strip_mount("/app", "/application"), "/application");
        assert_eq!(strip_mount("/", "/anything"), "/anything");
        assert_eq!(strip_mount("", "/anything"), "/anything");
    }

    /// A `RequestIo` with a scripted body, recording what the response wrote.
    struct FakeIo {
        chunks: std::collections::VecDeque<Vec<u8>>,
        head: ReqHead,
        head_written: Option<RespHead>,
        body: Vec<u8>,
        lines: Vec<String>,
    }

    impl FakeIo {
        fn with_body(chunks: Vec<Vec<u8>>) -> Self {
            Self {
                chunks: chunks.into(),
                head: ReqHead::default(),
                head_written: None,
                body: Vec::new(),
                lines: Vec::new(),
            }
        }
    }

    impl RequestIo for FakeIo {
        fn head(&self) -> &ReqHead {
            &self.head
        }
        fn read_body(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let Some(chunk) = self.chunks.front_mut() else {
                return Ok(0);
            };
            let n = buf.len().min(chunk.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            chunk.drain(..n);
            if chunk.is_empty() {
                self.chunks.pop_front();
            }
            Ok(n)
        }
        fn write_head(&mut self, head: RespHead) -> io::Result<()> {
            self.head_written = Some(head);
            Ok(())
        }
        fn write_body(&mut self, data: &[u8]) -> io::Result<()> {
            self.body.extend_from_slice(data);
            Ok(())
        }
        fn output(&mut self, _stream: OutputStream, line: &str) {
            self.lines.push(line.to_owned());
        }
        fn flush_response(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_request_body_past_the_worker_budget_is_refused() {
        // The accepted twin first: the same body fits a larger budget.
        let mut io = FakeIo::with_body(vec![vec![0x41; 64], vec![0x42; 64]]);
        let body = read_body(&mut io, 1024).expect("128 bytes fit");
        assert_eq!(body.len(), 128);

        let mut io = FakeIo::with_body(vec![vec![0x41; 64], vec![0x42; 64]]);
        let err = read_body(&mut io, 127).expect_err("128 > 127");
        assert!(matches!(err, BodyError::TooLarge));
    }

    #[test]
    fn a_bailed_worker_retires_and_max_requests_still_applies() {
        // The rule `recycle()` implements, without a loaded libphp.
        assert!(should_recycle(true, 0, 100), "a bailout retires at once");
        assert!(should_recycle(false, 3, 3), "max_requests reached");
        assert!(!should_recycle(false, 2, 3), "one more is allowed");
    }

    #[test]
    fn a_runtime_that_retires_ends_the_loop_after_its_done() {
        // `max_requests`/bailout exit: `serve_forever` returns `Ok` right after
        // the request's `DONE`, without waiting for the channel to close.
        struct Once(Mutex<u32>);
        impl WorkerRuntime for Once {
            fn kind(&self) -> &'static str {
                "php"
            }
            fn max_inflight(&self) -> u32 {
                1
            }
            fn serve(&self, _req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd {
                *lock(&self.0) += 1;
                let _ = io.write_head(RespHead::default());
                RequestEnd::new(DoneOutcome::Completed, None)
            }
            fn recycle(&self) -> bool {
                *lock(&self.0) >= 1
            }
        }
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Once(Mutex::new(0)));
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(3, Frame::ReqHead(ReqHead::default()));
        peer.send(3, Frame::ReqEnd);
        // The response is head, cost and done; there is no body.
        let frames = wait_for(&mut peer, 3);
        assert!(
            frames
                .iter()
                .any(|(_, frame)| matches!(frame, Frame::Done { .. })),
            "{frames:?}"
        );
        // The scheduler's end is still open, but the worker has retired.
        assert!(handle.join().expect("worker thread").is_ok());
    }

    #[test]
    fn a_response_body_before_the_head_is_a_protocol_error() {
        struct BodyFirst;
        impl WorkerRuntime for BodyFirst {
            fn kind(&self) -> &'static str {
                "php"
            }
            fn max_inflight(&self) -> u32 {
                1
            }
            fn serve(&self, _req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd {
                let _ = io.write_body(b"without a head");
                RequestEnd::new(DoneOutcome::Errored, None)
            }
            fn recycle(&self) -> bool {
                false
            }
        }
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(BodyFirst);
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(4, Frame::ReqHead(ReqHead::default()));
        peer.send(4, Frame::ReqEnd);
        drop(peer.stream);
        // The transport records the refusal and ends the view.
        assert!(handle.join().expect("worker thread").is_err());
    }

    #[test]
    fn a_request_body_past_the_flow_window_is_a_protocol_error() {
        // The scheduler may not send more body than the window allows, and the
        // view must refuse the frame instead of buffering it.
        let limits = Limits::default();
        let (mut ours, mut peer) = UnixStream::pair().expect("socketpair");
        peer.set_nonblocking(true).expect("non-blocking");
        let writer = Writer::new(ours.try_clone().expect("clone"), limits);
        let flow = Flow::new();
        assert!(
            flow.body_received(INITIAL_WINDOW as usize),
            "spend the whole window"
        );
        let mut decoder = Decoder::new(limits);
        let mut buf = [0u8; 1024];
        let mut io = ChanIo {
            head: ReqHead::default(),
            id: 5,
            writer: &writer,
            flow,
            chan: &mut ours,
            decoder: &mut decoder,
            buf: &mut buf,
            pending: Vec::new(),
            ended: false,
            head_sent: false,
            cancelled: false,
            error: None,
        };
        let mut frames = Vec::new();
        limits
            .encode(&Frame::ReqBody(b"x".to_vec()), 5, &mut frames)
            .expect("encode");
        peer.write_all(&frames).expect("write");
        assert!(io.read_body(&mut [0u8; 8]).is_err());
        assert!(io.error.is_some(), "the refusal is remembered");
    }

    #[test]
    fn a_header_burst_past_the_cap_aborts_the_response() {
        let mut io = FakeIo::with_body(Vec::new());
        let mut sink = PhpSink {
            io: &mut io,
            headers: Vec::new(),
            header_bytes: 0,
            head_sent: false,
            abort: false,
        };
        // The accepted twin: a normal header block.
        for _ in 0..MAX_HEADERS {
            assert!(sink.head(200, Some(b"X-Ok"), b"1"));
        }
        // The (MAX_HEADERS + 1)th is refused, and so is a single oversized one.
        assert!(!sink.head(200, Some(b"X-Too-Many"), b"1"));
        assert!(sink.abort);
        let mut sink = PhpSink {
            io: &mut io,
            headers: Vec::new(),
            header_bytes: 0,
            head_sent: false,
            abort: false,
        };
        let big = vec![b'v'; MAX_HEADER_BYTES + 1];
        assert!(!sink.head(200, Some(b"X-Big"), &big));
        assert!(sink.abort);
        assert!(sink.headers.is_empty());
    }
}
