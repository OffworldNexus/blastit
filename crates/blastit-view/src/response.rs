//! `start_response`, the `write` callable, and the response as frames.

use std::sync::{Arc, Mutex};

use blastit_frame::{Frame, RespHead};
use pyo3::exceptions::{PyOSError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyString, PyTuple};

use crate::environ::latin1_bytes;
use crate::request::RequestState;
use crate::sync::lock;
use crate::wire::{WriteError, Writer};

/// The status line and headers `start_response` stored but did not send.
#[derive(Clone)]
struct PendingHead {
    status: u16,
    reason: Vec<u8>,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
}

/// Whether a response head exists, and whether it has gone out.
#[derive(Default)]
struct HeadState {
    pending: Option<PendingHead>,
    sent: bool,
}

/// What can go wrong while writing a response.
#[derive(Debug)]
pub(crate) enum RespError {
    /// The application wrote a body, or finished, without calling
    /// `start_response`. PEP 3333 allows no response without one.
    NotStarted,
    /// The channel write failed, or the flow-control window was closed: the
    /// rest of the response cannot be delivered.
    Channel(WriteError),
}

/// The response of one request: the state machine behind `start_response`, the
/// `write` callable and the body iterator.
///
/// PEP 3333's laziness lives here: `start_response` only *stores* the head, and
/// [`Response::write_chunk`] sends it as `RESP_HEAD` just before the first
/// non-empty body chunk — or [`Response::finish`] does, if the application
/// returns an empty body. That way a generator that raises before its first
/// chunk can still be answered with a `500`.
///
/// Body bytes are spent against the request's flow-control window (§3.1), so a
/// request thread whose client is not reading blocks in its write instead of
/// filling the channel.
pub(crate) struct Response {
    writer: Arc<Writer>,
    id: u32,
    state: Arc<RequestState>,
    head: Mutex<HeadState>,
}

impl Response {
    /// The response of request `id`.
    pub(crate) fn new(writer: Arc<Writer>, id: u32, state: Arc<RequestState>) -> Self {
        Self {
            writer,
            id,
            state,
            head: Mutex::new(HeadState::default()),
        }
    }

    /// Whether `start_response` has been called at all.
    pub(crate) fn head_started(&self) -> bool {
        let head = lock(&self.head);
        head.pending.is_some() || head.sent
    }

    /// Whether the response head has already gone out.
    ///
    /// This is PEP 3333's "have the headers been sent?", which decides whether
    /// a late `start_response` re-raises `exc_info` and whether an exception is
    /// answered with a `500` or with `RESP_ABORT`.
    pub(crate) fn head_sent(&self) -> bool {
        lock(&self.head).sent
    }

