//! The typed frame vocabulary for `http.chan` and `view.chan`
//! (ARCHITECTURE.md §3, §3.1).
//!
//! # Payload encoding
//!
//! The header is fixed (`u32 length | u8 type | u32 id`, little-endian). The
//! payload of each frame kind is a flat sequence of fields, with no padding
//! and no optional parts:
//!
//! * `u8`, `u16`, `u32`, `i32`: the little-endian integer itself.
//! * `bytes32`: a `u32` byte length followed by that many raw bytes.
//! * `string32`: a `bytes32` that must be valid UTF-8.
//! * `count × …`: a `u16` count followed by that many entries.
//!
//! | Type | Code | Payload |
//! |---|---|---|
//! | `REQ_HEAD` | `0x01` | `method bytes32`, `target bytes32`, `headers` (`u16` count of `name bytes32` + `value bytes32`), `remote_addr string32`, `scheme u8`, `server_name string32`, `server_port u16` |
//! | `REQ_BODY` | `0x02` | the body bytes, raw |
//! | `REQ_END` | `0x03` | empty |
//! | `CANCEL` | `0x04` | empty |
//! | `RESP_HEAD` | `0x05` | `status u16`, `reason bytes32`, `headers` as in `REQ_HEAD` |
//! | `RESP_BODY` | `0x06` | the body bytes, raw |
//! | `RESP_END` | `0x07` | empty |
//! | `RESP_ABORT` | `0x08` | `reason string32` |
//! | `DONE` | `0x09` | `outcome u8` |
//! | `READY` | `0x0A` | empty |
//!
//! `READY` is not in §3.1's table but is what a freshly forked view sends on
//! `view.chan` before the scheduler may dispatch to it (§4). It is the only
//! code outside `0x01`–`0x09`, and it is view-chan only.
//!
//! Decoding is strict: a payload with trailing bytes, a field that does not
//! fit, an undefined enum value or a non-UTF-8 `string32` is an error. Nothing
//! in this module panics on input, whatever the bytes are.
//!
//! Beyond the encoding, two fields are range-checked in both directions:
//! `RESP_HEAD.status` must be in [`MIN_STATUS`]`..=`[`MAX_STATUS`]
//! (`100..=999`), and [`crate::ExitStatus::Signal`] must be a plausible signal
//! number (see [`crate::MIN_SIGNAL`], [`crate::MAX_SIGNAL`]).
//!
//! # Log safety
//!
//! [`Debug`](core::fmt::Debug) is implemented by hand for [`Frame`],
//! [`ReqHead`] and [`RespHead`], and nowhere prints a header value or a body:
//! those routinely carry personal data and secrets and are never logged
//! (ARCHITECTURE.md §8.1). A head prints its method, the *path* of its target
//! (query strings are dropped), its header names, and its connection facts; a
//! body prints its length. Use the accessors when the bytes themselves are
//! needed.
//!
//! ```text
//! ReqHead { method: "GET", path: "/search", headers: ["Host", "Cookie"],
//!           remote_addr: "203.0.113.7:1234", scheme: Https, server_port: 443, .. }
//! RespBody { len: 5120 }
//! ```
//!
//! `server_name` is the `Host` header's value, so it is left out too.

use crate::ctl::Ctl;
use crate::error::FrameError;
use crate::wire::{Reader, Sink, len_u16};

/// Which kind of frame a payload belongs to.
///
/// The discriminant *is* the wire type code; [`TypeCode::code`] and
/// [`TypeCode::from_code`] convert in both directions. Codes `0x01`–`0x0A` are
/// the request/response family shared by `http.chan` and `view.chan`; codes
/// `0x40`–`0x44` are the `ctl.chan` messages. The two ranges are disjoint on
/// purpose, so a frame forwarded between channels can never be mistaken for a
/// control message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum TypeCode {
    /// Request head: method, target, headers and connection facts.
    ReqHead = 0x01,
    /// A chunk of the request body.
    ReqBody = 0x02,
    /// The request body is complete.
    ReqEnd = 0x03,
    /// The client went away; the worker may stop.
    Cancel = 0x04,
    /// Response head: status, reason and headers.
    RespHead = 0x05,
    /// A chunk of the response body.
    RespBody = 0x06,
    /// The response is complete.
    RespEnd = 0x07,
    /// The response failed after the head was sent.
    RespAbort = 0x08,
    /// The request's worker slot is released.
    Done = 0x09,
    /// A newly forked view is ready to serve (`view.chan`).
    Ready = 0x0A,
    /// Ask init to fork a view process.
    Spawn = 0x40,
    /// Answer to `Spawn`, carrying the new pid.
    Spawned = 0x41,
    /// Ask init to retire a view process.
    Retire = 0x42,
    /// A view process exited.
    Exited = 0x43,
    /// Finish in-flight work and stop spawning.
    Drain = 0x44,
}

