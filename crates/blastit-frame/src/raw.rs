//! The raw layer: a frame whose payload is still opaque bytes.
//!
//! The scheduler forwards most frames from one channel to another after
//! rewriting the id. Decoding them into a [`Frame`] and encoding them again
//! would copy every header and validate every field twice, so this layer keeps
//! the payload as it arrived.

use crate::error::FrameError;
use crate::frame::{Frame, TypeCode};
use crate::limits::Limits;
use crate::wire::{HEADER_LEN, MIN_LENGTH, Reader, len_u32};

/// A frame split into its header fields and its undecoded payload.
///
/// `ty` is a plain `u8` rather than a [`TypeCode`] so that a frame this crate
/// does not know can still be forwarded, logged and re-encoded byte for byte.
#[derive(Clone, PartialEq, Eq)]
pub struct RawFrame {
    /// The wire type code.
    pub ty: u8,
    /// The stream or request id.
    pub id: u32,
    /// The payload, exactly as it arrived.
    pub payload: Vec<u8>,
}

/// The `Debug` view of a raw frame, for logs (ARCHITECTURE.md §8.1).
///
/// The payload is exactly the header values and body bytes the log format
/// forbids, so only its length is printed.
impl core::fmt::Debug for RawFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RawFrame")
            .field("ty", &format_args!("{:#04x}", self.ty))
            .field("id", &self.id)
            .field("len", &self.payload.len())
            .finish()
    }
}

impl RawFrame {
    /// Build a raw frame from its parts.
    pub fn new(ty: u8, id: u32, payload: Vec<u8>) -> Self {
        Self { ty, id, payload }
    }

    /// The type code, when this crate knows it.
    ///
    /// # Errors
    ///
    /// [`FrameError::UnknownType`] for a code outside the frame vocabulary.
    pub fn type_code(&self) -> Result<TypeCode, FrameError> {
        TypeCode::from_code(self.ty)
    }

    /// Whether this is a `ctl.chan` message.
    ///
    /// A code this crate does not know is not a control message.
    pub fn is_ctl(&self) -> bool {
        self.type_code().map(TypeCode::is_ctl).unwrap_or(false)
    }

    /// Whether this frame has no stream and must therefore carry id 0.
    ///
    /// A code this crate does not know is left alone: the raw layer exists to
    /// pass frames through, not to judge them.
    pub fn requires_zero_id(&self) -> bool {
        self.type_code()
            .map(TypeCode::requires_zero_id)
            .unwrap_or(false)
    }

    /// Rewrite the id in place, which is what the scheduler does when it
    /// forwards a stream between `http.chan` and `view.chan`.
    ///
    /// Rewriting an id to 0 is always allowed; [`RawFrame::encode`] refuses a
    /// non-zero id on a frame that has no stream.
    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    /// Decode the payload into a typed [`Frame`], copying it.
    ///
    /// # Errors
    ///
    /// Any [`FrameError`] the payload can raise, including
    /// [`FrameError::UnknownType`].
    pub fn decode(&self) -> Result<Frame, FrameError> {
        Frame::decode(self.ty, &self.payload)
    }

    /// Decode the payload into a typed [`Frame`], moving it.
    ///
    /// This is what the streaming decoder uses: a body frame hands its buffer
    /// straight to the [`Frame`] instead of copying it a second time.
    ///
    /// # Errors
    ///
    /// Any [`FrameError`] the payload can raise, including
    /// [`FrameError::UnknownType`].
    pub fn into_frame(self) -> Result<Frame, FrameError> {
        Frame::decode_owned(self.ty, self.payload)
    }

    /// Encode this frame, header included, into `out`.
    ///
    /// # Errors
    ///
    /// [`FrameError::IdNotZero`] when the frame has no stream (`ctl.chan` and
    /// `READY`) but carries an id, and [`FrameError::FrameTooLarge`] when the
    /// frame does not fit `limits`.
    pub fn encode(&self, limits: &Limits, out: &mut Vec<u8>) -> Result<(), FrameError> {
        if self.id != 0 && self.requires_zero_id() {
            return Err(FrameError::IdNotZero { id: self.id });
        }
        let total = HEADER_LEN + self.payload.len();
        limits.check_total(total)?;
        let length = len_u32(MIN_LENGTH as usize + self.payload.len(), "length")?;
        out.reserve(total);
        out.extend_from_slice(&length.to_le_bytes());
        out.push(self.ty);
        out.extend_from_slice(&self.id.to_le_bytes());
        out.extend_from_slice(&self.payload);
        Ok(())
    }
}

impl Frame {
    /// Encode this frame's payload into an untyped [`RawFrame`].
    ///
    /// Convenience for tests and diagnostics; the size limit is not applied
    /// here, [`crate::Limits::encode`] applies it when the header is added.
    ///
    /// # Errors
    ///
    /// [`FrameError::IdNotZero`] for a frame without a stream that carries an
    /// id, and any payload error (a field that does not fit its wire encoding).
    pub fn to_raw(&self, id: u32) -> Result<RawFrame, FrameError> {
        self.check_id(id)?;
        let mut payload = Vec::new();
        self.write_payload(&mut payload)?;
        Ok(RawFrame::new(self.code(), id, payload))
    }
}