    /// Store a head from `start_response`.
    ///
    /// # Errors
    ///
    /// `false` when the head already went out: PEP 3333 allows no second
    /// `start_response` after that.
    pub(crate) fn set_head(
        &self,
        status: u16,
        reason: Vec<u8>,
        headers: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> bool {
        let mut head = lock(&self.head);
        if head.sent {
            return false;
        }
        head.pending = Some(PendingHead {
            status,
            reason,
            headers,
        });
        true
    }

    /// Write one body chunk: flush the head first, then the chunk.
    ///
    /// An empty chunk is ignored entirely, as PEP 3333 requires, so it neither
    /// flushes the head nor produces a frame.
    pub(crate) fn write_chunk(&self, py: Python<'_>, data: &[u8]) -> Result<(), RespError> {
        if data.is_empty() || self.state.output_closed() {
            return Ok(());
        }
        self.flush_head(py)?;
        // A cancel may have landed while the head was being written; the chunk
        // is not sent then either.
        if self.state.output_closed() {
            return Ok(());
        }
        self.send_body(py, data)
    }

    /// Finish a response normally: flush the head if the body was empty, then
    /// `RESP_END`.
    pub(crate) fn finish(&self, py: Python<'_>) -> Result<(), RespError> {
        if self.state.output_closed() {
            return Ok(());
        }
        if !self.head_started() {
            return Err(RespError::NotStarted);
        }
        if !self.state.claim_terminal() {
            return Ok(());
        }
        self.flush_head(py)?;
        self.send_frame(py, Frame::RespEnd)
    }

    /// Answer an application that failed before its head was sent.
    ///
    /// A short `500` takes the place of the response the application never
    /// produced; the traceback itself already went to stderr.
    pub(crate) fn send_internal_error(&self, py: Python<'_>) -> Result<(), RespError> {
        const BODY: &[u8] = b"Internal Server Error\n";
        if self.state.output_closed() || !self.state.claim_terminal() {
            return Ok(());
        }
        let head = RespHead {
            status: 500,
            reason: b"Internal Server Error".to_vec(),
            headers: vec![
                (
                    b"Content-Type".to_vec(),
                    b"text/plain; charset=utf-8".to_vec(),
                ),
                (
                    b"Content-Length".to_vec(),
                    BODY.len().to_string().into_bytes(),
                ),
            ],
        };
        self.send_frame(py, Frame::RespHead(head))?;
        self.send_body(py, BODY)?;
        self.send_frame(py, Frame::RespEnd)
    }

    /// Abandon a response whose head is already on the wire.
    pub(crate) fn abort(&self, py: Python<'_>, reason: &str) -> Result<(), RespError> {
        if self.state.is_cancelled() || !self.state.claim_terminal() {
            return Ok(());
        }
        self.send_frame(
            py,
            Frame::RespAbort {
                reason: reason.to_string(),
            },
        )
    }

    /// Send the stored head, if it has not gone out yet.
    ///
    /// The head is only marked as sent once the frame is on the wire, so a
    /// failing write leaves it pending: the response is then decided by whether
    /// `start_response` had been called, not by whether a frame happened to
    /// make it out.
    fn flush_head(&self, py: Python<'_>) -> Result<(), RespError> {
        let pending = {
            let head = lock(&self.head);
            if head.sent {
                return Ok(());
            }
            match &head.pending {
                Some(pending) => pending.clone(),
                None => return Err(RespError::NotStarted),
            }
        };
        self.send_frame(
            py,
            Frame::RespHead(RespHead {
                status: pending.status,
                reason: pending.reason,
                headers: pending.headers,
            }),
        )?;
        let mut head = lock(&self.head);
        head.sent = true;
        head.pending = None;
        Ok(())
    }

    /// One frame, written with the GIL released.
    fn send_frame(&self, py: Python<'_>, frame: Frame) -> Result<(), RespError> {
        py.detach(move || self.writer.write_frame(&frame, self.id))
            .map_err(RespError::Channel)
    }

    /// The body of one chunk, split into `RESP_BODY` frames and spent against
    /// the flow-control window, with the GIL released: neither a slow scheduler
    /// nor a slow client holds up the rest of the process.
    fn send_body(&self, py: Python<'_>, data: &[u8]) -> Result<(), RespError> {
        let flow = self.state.flow();
        py.detach(|| self.writer.write_body(self.id, data, flow))
            .map_err(RespError::Channel)
    }
}

/// The WSGI `start_response` callable of one request.
#[pyclass]
pub(crate) struct StartResponse {
    resp: Arc<Response>,
}

impl StartResponse {
    /// The callable an application gets in its `environ` call.
    pub(crate) fn new(resp: Arc<Response>) -> Self {
        Self { resp }
    }
}

#[pymethods]
impl StartResponse {
    /// `start_response(status, headers, exc_info=None)` -> `write`.
    ///
    /// The head is validated and stored, never sent: PEP 3333 wants it on the
    /// wire just before the first body chunk.
    ///
    /// `exc_info` is what an application passes from inside an `except` block —
    /// `sys.exc_info()` or the exception itself. Once the head has gone out it
    /// is re-raised, as PEP 3333 demands; before that it is ignored and the new
    /// head replaces the stored one, which is what makes a late failure
    /// answerable at all.
    #[pyo3(signature = (status, headers, exc_info = None))]
    fn __call__<'py>(
        &self,
        py: Python<'py>,
        status: &Bound<'py, PyAny>,
        headers: &Bound<'py, PyAny>,
        exc_info: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let exc_info: Option<&Bound<'py, PyAny>> = exc_info.filter(|exc| !exc.is_none());
        if let Some(exc) = exc_info
            && self.resp.head_sent()
        {
            return Err(exception_from(exc)?);
        }

        // A second `start_response` without `exc_info` is an application bug
        // (PEP 3333): the response would be the one the application already
        // described, and silently replacing it would hide the mistake.
        if exc_info.is_none() && self.resp.head_started() {
            return Err(PyRuntimeError::new_err(
                "start_response was called twice without exc_info",
            ));
        }

