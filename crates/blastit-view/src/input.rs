//! `wsgi.input`: the request body as a blocking Python stream.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvError};
use std::sync::{Arc, Mutex};

use pyo3::exceptions::PyOSError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyList};

use crate::request::{RequestState, lock};

/// One message from the I/O thread to a request's `wsgi.input`.
#[derive(Debug)]
pub(crate) enum BodyMsg {
    /// A chunk of the request body, in order.
    Data(Vec<u8>),
    /// The body is complete (`REQ_END`), or the channel went away.
    End,
    /// The scheduler cancelled the request (`CANCEL`).
    Cancelled,
    /// More body arrived than the view is willing to queue (see
    /// [`crate::BODY_BUFFER_LIMIT`]).
    Overflow,
}

/// Why a read failed.
///
/// Both cases are the *request's* failure, not the application's: a cancelled
/// request and an aborted one are not going to have their response delivered
/// either way. They surface as `OSError`, which is what a WSGI application
/// already has to handle for a broken connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputError {
    /// The scheduler cancelled the request.
    Cancelled,
    /// The body queue overflowed.
    Overflow,
}

impl InputError {
    /// The Python exception a read raises.
    fn to_py(self) -> PyErr {
        match self {
            Self::Cancelled => PyOSError::new_err("request cancelled, wsgi.input is closed"),
            Self::Overflow => PyOSError::new_err(
                "request body exceeded the view's buffer limit, wsgi.input is closed",
            ),
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

/// The `wsgi.input` object of one request.
///
/// It is a *stream*, not a buffer: `REQ_BODY` frames are handed to it as they
/// arrive, and a read that needs more than is buffered blocks — with the GIL
/// released — on the channel from the I/O thread until more arrives, the body
/// ends, or the request is cancelled. After `REQ_END` every read returns
/// `b""`, so iteration and `read()` terminate.
///
/// Partial reads are buffered here, so `read(1)` a thousand times does not lose
/// a thousand times' worth of bytes; nothing else is buffered anywhere.
#[pyclass]
pub(crate) struct Input {
    state: Arc<RequestState>,
    rx: Arc<Mutex<Receiver<BodyMsg>>>,
    buf: Arc<Mutex<BodyBuf>>,
}

impl Input {
    /// The input object for a request whose body arrives on `rx`.
    pub(crate) fn new(state: Arc<RequestState>, rx: Receiver<BodyMsg>) -> Self {
        Self {
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
        py.detach(move || pump(&state, &rx, &buf, spec))
            .map_err(InputError::to_py)
    }
}

#[pymethods]
impl Input {
    /// `read(size=-1)` — the whole body, or the next `size` bytes.
    ///
    /// Like a buffered file, a positive `size` blocks until that many bytes are
    /// available or the body ends, which is what frameworks do when they read
    /// `CONTENT_LENGTH` bytes in one go.
    #[pyo3(signature = (size = -1))]
    fn read<'py>(&self, py: Python<'py>, size: i64) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.read_bytes(py, spec_for(size))?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// `readline(size=-1)` — up to and including the next newline.
    #[pyo3(signature = (size = -1))]
    fn readline<'py>(&self, py: Python<'py>, size: i64) -> PyResult<Bound<'py, PyBytes>> {
        let spec = Spec::Line(if size < 0 { None } else { Some(size as usize) });
        let bytes = self.read_bytes(py, spec)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// `readlines(hint=-1)` — every remaining line.
    ///
    /// A positive `hint` stops early as soon as that many bytes of lines have
    /// been collected, as `io.BytesIO.readlines` does.
    #[pyo3(signature = (hint = -1))]
    fn readlines<'py>(&self, py: Python<'py>, hint: i64) -> PyResult<Bound<'py, PyList>> {
        let limit = if hint <= 0 { None } else { Some(hint as usize) };
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
fn spec_for(size: i64) -> Spec {
    if size < 0 {
        Spec::All
    } else {
        Spec::AtMost(size as usize)
    }
}

/// Move bytes out of the queue until `spec` can be satisfied, blocking for more
/// as needed. Runs with the GIL released.
fn pump(
    state: &RequestState,
    rx: &Mutex<Receiver<BodyMsg>>,
    buf: &Mutex<BodyBuf>,
    spec: Spec,
) -> Result<Vec<u8>, InputError> {
    let mut out = Vec::new();
    loop {
        // The flags come first: once the request is cancelled or aborted the
        // channel may never carry anything again, and a read must not block on
        // a body that will never be completed.
        if state.is_cancelled() {
            return Err(InputError::Cancelled);
        }
        if state.is_overflowed() {
            return Err(InputError::Overflow);
        }
        {
            let mut body = lock(buf);
            if take(&mut body, spec, &mut out) {
                state.consume_pending(out.len());
                return Ok(out);
            }
        }
        match lock(rx).recv() {
            Ok(BodyMsg::Data(bytes)) => lock(buf).bytes.extend(bytes),
            Ok(BodyMsg::End) => lock(buf).ended = true,
            Ok(BodyMsg::Cancelled) => return Err(InputError::Cancelled),
            Ok(BodyMsg::Overflow) => return Err(InputError::Overflow),
            // Every sender is gone: the I/O thread has finished with this
            // request, so the body is as complete as it will ever be.
            Err(RecvError) => lock(buf).ended = true,
        }
    }
}

/// Move what `spec` can be given from `body` into `out`.
///
/// Returns `false` when `spec` needs more bytes than are buffered and the body
/// has not ended, which is the caller's signal to block.
fn take(body: &mut BodyBuf, spec: Spec, out: &mut Vec<u8>) -> bool {
    let wanted = match spec {
        Spec::All => {
            if !body.ended {
                return false;
            }
            body.bytes.len()
        }
        Spec::AtMost(0) => 0,
        Spec::AtMost(size) => {
            if body.bytes.len() >= size {
                size
            } else if body.ended {
                body.bytes.len()
            } else {
                return false;
            }
        }
        Spec::Line(limit) => {
            let line = match body.bytes.iter().position(|byte| *byte == b'\n') {
                Some(at) => at + 1,
                None if body.ended => body.bytes.len(),
                None => return false,
            };
            match limit {
                Some(limit) => line.min(limit),
                None => line,
            }
        }
    };
    out.extend(body.bytes.drain(..wanted));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    /// A body with `bytes` delivered and no end.
    fn buf(bytes: &[u8]) -> BodyBuf {
        BodyBuf {
            bytes: bytes.iter().copied().collect(),
            ended: false,
        }
    }

    fn take_from(body: &mut BodyBuf, spec: Spec) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        if take(body, spec, &mut out) {
            Some(out)
        } else {
            None
        }
    }

    #[test]
    fn read_all_needs_the_end_of_the_body() {
        let mut body = buf(b"abc");
        assert_eq!(take_from(&mut body, Spec::All), None);
        body.ended = true;
        assert_eq!(take_from(&mut body, Spec::All), Some(b"abc".to_vec()));
        assert_eq!(take_from(&mut body, Spec::All), Some(Vec::new()));
    }

    #[test]
    fn read_n_blocks_until_n_are_available() {
        let mut body = buf(b"abc");
        assert_eq!(take_from(&mut body, Spec::AtMost(2)), Some(b"ab".to_vec()));
        assert_eq!(
            take_from(&mut body, Spec::AtMost(2)),
            None,
            "one byte short"
        );
        body.ended = true;
        assert_eq!(take_from(&mut body, Spec::AtMost(2)), Some(b"c".to_vec()));
        assert_eq!(take_from(&mut body, Spec::AtMost(2)), Some(Vec::new()));
    }

    #[test]
    fn read_zero_never_blocks() {
        let mut body = BodyBuf::default();
        assert_eq!(take_from(&mut body, Spec::AtMost(0)), Some(Vec::new()));
        assert_eq!(take_from(&mut body, Spec::AtMost(0)), Some(Vec::new()));
    }

    #[test]
    fn readline_stops_after_the_newline() {
        let mut body = buf(b"one\ntwo");
        assert_eq!(
            take_from(&mut body, Spec::Line(None)),
            Some(b"one\n".to_vec())
        );
        assert_eq!(
            take_from(&mut body, Spec::Line(None)),
            None,
            "no newline yet"
        );
        body.ended = true;
        assert_eq!(
            take_from(&mut body, Spec::Line(None)),
            Some(b"two".to_vec())
        );
        assert_eq!(take_from(&mut body, Spec::Line(None)), Some(Vec::new()));
    }

    #[test]
    fn readline_honours_its_size() {
        let mut body = buf(b"one\ntwo\n");
        assert_eq!(
            take_from(&mut body, Spec::Line(Some(2))),
            Some(b"on".to_vec())
        );
        // The newline wins over the size: `readline(4)` stops at the line end.
        assert_eq!(
            take_from(&mut body, Spec::Line(Some(4))),
            Some(b"e\n".to_vec())
        );
        assert_eq!(take_from(&mut body, Spec::Line(Some(0))), Some(Vec::new()));
        assert_eq!(
            take_from(&mut body, Spec::Line(None)),
            Some(b"two\n".to_vec())
        );
        body.ended = true;
        assert_eq!(take_from(&mut body, Spec::Line(None)), Some(Vec::new()));
    }

    #[test]
    fn an_empty_line_is_a_line() {
        let mut body = buf(b"\n\n");
        assert_eq!(take_from(&mut body, Spec::Line(None)), Some(b"\n".to_vec()));
        assert_eq!(take_from(&mut body, Spec::Line(None)), Some(b"\n".to_vec()));
        body.ended = true;
        assert_eq!(take_from(&mut body, Spec::Line(None)), Some(Vec::new()));
    }

    #[test]
    fn a_read_specification_maps_from_the_python_signature() {
        assert!(matches!(spec_for(-1), Spec::All));
        assert!(matches!(spec_for(-100), Spec::All));
        assert!(matches!(spec_for(0), Spec::AtMost(0)));
        assert!(matches!(spec_for(1), Spec::AtMost(1)));
        assert!(matches!(spec_for(4096), Spec::AtMost(4096)));
    }

    #[test]
    fn a_dropped_sender_ends_the_body() {
        // `pump` treats a closed channel as the end of the body, so a request
        // whose I/O thread is gone cannot hang on a read.
        let state = RequestState::new();
        let (tx, rx) = channel::<BodyMsg>();
        let rx = Mutex::new(rx);
        let buf = Mutex::new(BodyBuf::default());
        drop(tx);
        let bytes = pump(&state, &rx, &buf, Spec::All).expect("end of body");
        assert!(bytes.is_empty());
        assert!(buf.lock().expect("lock").ended);
    }

    #[test]
    fn pump_returns_what_was_queued_before_the_end() {
        let state = RequestState::new();
        let (tx, rx) = channel::<BodyMsg>();
        state.add_pending(6);
        tx.send(BodyMsg::Data(b"abc".to_vec())).expect("send");
        tx.send(BodyMsg::Data(b"def".to_vec())).expect("send");
        tx.send(BodyMsg::End).expect("send");
        let rx = Mutex::new(rx);
        let buf = Mutex::new(BodyBuf::default());
        let bytes = pump(&state, &rx, &buf, Spec::All).expect("body");
        assert_eq!(bytes, b"abcdef");
        assert_eq!(state.pending(), 0, "consumed bytes leave the queue");
    }

    #[test]
    fn pump_stops_on_a_cancel_or_an_overflow() {
        let (tx, rx) = channel::<BodyMsg>();
        let state = RequestState::new();
        let rx = Mutex::new(rx);
        let buf = Mutex::new(BodyBuf::default());
        tx.send(BodyMsg::Cancelled).expect("send");
        assert_eq!(
            pump(&state, &rx, &buf, Spec::All),
            Err(InputError::Cancelled)
        );

        let (tx, rx) = channel::<BodyMsg>();
        let state = RequestState::new();
        let rx = Mutex::new(rx);
        tx.send(BodyMsg::Overflow).expect("send");
        assert_eq!(
            pump(&state, &rx, &buf, Spec::All),
            Err(InputError::Overflow)
        );
        assert!(state.overflow());
        assert_eq!(
            pump(&state, &rx, &buf, Spec::All),
            Err(InputError::Overflow),
            "the failed state is sticky"
        );
    }
}
