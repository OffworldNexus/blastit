//! `wsgi.input`: the request body as a blocking Python stream.
//!
//! The stream is also where request-body flow control is spent: the scheduler
//! may send [`blastit_frame::INITIAL_WINDOW`] bytes of a body without asking,
//! and gets one `WINDOW` frame back for every byte the application consumes
//! (§3.1). Nothing is buffered on behalf of an application that does not read:
//! the sender simply runs out of credit and stalls, over TCP, in its own
//! stream.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvError};
use std::sync::{Arc, Mutex};

use blastit_frame::Frame;
use pyo3::exceptions::PyOSError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyList};

use crate::request::RequestState;
use blastit_view::flow::WINDOW_BATCH;
use blastit_view::sync::lock;
use blastit_view::wire::Writer;

/// The largest size a single read will be asked for.
///
/// Python's `size` argument is an unbounded integer; a caller that passes one
/// means "as much as you have". Nothing here allocates from a size it has not
/// seen bytes for, and the clamp keeps the arithmetic in `usize` honest on
/// every platform.
const MAX_READ_SIZE: usize = 1024 * 1024 * 1024;

/// One message from the I/O thread to a request's `wsgi.input`.
#[derive(Debug)]
pub(crate) enum BodyMsg {
    /// A chunk of the request body, in order.
    Data(Vec<u8>),
    /// The body is complete (`REQ_END`), or the channel went away.
    End,
    /// The scheduler cancelled the request (`CANCEL`).
    Cancelled,
}

/// Why a read failed.
///
/// Both cases are the *request's* failure, not the application's: a cancelled
/// request is not going to have its response delivered either way, and a broken
/// channel will not carry the body or the response. They surface as `OSError`,
/// which is what a WSGI application already has to handle for a broken
/// connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputError {
    /// The scheduler cancelled the request.
    Cancelled,
    /// The channel to the scheduler is gone.
    Channel,
}

impl InputError {
    /// The Python exception a read raises.
    fn to_py(self) -> PyErr {
        match self {
            Self::Cancelled => PyOSError::new_err("request cancelled, wsgi.input is closed"),
            Self::Channel => PyOSError::new_err("view channel is gone, wsgi.input is closed"),
        }
    }
}

/// What one `read`-family call wants.
#[derive(Debug, Clone, Copy)]
enum Spec {
    /// `read()`: every byte up to the end of the body.
    All,
    /// `read(n)`: exactly `n` bytes, or fewer at the end of the body.
    AtMost(usize),
    /// `readline(n)`: up to and including one `\n`, at most `n` bytes.
    Line(Option<usize>),
}

/// Bytes already delivered for one request, and whether more are coming.
#[derive(Debug, Default)]
struct BodyBuf {
    bytes: VecDeque<u8>,
    ended: bool,
}

/// Collects consumed body bytes and hands them back as whole `WINDOW` batches.
///
/// A frame per byte would be a frame per byte of upload; waiting for the end of
/// a read would stall a large `read()`. So credit is sent as it is consumed, in
/// whole [`WINDOW_BATCH`] pieces, with the remainder flushed at the end of each
/// read — a body smaller than one batch is still paid for.
#[derive(Debug, Default)]
struct CreditBatch {
    unsettled: usize,
}

impl CreditBatch {
    /// Note `bytes` more consumed body, returning a whole batch to send, if one
    /// is due.
    fn add(&mut self, bytes: usize) -> u32 {
        self.unsettled = self.unsettled.saturating_add(bytes);
        let whole = self.unsettled / WINDOW_BATCH;
        let batch = (whole * WINDOW_BATCH).min(u32::MAX as usize);
        self.unsettled -= batch;
        batch as u32
    }

    /// Take whatever is left, at a read boundary.
    fn flush(&mut self) -> u32 {
        let batch = self.unsettled.min(u32::MAX as usize);
        self.unsettled -= batch;
        batch as u32
    }
}

/// The credit this request hands back to the scheduler as its body is consumed.
struct CreditBack {
    writer: Arc<Writer>,
    id: u32,
    state: Arc<RequestState>,
}