/// A borrowed view of a `REQ_HEAD` payload, read where it lies.
///
/// The scheduler reads only a handful of a request head's fields: the method
/// and target for routing and the `request` event, the two addresses, and —
/// only when fair queuing is on — the values of the headers and cookies the
/// operator configured. Decoding the head into an owned [`crate::ReqHead`] to
/// read those, then re-encoding it for the view, copies and validates every
/// header twice; this view borrows the fields out of the payload instead, so
/// the [`RawFrame`] that carries the head onward stays exactly the bytes http
/// sent.
///
/// **Only `http.chan` may be read this way.** http is blastit's own Rust code
/// and encodes from a typed [`Frame`], so its bytes are well-formed by
/// construction. A `view.chan` frame comes from a sandboxed, potentially
/// compromised process and must keep going through the strict
/// [`crate::Decoder::next_frame`], which validates every field.
pub struct ReqHeadView<'a> {
    /// The method bytes, verbatim.
    method: &'a [u8],
    /// The target bytes, verbatim.
    target: &'a [u8],
    /// The header list, still in its wire encoding: a `u16` count followed by
    /// `name`/`value` byte strings. [`ReqHeadView::parse`] has validated its
    /// framing, so [`ReqHeadView::headers`] can walk it without checking again.
    headers: &'a [u8],
    /// How many header pairs `headers` holds.
    header_count: u16,
    /// The client address the front end resolved, validated as UTF-8.
    remote_addr: &'a str,
    /// The socket peer the request arrived on, validated as UTF-8.
    peer_addr: &'a str,
}

impl<'a> ReqHeadView<'a> {
    /// Parse the fields the scheduler reads out of a `REQ_HEAD` payload.
    ///
    /// The shape is the one documented in [`crate::frame`]: `method`, `target`,
    /// a header count and its pairs, `remote_addr`, `scheme`, `server_name`,
    /// `server_port`, `peer_addr`. Everything the scheduler does not read is
    /// walked past, and the whole payload is checked for trailing bytes, so a
    /// head that parses here is one the view's own decoder would accept too.
    ///
    /// # Errors
    ///
    /// Any [`FrameError`] a truncated or trailing payload raises, and
    /// [`FrameError::NotUtf8`] for the two UTF-8 fields this view hands out as
    /// `&str`.
    pub fn parse(payload: &'a [u8]) -> Result<Self, FrameError> {
        let mut reader = Reader::new(TypeCode::ReqHead.name(), payload);
        let method = reader.bytes32("method")?;
        let target = reader.bytes32("target")?;
        let header_count = reader.u16("header_count")?;
        let headers_start = reader.position();
        for _ in 0..header_count {
            let _ = reader.bytes32("header name")?;
            let _ = reader.bytes32("header value")?;
        }
        let headers_end = reader.position();
        let remote_addr = reader.string32("remote_addr")?;
        // `scheme`, `server_name` and `server_port` are carried to the view
        // untouched: the scheduler never reads them, so this only walks past
        // them, and the view's own strict decode still checks them.
        let _ = reader.u8("scheme")?;
        let _ = reader.string32("server_name")?;
        let _ = reader.u16("server_port")?;
        let peer_addr = reader.string32("peer_addr")?;
        reader.finish()?;
        Ok(Self {
            method,
            target,
            headers: &payload[headers_start..headers_end],
            header_count,
            remote_addr,
            peer_addr,
        })
    }

    /// The request method, verbatim.
    pub fn method(&self) -> &'a [u8] {
        self.method
    }

    /// The request target, verbatim.
    pub fn target(&self) -> &'a [u8] {
        self.target
    }

    /// The client address the front end resolved ([`crate::ReqHead::remote_addr`]).
    pub fn remote_addr(&self) -> &'a str {
        self.remote_addr
    }

    /// The socket peer the request arrived on ([`crate::ReqHead::peer_addr`]).
    pub fn peer_addr(&self) -> &'a str {
        self.peer_addr
    }

    /// The header pairs, in the order they arrived.
    ///
    /// The pairs are borrowed out of the payload, so a reader that hashes or
    /// filters them — the fair queue's inner key — pays for no copy.
    pub fn headers(&self) -> ReqHeaders<'a> {
        ReqHeaders {
            rest: self.headers,
            remaining: self.header_count,
        }
    }
}

/// The header pairs of a [`ReqHeadView`], borrowed from the payload.
#[derive(Clone)]
pub struct ReqHeaders<'a> {
    /// The pairs not yet handed out, in wire order.
    rest: &'a [u8],
    /// How many pairs are left.
    remaining: u16,
}

impl<'a> Iterator for ReqHeaders<'a> {
    type Item = (&'a [u8], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        // `ReqHeadView::parse` walked these exact bytes, so each pair is here
        // and whole; stopping early would only hide a corruption that parse
        // would already have refused.
        let (name, rest) = take_bytes32(self.rest)?;
        let (value, rest) = take_bytes32(rest)?;
        self.rest = rest;
        Some((name, value))
    }
}

/// One `u32`-length-prefixed byte string at the front of `buf`, with the rest.
///
/// `None` when `buf` is shorter than the length it claims, which a payload that
/// reached [`ReqHeadView::parse`] cannot be.
fn take_bytes32(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    let len = u32::from_le_bytes(buf.get(..4)?.try_into().ok()?) as usize;
    let (field, rest) = buf.get(4..)?.split_at_checked(len)?;
    Some((field, rest))
}
