//! The error type shared by every encoder and decoder in this crate.

use thiserror::Error;

/// Everything that can go wrong while encoding or decoding a frame.
///
/// The variants mirror the four kinds of failure the wire format can produce:
/// a bad header, a payload that does not match its frame kind, a frame that
/// does not fit the configured limit, and a value that is out of range for its
/// field.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum FrameError {
    /// The frame (header included) is larger than [`crate::Limits::max_frame`].
    #[error("frame of {size} bytes exceeds the maximum of {max} bytes")]
    FrameTooLarge {
        /// Total size the frame would need, header included.
        size: usize,
        /// Configured maximum, see [`crate::Limits::max_frame`].
        max: usize,
    },

    /// The `length` field is smaller than the fixed `type` + `id` header.
    #[error("declared frame length {length} is smaller than the 5-byte type+id header")]
    BadLength {
        /// The value read from the `length` field.
        length: u32,
    },

    /// [`crate::Limits::new`] was given a value outside the supported range.
    #[error("max_frame {max_frame} is outside the supported range {min}..={max}")]
    BadLimits {
        /// The rejected value.
        max_frame: usize,
        /// Smallest accepted value.
        min: usize,
        /// Largest accepted value.
        max: usize,
    },

    /// The type code is not part of the frame vocabulary.
    #[error("unknown frame type code {ty:#04x}")]
    UnknownType {
        /// The offending code.
        ty: u8,
    },

    /// The payload ends in the middle of a field.
    #[error("payload is truncated in field `{field}`")]
    Truncated {
        /// Name of the field that ran out of bytes.
        field: &'static str,
    },

    /// The payload has bytes left over after the last field of its frame kind.
    #[error("payload of a {ty} frame has {extra} trailing byte(s)")]
    TrailingBytes {
        /// Frame kind, as printed in the architecture's frame tables.
        ty: &'static str,
        /// Number of unused bytes.
        extra: usize,
    },

    /// An enum field holds a value this codec does not define.
    #[error("value {value} is not a valid {field}")]
    BadEnum {
        /// Name of the enum field.
        field: &'static str,
        /// The offending value.
        value: u64,
    },

    /// A field that must be UTF-8 is not.
    #[error("field `{field}` is not valid UTF-8")]
    NotUtf8 {
        /// Name of the string field.
        field: &'static str,
    },

    /// A field cannot be expressed by its wire encoding.
    #[error("field `{field}` is out of range: {detail}")]
    OutOfRange {
        /// Name of the field.
        field: &'static str,
        /// Why the value does not fit.
        detail: &'static str,
    },

    /// A response status outside the range HTTP allows.
    #[error("response status {status} is outside the 100..=999 range")]
    BadStatus {
        /// The offending status code.
        status: u16,
    },

    /// A frame that has no stream carried a non-zero id.
    ///
    /// `ctl.chan` messages and `READY` are the only frames without a stream, so
    /// they are the only ones that must use id 0.
    #[error("frames without a stream (ctl.chan and READY) must carry id 0, got {id}")]
    IdNotZero {
        /// The offending id.
        id: u32,
    },
}