impl CreditBack {
    /// Credit `bytes` more request body, and tell the scheduler.
    ///
    /// The local window is raised *first*: frames the scheduler sent on the
    /// strength of an earlier grant must never be refused because a `WINDOW`
    /// frame was lost on the way, so the process stays generous and only the
    /// sender ever stalls.
    fn grant(&self, bytes: u32) -> Result<(), InputError> {
        if bytes == 0 {
            return Ok(());
        }
        self.state.flow().body_granted(bytes as usize);
        self.writer
            .write_frame(&Frame::Window { bytes }, self.id)
            .map_err(|_| InputError::Channel)
    }
}

/// The `wsgi.input` object of one request.
///
/// It is a *stream*, not a buffer: `REQ_BODY` frames are handed to it as they
/// arrive, and a read that needs more than is buffered blocks — with the GIL
/// released — on the channel from the I/O thread until more arrives, the body
/// ends, or the request is cancelled. After `REQ_END` every read returns `b""`,
/// so iteration and `read()` terminate.
///
/// Every byte a read hands to the application is credited back to the
/// scheduler, which is what lets an upload larger than the initial window
/// through at all.
#[pyclass]
pub(crate) struct Input {
    state: Arc<RequestState>,
    rx: Arc<Mutex<Receiver<BodyMsg>>>,
    buf: Arc<Mutex<BodyBuf>>,
    credit: Arc<CreditBack>,
}

impl Input {
    /// The input object for a request whose body arrives on `rx`.
    pub(crate) fn new(
        state: Arc<RequestState>,
        rx: Receiver<BodyMsg>,
        id: u32,
        writer: Arc<Writer>,
    ) -> Self {
        Self {
            credit: Arc::new(CreditBack {
                writer,
                id,
                state: Arc::clone(&state),
            }),
            state,
            rx: Arc::new(Mutex::new(rx)),
            buf: Arc::new(Mutex::new(BodyBuf::default())),
        }
    }

    /// Read as `spec` asks, blocking (without the GIL) until it can be met.
    fn read_bytes(&self, py: Python<'_>, spec: Spec) -> PyResult<Vec<u8>> {
        let state = Arc::clone(&self.state);
        let rx = Arc::clone(&self.rx);
        let buf = Arc::clone(&self.buf);
        let credit = Arc::clone(&self.credit);
        py.detach(move || pump(&state, &rx, &buf, &credit, spec))
            .map_err(InputError::to_py)
    }
}

#[pymethods]
impl Input {
    /// `read(size=-1)` — the whole body, or the next `size` bytes.
    ///
    /// Like a buffered file, a positive `size` blocks until that many bytes are
    /// available or the body ends, which is what frameworks do when they read
    /// `CONTENT_LENGTH` bytes in one go. `size` may be `None` or negative and
    /// means "everything"; an absurd size is clamped.
    ///
    /// A read with no size credits the scheduler as it goes, so it never
    /// blocks on a body it is not letting through.
    #[pyo3(signature = (size = None))]
    fn read<'py>(&self, py: Python<'py>, size: Option<i64>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.read_bytes(py, spec_for(size))?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// `readline(size=-1)` — up to and including the next newline.
    #[pyo3(signature = (size = None))]
    fn readline<'py>(&self, py: Python<'py>, size: Option<i64>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.read_bytes(py, Spec::Line(limit_for(size)))?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// `readlines(hint=-1)` — every remaining line.
    ///
    /// A positive `hint` stops early as soon as that many bytes of lines have
    /// been collected, as `io.BytesIO.readlines` does.
    #[pyo3(signature = (hint = None))]
    fn readlines<'py>(&self, py: Python<'py>, hint: Option<i64>) -> PyResult<Bound<'py, PyList>> {
        let limit = limit_for(hint);
        let mut lines = Vec::new();
        let mut read = 0;
        loop {
            let line = self.read_bytes(py, Spec::Line(None))?;
            if line.is_empty() {
                break;
            }
            read += line.len();
            lines.push(PyBytes::new(py, &line));
            if limit.is_some_and(|limit| read >= limit) {
                break;
            }
        }
        PyList::new(py, lines)
    }

    /// `iter(wsgi.input)` — line iteration, as for any Python file object.
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// The next line, or `None` at the end of the body.
    fn __next__<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyBytes>>> {
        let line = self.read_bytes(py, Spec::Line(None))?;
        if line.is_empty() {
            return Ok(None);
        }
        Ok(Some(PyBytes::new(py, &line)))
    }
}