impl TypeCode {
    /// The wire code of this frame kind.
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Look up a type code.
    ///
    /// # Errors
    ///
    /// [`FrameError::UnknownType`] for any code this crate does not define.
    pub const fn from_code(code: u8) -> Result<Self, FrameError> {
        match code {
            0x01 => Ok(Self::ReqHead),
            0x02 => Ok(Self::ReqBody),
            0x03 => Ok(Self::ReqEnd),
            0x04 => Ok(Self::Cancel),
            0x05 => Ok(Self::RespHead),
            0x06 => Ok(Self::RespBody),
            0x07 => Ok(Self::RespEnd),
            0x08 => Ok(Self::RespAbort),
            0x09 => Ok(Self::Done),
            0x0A => Ok(Self::Ready),
            0x40 => Ok(Self::Spawn),
            0x41 => Ok(Self::Spawned),
            0x42 => Ok(Self::Retire),
            0x43 => Ok(Self::Exited),
            0x44 => Ok(Self::Drain),
            _ => Err(FrameError::UnknownType { ty: code }),
        }
    }

    /// Whether this kind belongs to the `ctl.chan` range.
    ///
    /// `ctl.chan` frames always carry `id == 0`; encoding or decoding one with
    /// another id is [`FrameError::IdNotZero`].
    pub const fn is_ctl(self) -> bool {
        matches!(
            self,
            Self::Spawn | Self::Spawned | Self::Retire | Self::Exited | Self::Drain
        )
    }

    /// Whether this kind has no stream and therefore must use id 0.
    ///
    /// That is every `ctl.chan` message, plus `READY`, which describes the
    /// process rather than a request.
    pub const fn requires_zero_id(self) -> bool {
        self.is_ctl() || matches!(self, Self::Ready)
    }

    /// The frame's name as it appears in ARCHITECTURE.md §3.1.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ReqHead => "REQ_HEAD",
            Self::ReqBody => "REQ_BODY",
            Self::ReqEnd => "REQ_END",
            Self::Cancel => "CANCEL",
            Self::RespHead => "RESP_HEAD",
            Self::RespBody => "RESP_BODY",
            Self::RespEnd => "RESP_END",
            Self::RespAbort => "RESP_ABORT",
            Self::Done => "DONE",
            Self::Ready => "READY",
            Self::Spawn => "SPAWN",
            Self::Spawned => "SPAWNED",
            Self::Retire => "RETIRE",
            Self::Exited => "EXITED",
            Self::Drain => "DRAIN",
        }
    }
}

/// The scheme the client used, as far as the load balancer tells us.
///
/// Wire value `0` is `Http`, `1` is `Https`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scheme {
    /// Plain HTTP.
    #[default]
    Http,
    /// HTTPS, terminated upstream by the load balancer.
    Https,
}

impl Scheme {
    /// The wire value of this scheme.
    pub const fn code(self) -> u8 {
        match self {
            Self::Http => 0,
            Self::Https => 1,
        }
    }

    /// Look up a scheme.
    ///
    /// # Errors
    ///
    /// [`FrameError::BadEnum`] for any value other than `0` or `1`.
    pub const fn from_code(value: u8) -> Result<Self, FrameError> {
        match value {
            0 => Ok(Self::Http),
            1 => Ok(Self::Https),
            other => Err(FrameError::BadEnum {
                field: "scheme",
                value: other as u64,
            }),
        }
    }
}

/// How a request's worker thread finished, carried by [`Frame::Done`].
///
/// It only tells the scheduler whether the slot may be reused and what to log;
/// the response itself was already streamed to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DoneOutcome {
    /// The response was written out in full.
    #[default]
    Completed,
    /// The response was abandoned, usually because the client disconnected.
    Aborted,
    /// The WSGI callable raised.
    Errored,
}

impl DoneOutcome {
    /// The wire value of this outcome.
    pub const fn code(self) -> u8 {
        match self {
            Self::Completed => 0,
            Self::Aborted => 1,
            Self::Errored => 2,
        }
    }

    /// Look up an outcome.
    ///
    /// # Errors
    ///
    /// [`FrameError::BadEnum`] for any value other than `0`, `1` or `2`.
    pub const fn from_code(value: u8) -> Result<Self, FrameError> {
        match value {
            0 => Ok(Self::Completed),
            1 => Ok(Self::Aborted),
            2 => Ok(Self::Errored),
            other => Err(FrameError::BadEnum {
                field: "outcome",
                value: other as u64,
            }),
        }
    }
}

