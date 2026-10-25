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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    /// The wire type code.
    pub ty: u8,
    /// The stream or request id.
    pub id: u32,
    /// The payload, exactly as it arrived.
    pub payload: Vec<u8>,
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

    /// Rewrite the id in place, which is what the scheduler does when it
    /// forwards a stream between `http.chan` and `view.chan`.
    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    /// Decode the payload into a typed [`Frame`].
    ///
    /// # Errors
    ///
    /// Any [`FrameError`] the payload can raise, including
    /// [`FrameError::UnknownType`].
    pub fn decode(&self) -> Result<Frame, FrameError> {
        Frame::decode(self.ty, &self.payload)
    }

    /// Encode this frame, header included, into `out`.
    ///
    /// # Errors
    ///
    /// [`FrameError::FrameTooLarge`] when the frame does not fit `limits`.
    pub fn encode(&self, limits: &Limits, out: &mut Vec<u8>) -> Result<(), FrameError> {
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
    /// [`FrameError::IdNotZero`] for a control message with `id != 0`, and any
    /// payload error (a field that does not fit its wire encoding).
    pub fn to_raw(&self, id: u32) -> Result<RawFrame, FrameError> {
        self.check_id(id)?;
        let mut payload = Vec::new();
        self.write_payload(&mut payload)?;
        Ok(RawFrame::new(self.code(), id, payload))
    }
}