        let text = status
            .cast::<PyString>()
            .map_err(|_| PyTypeError::new_err("status must be a str"))?
            .to_str()?;
        let (code, reason) = parse_status(text)
            .map_err(|detail| PyValueError::new_err(format!("status: {detail}")))?;
        let headers = parse_headers(headers)?;
        if !self.resp.set_head(code, reason, headers) {
            return Err(PyRuntimeError::new_err(
                "start_response called after the response headers were sent",
            ));
        }

        Ok(Bound::new(
            py,
            WriteCallable {
                resp: Arc::clone(&self.resp),
            },
        )?
        .into_any())
    }
}

/// The `write(data)` callable `start_response` returns.
#[pyclass]
pub(crate) struct WriteCallable {
    resp: Arc<Response>,
}

#[pymethods]
impl WriteCallable {
    /// `write(data)` — send a body chunk now, flushing the head if needed.
    fn __call__(&self, py: Python<'_>, data: &Bound<'_, PyAny>) -> PyResult<()> {
        let bytes = data
            .cast::<pyo3::types::PyBytes>()
            .map_err(|_| PyTypeError::new_err("write() argument must be bytes"))?;
        self.resp
            .write_chunk(py, bytes.as_bytes())
            .map_err(resp_error_to_py)
    }
}

/// The Python exception for a response failure.
pub(crate) fn resp_error_to_py(err: RespError) -> PyErr {
    match err {
        RespError::NotStarted => PyRuntimeError::new_err(
            "write() called before start_response(), or start_response() was never called",
        ),
        RespError::Channel(WriteError::Io(err)) => {
            PyOSError::new_err(format!("view channel is gone: {err}"))
        }
        RespError::Channel(WriteError::WindowClosed) => PyOSError::new_err(
            "the response's flow-control window was closed before the body was written",
        ),
        RespError::Channel(WriteError::Frame(err)) => {
            PyValueError::new_err(format!("response frame could not be encoded: {err}"))
        }
    }
}

/// The exception to re-raise for a `start_response`'s `exc_info`.
///
/// PEP 3333 spells `exc_info` the way `sys.exc_info()` does — a
/// `(type, value, traceback)` tuple — so the *value*, element 1, is the
/// exception to re-raise. Applications also pass the exception itself, which
/// PEP 3333's wording allows and every server accepts.
fn exception_from(exc_info: &Bound<'_, PyAny>) -> PyResult<PyErr> {
    if let Ok(tuple) = exc_info.cast::<PyTuple>()
        && tuple.len() == 3
    {
        let value = tuple.get_item(1)?;
        if !value.is_none() {
            return Ok(PyErr::from_value(value));
        }
        // `(None, None, None)`: outside an `except` block there is nothing to
        // re-raise, and the type slot is the only thing left to report.
        return Ok(PyErr::from_value(tuple.get_item(0)?));
    }
    Ok(PyErr::from_value(exc_info.clone()))
}

/// Parse PEP 3333's `status` string into a code and a reason phrase.
///
/// `"200 OK"` is the normal shape: three digits, a space, and a reason that is
/// allowed to be empty. A bare `"200"` is accepted too — plenty of applications
/// pass one — but anything else, including a fourth character that is not a
/// space, is not a status line and must not reach the client.
fn parse_status(text: &str) -> Result<(u16, Vec<u8>), String> {
    let bytes = text.as_bytes();
    if bytes.len() < 3 {
        return Err("must be at least three characters, like \"200 OK\"".to_string());
    }
    if !bytes[..3].iter().all(u8::is_ascii_digit) {
        return Err("must start with a three-digit status code".to_string());
    }
    let code: u16 = text[..3]
        .parse()
        .map_err(|_| "bad status code".to_string())?;
    if !(blastit_frame::MIN_STATUS..=blastit_frame::MAX_STATUS).contains(&code) {
        return Err(format!("status code {code} is outside 100..=999"));
    }
    let reason = match bytes.len() {
        3 => "",
        _ if bytes[3] == b' ' => &text[4..],
        _ => return Err("the status code must be followed by a space".to_string()),
    };
    let reason =
        latin1_bytes(reason).ok_or_else(|| "the reason phrase must be latin-1".to_string())?;
    Ok((code, reason))
}