/// The head of a request, rebuilt into a WSGI `environ` by the view.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct ReqHead {
    /// Request method, verbatim and case-preserved.
    pub method: Vec<u8>,
    /// Request target (origin form, e.g. `/x?y=1`), verbatim.
    pub target: Vec<u8>,
    /// Headers in the order they arrived, names and values as received.
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// Client address as the front end saw it.
    pub remote_addr: String,
    /// Scheme, from the load balancer's `X-Forwarded-Proto`.
    pub scheme: Scheme,
    /// Host name.
    pub server_name: String,
    /// Host port.
    pub server_port: u16,
}

/// The `Debug` view of a request head, for logs (ARCHITECTURE.md §8.1).
///
/// It prints what the `request` event is allowed to carry and nothing else:
/// the method, the *path* of the target (a query string is never logged), the
/// remote address, the scheme and port, and the header **names**.
/// `server_name` is the `Host` header's value and header values are never
/// logged, so it is left out along with every value in `headers`.
impl core::fmt::Debug for ReqHead {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReqHead")
            .field("method", &BytesAsText(&self.method))
            .field("path", &PathOnly(&self.target))
            .field("headers", &HeaderNames(&self.headers))
            .field("remote_addr", &self.remote_addr)
            .field("scheme", &self.scheme)
            .field("server_port", &self.server_port)
            .finish_non_exhaustive()
    }
}

/// Lowest response status this crate accepts; below it, a "status" is not a
/// response status at all (interim `1xx` answers are perfectly valid).
pub const MIN_STATUS: u16 = 100;

/// Highest response status this crate accepts.
///
/// HTTP's own registry stops at `511`, but nothing stops an application from
/// having its own code in the `5xx` block, so the ceiling is the last
/// three-digit status.
pub const MAX_STATUS: u16 = 999;

/// The head of a response, sent by the view before any body frame.
#[derive(Clone, PartialEq, Eq)]
pub struct RespHead {
    /// HTTP status code, in `100..=999`.
    pub status: u16,
    /// Reason phrase; may be empty, in which case the front end picks one.
    pub reason: Vec<u8>,
    /// Response headers, in the order the app produced them.
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Default for RespHead {
    /// A `200` head with no reason and no headers.
    ///
    /// Deliberately not derived: a default `0` status could never be encoded or
    /// decoded, so [`RespHead::default`] produces a valid one.
    fn default() -> Self {
        Self {
            status: 200,
            reason: Vec::new(),
            headers: Vec::new(),
        }
    }
}

impl RespHead {
    /// Reject a status outside the range a response may carry.
    ///
    /// # Errors
    ///
    /// [`FrameError::BadStatus`] for anything outside `100..=999`.
    pub const fn validated(&self) -> Result<(), FrameError> {
        if self.status < MIN_STATUS || self.status > MAX_STATUS {
            return Err(FrameError::BadStatus {
                status: self.status,
            });
        }
        Ok(())
    }
}

/// The `Debug` view of a response head, for logs (ARCHITECTURE.md §8.1).
///
/// Only the status and the header **names**: header values are never logged,
/// and the reason phrase is application text.
impl core::fmt::Debug for RespHead {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RespHead")
            .field("status", &self.status)
            .field("headers", &HeaderNames(&self.headers))
            .finish_non_exhaustive()
    }
}

/// One frame: its kind and its fields.
///
/// The stream a frame belongs to is carried by the frame's `id`, not by the
/// frame itself (see [`crate::Decoder::next_frame`] and [`crate::encode`]).
#[derive(Clone, PartialEq, Eq)]
pub enum Frame {
    /// A request head; starts a new stream.
    ReqHead(ReqHead),
    /// A chunk of the request body.
    ReqBody(Vec<u8>),
    /// The request body is complete.
    ReqEnd,
    /// The client went away.
    Cancel,
    /// A response head.
    RespHead(RespHead),
    /// A chunk of the response body.
    RespBody(Vec<u8>),
    /// The response is complete.
    RespEnd,
    /// The response failed after its head was sent.
    RespAbort {
        /// Human-readable reason, for logs.
        reason: String,
    },
    /// The worker thread released the stream's worker slot.
    Done {
        /// How the request ended.
        outcome: DoneOutcome,
    },
    /// A freshly forked view is ready (`view.chan`, id 0).
    Ready,
    /// A `ctl.chan` message; always sent with `id == 0`.
    Ctl(Ctl),
}

