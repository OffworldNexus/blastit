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

use crate::flow::WINDOW_BATCH;
use crate::request::RequestState;
use crate::sync::lock;
use crate::wire::Writer;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc::channel;

    /// A body with `bytes` delivered and no end.
    fn buf(bytes: &[u8]) -> BodyBuf {
        BodyBuf {
            bytes: bytes.iter().copied().collect(),
            ended: false,
        }
    }

    fn read_from(body: &mut BodyBuf, spec: Spec) -> Vec<u8> {
        let mut out = Vec::new();
        take(body, spec, &mut out);
        out
    }

    #[test]
    fn read_all_waits_for_the_end_and_takes_everything() {
        let mut body = buf(b"abc");
        assert!(!satisfied(&body, Spec::All, b""));
        body.ended = true;
        assert!(satisfied(&body, Spec::All, b""));
        assert_eq!(read_from(&mut body, Spec::All), b"abc");
        assert_eq!(read_from(&mut body, Spec::All), b"");
    }

    #[test]
    fn read_n_takes_progress_but_waits_for_n_or_the_end() {
        // Two of the three bytes are there: they move, but the read is not
        // finished.
        let mut body = buf(b"ab");
        let mut out = Vec::new();
        assert_eq!(take(&mut body, Spec::AtMost(3), &mut out), 2);
        assert_eq!(out, b"ab");
        assert!(
            !satisfied(&body, Spec::AtMost(3), &out),
            "two bytes of the three it asked for"
        );
        body.ended = true;
        assert!(satisfied(&body, Spec::AtMost(3), &out));

        // Once it has what it asked for, it stops there.
        let mut body = buf(b"abcde");
        let mut out = Vec::new();
        assert_eq!(take(&mut body, Spec::AtMost(2), &mut out), 2);
        assert_eq!(out, b"ab");
        assert!(satisfied(&body, Spec::AtMost(2), &out));
        assert_eq!(take(&mut body, Spec::AtMost(2), &mut out), 0, "not more");
    }

    #[test]
    fn read_zero_never_blocks() {
        let mut body = BodyBuf::default();
        assert!(satisfied(&body, Spec::AtMost(0), b""));
        assert_eq!(read_from(&mut body, Spec::AtMost(0)), b"");
    }

    #[test]
    fn readline_stops_after_the_newline_or_at_the_end() {
        let mut body = buf(b"one\ntwo");
        let mut out = Vec::new();
        assert_eq!(take(&mut body, Spec::Line(None), &mut out), 4);
        assert_eq!(out, b"one\n");
        assert!(satisfied(&body, Spec::Line(None), &out), "a whole line");
        assert!(
            !satisfied(&body, Spec::Line(None), b""),
            "no line yet, and the body has not ended"
        );
        body.ended = true;
        assert!(satisfied(&body, Spec::Line(None), b""));
        out.clear();
        assert_eq!(take(&mut body, Spec::Line(None), &mut out), 3);
        assert_eq!(out, b"two");
    }

    #[test]
    fn readline_honours_its_size() {
        let mut body = buf(b"one\ntwo\n");
        assert_eq!(read_from(&mut body, Spec::Line(Some(2))), b"on");
        // Two bytes is the size it was given, so that read is over even though
        // no newline was reached.
        assert!(satisfied(&body, Spec::Line(Some(2)), b"on"));
        // The newline wins over the size: `readline(4)` stops at the line end.
        assert_eq!(read_from(&mut body, Spec::Line(Some(4))), b"e\n");
        assert!(satisfied(&body, Spec::Line(Some(4)), b"e\n"));
        assert_eq!(read_from(&mut body, Spec::Line(Some(0))), b"");
        assert!(satisfied(&body, Spec::Line(Some(0)), b""));
        assert_eq!(read_from(&mut body, Spec::Line(None)), b"two\n");
    }

    #[test]
    fn an_empty_line_is_a_line() {
        let mut body = buf(b"\n\n");
        assert_eq!(read_from(&mut body, Spec::Line(None)), b"\n");
        assert_eq!(read_from(&mut body, Spec::Line(None)), b"\n");
        body.ended = true;
        assert_eq!(read_from(&mut body, Spec::Line(None)), b"");
    }

    #[test]
    fn sizes_map_from_the_python_signature() {
        assert!(matches!(spec_for(None), Spec::All));
        assert!(matches!(spec_for(Some(-1)), Spec::All));
        assert!(matches!(spec_for(Some(0)), Spec::AtMost(0)));
        assert!(matches!(spec_for(Some(4096)), Spec::AtMost(4096)));
        assert!(matches!(spec_for(Some(i64::MAX)), Spec::AtMost(n) if n == MAX_READ_SIZE));

        assert!(limit_for(None).is_none());
        assert_eq!(limit_for(Some(-1)), Some(0));
        assert_eq!(limit_for(Some(0)), Some(0));
        assert_eq!(limit_for(Some(7)), Some(7));
        assert_eq!(limit_for(Some(i64::MAX)), Some(MAX_READ_SIZE));
    }

    #[test]
    fn credit_is_sent_in_whole_batches_and_the_rest_at_a_boundary() {
        let mut batch = CreditBatch::default();
        assert_eq!(batch.add(1), 0, "a byte is not a batch");
        assert_eq!(batch.add(10), 0);
        assert_eq!(batch.flush(), 11, "the read boundary pays for the rest");

        assert_eq!(batch.add(WINDOW_BATCH), WINDOW_BATCH as u32);
        assert_eq!(batch.flush(), 0);

        // A read that moves several batches' worth sends several whole ones and
        // leaves the remainder settled up at the boundary.
        assert_eq!(batch.add(WINDOW_BATCH * 2 + 5), (WINDOW_BATCH * 2) as u32);
        assert_eq!(batch.flush(), 5);
    }

    #[test]
    fn a_dropped_sender_ends_the_body() {
        // `pump` treats a closed channel as the end of the body, so a request
        // whose I/O thread is gone cannot hang on a read.
        let state = RequestState::new();
        let (tx, rx) = channel::<BodyMsg>();
        let rx = Mutex::new(rx);
        let buf = Mutex::new(BodyBuf::default());
        let (credit, _peer) = credit_back(&state);
        drop(tx);
        let bytes = pump(&state, &rx, &buf, &credit, Spec::All).expect("end of body");
        assert!(bytes.is_empty());
        assert!(lock(&buf).ended);
    }

    #[test]
    fn pump_returns_what_was_queued_and_credits_it_back() {
        let state = RequestState::new();
        assert!(state.flow().body_received(6));
        let (tx, rx) = channel::<BodyMsg>();
        tx.send(BodyMsg::Data(b"abc".to_vec())).expect("send");
        tx.send(BodyMsg::Data(b"def".to_vec())).expect("send");
        tx.send(BodyMsg::End).expect("send");
        let rx = Mutex::new(rx);
        let buf = Mutex::new(BodyBuf::default());
        let (credit, _peer) = credit_back(&state);
        let bytes = pump(&state, &rx, &buf, &credit, Spec::All).expect("body");
        assert_eq!(bytes, b"abcdef");
        assert_eq!(
            state.flow().body_left(),
            i64::from(blastit_frame::INITIAL_WINDOW),
            "every consumed byte was credited back"
        );
    }

    #[test]
    fn pump_stops_on_a_cancel() {
        let state = RequestState::new();
        let (tx, rx) = channel::<BodyMsg>();
        tx.send(BodyMsg::Cancelled).expect("send");
        let rx = Mutex::new(rx);
        let buf = Mutex::new(BodyBuf::default());
        let (credit, _peer) = credit_back(&state);
        assert_eq!(
            pump(&state, &rx, &buf, &credit, Spec::All),
            Err(InputError::Cancelled)
        );

        // The cancelled flag alone is enough, even with nothing on the channel.
        let state = RequestState::new();
        let (_tx, rx) = channel::<BodyMsg>();
        let rx = Mutex::new(rx);
        state.cancel();
        assert_eq!(
            pump(&state, &rx, &buf, &credit, Spec::All),
            Err(InputError::Cancelled)
        );
    }

    /// A credit back-end over a live socketpair, plus the far end, which the
    /// caller keeps alive for as long as it writes.
    fn credit_back(state: &Arc<RequestState>) -> (CreditBack, UnixStream) {
        let (ours, peer) = UnixStream::pair().expect("socketpair");
        let writer = Arc::new(Writer::new(ours, blastit_frame::Limits::default()));
        (
            CreditBack {
                writer,
                id: 1,
                state: Arc::clone(state),
            },
            peer,
        )
    }
}
