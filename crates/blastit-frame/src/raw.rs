//! The raw layer: a frame whose payload is still opaque bytes.
//!
//! The scheduler forwards most frames from one channel to another after
//! rewriting the id. Decoding them into a [`Frame`] and encoding them again
//! would copy every header and validate every field twice, so this layer keeps
//! the payload as it arrived.

use crate::error::FrameError;
use crate::frame::{Frame, TypeCode};
use crate::limits::Limits;
use crate::wire::{HEADER_LEN, MIN_LENGTH, len_u32};

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