/// The `Debug` view of a frame, for logs (ARCHITECTURE.md §8.1).
///
/// Header values and bodies are never printed: a body shows its length, and a
/// head goes through the redacting [`ReqHead`] / [`RespHead`] implementations.
/// Use the accessors when the real bytes are needed.
impl core::fmt::Debug for Frame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ReqHead(head) => f.debug_tuple("ReqHead").field(head).finish(),
            Self::ReqBody(body) => f.debug_struct("ReqBody").field("len", &body.len()).finish(),
            Self::ReqEnd => f.write_str("ReqEnd"),
            Self::Cancel => f.write_str("Cancel"),
            Self::RespHead(head) => f.debug_tuple("RespHead").field(head).finish(),
            Self::RespBody(body) => f
                .debug_struct("RespBody")
                .field("len", &body.len())
                .finish(),
            Self::RespEnd => f.write_str("RespEnd"),
            Self::RespAbort { reason } => {
                f.debug_struct("RespAbort").field("reason", reason).finish()
            }
            Self::Done { outcome } => f.debug_tuple("Done").field(outcome).finish(),
            Self::Ready => f.write_str("Ready"),
            Self::Ctl(msg) => f.debug_tuple("Ctl").field(msg).finish(),
        }
    }
}

/// Prints a byte string as lossy UTF-8: a method, a path or a header name is
/// useful in a log even when it is not valid UTF-8, and it is never a secret.
struct BytesAsText<'a>(&'a [u8]);

impl core::fmt::Debug for BytesAsText<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", String::from_utf8_lossy(self.0))
    }
}

/// Prints a request target with its query string and fragment removed: those
/// routinely carry personal data and are never logged (ARCHITECTURE.md §8.1).
struct PathOnly<'a>(&'a [u8]);

impl core::fmt::Debug for PathOnly<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let end = self
            .0
            .iter()
            .position(|byte| *byte == b'?' || *byte == b'#')
            .unwrap_or(self.0.len());
        BytesAsText(&self.0[..end]).fmt(f)
    }
}

/// Prints the names of a header list and none of the values.
struct HeaderNames<'a>(&'a [(Vec<u8>, Vec<u8>)]);

impl core::fmt::Debug for HeaderNames<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut list = f.debug_list();
        for (name, _value) in self.0 {
            list.entry(&BytesAsText(name));
        }
        list.finish()
    }
}

impl Frame {
    /// The wire type code of this frame.
    ///
    /// [`TypeCode::from_code`] turns it back into the enum, except for control
    /// messages, which use [`Ctl::type_code`].
    pub const fn code(&self) -> u8 {
        match self {
            Self::ReqHead(_) => TypeCode::ReqHead.code(),
            Self::ReqBody(_) => TypeCode::ReqBody.code(),
            Self::ReqEnd => TypeCode::ReqEnd.code(),
            Self::Cancel => TypeCode::Cancel.code(),
            Self::RespHead(_) => TypeCode::RespHead.code(),
            Self::RespBody(_) => TypeCode::RespBody.code(),
            Self::RespEnd => TypeCode::RespEnd.code(),
            Self::RespAbort { .. } => TypeCode::RespAbort.code(),
            Self::Done { .. } => TypeCode::Done.code(),
            Self::Ready => TypeCode::Ready.code(),
            Self::Ctl(msg) => msg.code(),
        }
    }

    /// The `ctl.chan` message, when this is one.
    pub const fn as_ctl(&self) -> Option<&Ctl> {
        match self {
            Self::Ctl(msg) => Some(msg),
            _ => None,
        }
    }

    /// Whether this frame has no stream and therefore must carry id 0.
    ///
    /// That is every `ctl.chan` message, plus `READY`, which is about the
    /// process rather than a request.
    pub const fn requires_zero_id(&self) -> bool {
        matches!(self, Self::Ctl(_) | Self::Ready)
    }

    /// Encode this frame's payload into `out`, without any header.
    ///
    /// The header needs the payload length, which is why the encoder runs this
    /// twice: once against a counting sink and once against the real buffer.
    /// Validation lives here too, so an invalid frame is refused before a byte
    /// of it is written.
    pub(crate) fn write_payload(&self, sink: &mut impl Sink) -> Result<(), FrameError> {
        match self {
            Self::ReqHead(head) => {
                sink.put_bytes32(&head.method, "method")?;
                sink.put_bytes32(&head.target, "target")?;
                write_headers(sink, &head.headers)?;
                sink.put_str32(&head.remote_addr, "remote_addr")?;
                sink.put_u8(head.scheme.code());
                sink.put_str32(&head.server_name, "server_name")?;
                sink.put_u16(head.server_port);
                Ok(())
            }
            Self::ReqBody(body) | Self::RespBody(body) => {
                sink.put_raw(body);
                Ok(())
            }
            Self::RespHead(head) => {
                head.validated()?;
                sink.put_u16(head.status);
                sink.put_bytes32(&head.reason, "reason")?;
                write_headers(sink, &head.headers)
            }
            Self::RespAbort { reason } => sink.put_str32(reason, "reason"),
            Self::Done { outcome } => {
                sink.put_u8(outcome.code());
                Ok(())
            }
            Self::ReqEnd | Self::Cancel | Self::RespEnd | Self::Ready => Ok(()),
            Self::Ctl(msg) => msg.write_payload(sink),
        }
    }