/// The read specification for a `read(size)` call.
fn spec_for(size: Option<i64>) -> Spec {
    match size {
        None => Spec::All,
        Some(size) if size < 0 => Spec::All,
        Some(size) => Spec::AtMost(clamp(size)),
    }
}

/// The byte limit for `readline(size)` or `readlines(hint)`.
fn limit_for(size: Option<i64>) -> Option<usize> {
    match size {
        None => None,
        Some(size) if size <= 0 => Some(0),
        Some(size) => Some(clamp(size)),
    }
}

/// A Python size, clamped so it can never overflow a `usize`.
fn clamp(size: i64) -> usize {
    usize::try_from(size)
        .unwrap_or(MAX_READ_SIZE)
        .min(MAX_READ_SIZE)
}

/// Move bytes out of the queue until `spec` can be satisfied, blocking for more
/// as needed, and credit the scheduler for what it hands over. Runs with the
/// GIL released.
fn pump(
    state: &RequestState,
    rx: &Mutex<Receiver<BodyMsg>>,
    buf: &Mutex<BodyBuf>,
    credit: &CreditBack,
    spec: Spec,
) -> Result<Vec<u8>, InputError> {
    let mut out = Vec::new();
    let mut batch = CreditBatch::default();
    loop {
        // The flag comes first: once the request is cancelled the channel may
        // never carry anything again, and a read must not block on a body that
        // will never be completed.
        if state.is_cancelled() {
            return Err(InputError::Cancelled);
        }
        {
            let mut body = lock(buf);
            let moved = take(&mut body, spec, &mut out);
            if moved > 0 {
                credit.grant(batch.add(moved))?;
            }
            if satisfied(&body, spec, &out) {
                break;
            }
        }
        match lock(rx).recv() {
            Ok(BodyMsg::Data(bytes)) => lock(buf).bytes.extend(bytes),
            Ok(BodyMsg::End) => lock(buf).ended = true,
            Ok(BodyMsg::Cancelled) => return Err(InputError::Cancelled),
            // Every sender is gone: the I/O thread has finished with this
            // request, so the body is as complete as it will ever be.
            Err(RecvError) => lock(buf).ended = true,
        }
    }
    // The end of the read is a credit boundary of its own: a body smaller than
    // one batch still has to be paid for.
    credit.grant(batch.flush())?;
    Ok(out)
}

/// Move what `spec` can be given from `body` into `out`, returning how many
/// bytes moved.
fn take(body: &mut BodyBuf, spec: Spec, out: &mut Vec<u8>) -> usize {
    let wanted = match spec {
        Spec::All => body.bytes.len(),
        Spec::AtMost(0) => 0,
        Spec::AtMost(size) => size.saturating_sub(out.len()).min(body.bytes.len()),
        Spec::Line(limit) => {
            let line = match body.bytes.iter().position(|byte| *byte == b'\n') {
                Some(at) => at + 1,
                None => body.bytes.len(),
            };
            let room = match limit {
                Some(limit) => limit.saturating_sub(out.len()),
                None => usize::MAX,
            };
            line.min(room)
        }
    };
    out.extend(body.bytes.drain(..wanted));
    wanted
}

/// Whether `spec` has all it is going to get.
///
/// It looks at what has been read so far rather than at what is buffered: a
/// `readline` that has already collected a line is done even though the line is
/// no longer in the buffer.
fn satisfied(body: &BodyBuf, spec: Spec, got: &[u8]) -> bool {
    match spec {
        Spec::All => body.ended,
        Spec::AtMost(0) => true,
        Spec::AtMost(size) => got.len() >= size || body.ended,
        Spec::Line(limit) => {
            got.ends_with(b"\n") || limit.is_some_and(|limit| got.len() >= limit) || body.ended
        }
    }
}