/// Parse PEP 3333's `headers` list.
///
/// Names and values must be `str`, and are latin-1: those are the bytes that
/// travel, and a character above `U+00FF` has no byte to travel as.
fn parse_headers(headers: &Bound<'_, PyAny>) -> PyResult<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut out = Vec::new();
    for item in headers.try_iter()? {
        let item = item?;
        if item.len()? != 2 {
            return Err(PyValueError::new_err(
                "each header must be a (name, value) pair",
            ));
        }
        let name: String = item
            .get_item(0)?
            .extract()
            .map_err(|_| PyTypeError::new_err("header names must be str"))?;
        let value: String = item
            .get_item(1)?
            .extract()
            .map_err(|_| PyTypeError::new_err("header values must be str"))?;
        let name = latin1_bytes(&name)
            .ok_or_else(|| PyValueError::new_err("header names must be latin-1"))?;
        if name.is_empty() {
            return Err(PyValueError::new_err("header names must not be empty"));
        }
        let value = latin1_bytes(&value)
            .ok_or_else(|| PyValueError::new_err("header values must be latin-1"))?;
        out.push((name, value));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blastit_frame::Limits;
    use std::os::unix::net::UnixStream;

    #[test]
    fn a_status_line_parses_into_a_code_and_a_reason() {
        assert_eq!(parse_status("200 OK"), Ok((200, b"OK".to_vec())));
        assert_eq!(
            parse_status("404 Not Found"),
            Ok((404, b"Not Found".to_vec()))
        );
        assert_eq!(parse_status("500"), Ok((500, Vec::new())));
        assert_eq!(parse_status("204 "), Ok((204, Vec::new())));
        assert_eq!(parse_status("599 café"), Ok((599, b"caf\xe9".to_vec())));
        assert_eq!(
            parse_status("100 Continue"),
            Ok((100, b"Continue".to_vec()))
        );
        assert_eq!(parse_status("999 x"), Ok((999, b"x".to_vec())));
    }

    #[test]
    fn a_bad_status_line_is_rejected() {
        for bad in [
            "", "2", "20", "20a OK", "abc OK", "099 Low", "1000 Big", "2000K", "200OK", "0200 OK",
        ] {
            assert!(parse_status(bad).is_err(), "{bad:?} should be rejected");
        }
        // A wide character in the reason phrase has no latin-1 byte.
        assert!(parse_status("200 ✓").is_err());
    }

    #[test]
    fn a_response_stays_quiet_when_the_head_was_already_claimed() {
        let (ours, _theirs) = UnixStream::pair().expect("socketpair");
        let writer = Arc::new(Writer::new(ours, Limits::default()));
        let state = RequestState::new();
        let resp = Response::new(Arc::clone(&writer), 1, Arc::clone(&state));
        assert!(!resp.head_sent());
        assert!(!resp.head_started());

        // The I/O thread got there first: the response must stay quiet.
        assert!(state.claim_terminal());
        Python::attach(|py| {
            assert!(resp.finish(py).is_ok());
            assert!(resp.abort(py, "late").is_ok());
            assert!(resp.send_internal_error(py).is_ok());
            assert!(resp.write_chunk(py, b"body").is_ok());
        });
    }

    #[test]
    fn a_response_without_start_response_refuses_to_finish() {
        let (ours, _theirs) = UnixStream::pair().expect("socketpair");
        let writer = Arc::new(Writer::new(ours, Limits::default()));
        let state = RequestState::new();
        let resp = Response::new(writer, 1, state);
        Python::attach(|py| {
            assert!(matches!(resp.finish(py), Err(RespError::NotStarted)));
            assert!(matches!(
                resp.write_chunk(py, b"body"),
                Err(RespError::NotStarted)
            ));
        });
    }

    #[test]
    fn storing_a_head_after_it_was_sent_is_refused() {
        let (ours, _theirs) = UnixStream::pair().expect("socketpair");
        let writer = Arc::new(Writer::new(ours, Limits::default()));
        let state = RequestState::new();
        let resp = Response::new(writer, 1, state);
        assert!(resp.set_head(200, b"OK".to_vec(), Vec::new()));
        assert!(!resp.head_sent(), "storing is not sending");
        Python::attach(|py| {
            // Flushing happens on the first non-empty chunk.
            assert!(resp.write_chunk(py, b"body").is_ok());
        });
        assert!(resp.head_sent());
        assert!(!resp.set_head(500, Vec::new(), Vec::new()));
    }
}