    /// Reject a frame without a stream that is not addressed to id 0.
    pub(crate) fn check_id(&self, id: u32) -> Result<(), FrameError> {
        if id == 0 || !self.requires_zero_id() {
            Ok(())
        } else {
            Err(FrameError::IdNotZero { id })
        }
    }

    /// Decode a payload of the given type code.
    pub(crate) fn decode(ty: u8, payload: &[u8]) -> Result<Self, FrameError> {
        let code = TypeCode::from_code(ty)?;
        let mut reader = Reader::new(code.name(), payload);
        let frame = match code {
            TypeCode::ReqHead => Self::ReqHead(ReqHead {
                method: reader.bytes32("method")?.to_vec(),
                target: reader.bytes32("target")?.to_vec(),
                headers: read_headers(&mut reader)?,
                remote_addr: reader.string32("remote_addr")?.to_owned(),
                scheme: Scheme::from_code(reader.u8("scheme")?)?,
                server_name: reader.string32("server_name")?.to_owned(),
                server_port: reader.u16("server_port")?,
            }),
            TypeCode::ReqBody => Self::ReqBody(reader.take_rest().to_vec()),
            TypeCode::ReqEnd => Self::ReqEnd,
            TypeCode::Cancel => Self::Cancel,
            TypeCode::RespHead => {
                let head = RespHead {
                    status: reader.u16("status")?,
                    reason: reader.bytes32("reason")?.to_vec(),
                    headers: read_headers(&mut reader)?,
                };
                head.validated()?;
                Self::RespHead(head)
            }
            TypeCode::RespBody => Self::RespBody(reader.take_rest().to_vec()),
            TypeCode::RespEnd => Self::RespEnd,
            TypeCode::RespAbort => Self::RespAbort {
                reason: reader.string32("reason")?.to_owned(),
            },
            TypeCode::Done => Self::Done {
                outcome: DoneOutcome::from_code(reader.u8("outcome")?)?,
            },
            TypeCode::Ready => Self::Ready,
            TypeCode::Spawn
            | TypeCode::Spawned
            | TypeCode::Retire
            | TypeCode::Exited
            | TypeCode::Drain => Self::Ctl(Ctl::decode(code, &mut reader)?),
        };
        reader.finish()?;
        Ok(frame)
    }

    /// Decode a payload the caller already owns.
    ///
    /// Body frames take the buffer as it is, so the streaming path never has to
    /// copy a body twice: once out of the read buffer, and again out of a
    /// [`crate::RawFrame`].
    pub(crate) fn decode_owned(ty: u8, payload: Vec<u8>) -> Result<Self, FrameError> {
        match TypeCode::from_code(ty)? {
            TypeCode::ReqBody => Ok(Self::ReqBody(payload)),
            TypeCode::RespBody => Ok(Self::RespBody(payload)),
            _ => Self::decode(ty, &payload),
        }
    }
}

/// A header name/value pair as it travels on the wire: bytes, not text.
type Headers = Vec<(Vec<u8>, Vec<u8>)>;

/// Write a `u16` count followed by name/value pairs.
fn write_headers(sink: &mut impl Sink, headers: &[(Vec<u8>, Vec<u8>)]) -> Result<(), FrameError> {
    sink.put_u16(len_u16(headers.len(), "header_count")?);
    for (name, value) in headers {
        sink.put_bytes32(name, "header name")?;
        sink.put_bytes32(value, "header value")?;
    }
    Ok(())
}

fn read_headers(reader: &mut Reader<'_>) -> Result<Headers, FrameError> {
    let count = reader.u16("header_count")? as usize;
    // Each entry needs at least two `u32` lengths, so a lying count cannot make
    // this allocate more than the payload could possibly hold.
    let mut headers = Vec::with_capacity(count.min(reader.remaining() / 8));
    for _ in 0..count {
        let name = reader.bytes32("header name")?.to_vec();
        let value = reader.bytes32("header value")?.to_vec();
        headers.push((name, value));
    }
    Ok(headers)
}
