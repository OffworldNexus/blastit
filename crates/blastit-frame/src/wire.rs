//! Byte-level primitives shared by the frame encoder and the frame decoder.
//!
//! Nothing here knows about frame kinds; it only knows how a single field is
//! laid out on the wire. Every integer is little-endian.

use crate::error::FrameError;

/// Size of the fixed part of every frame header: `u32 length | u8 type | u32 id`.
pub const HEADER_LEN: usize = 9;

/// Smallest value the `length` field may hold: it always covers at least the
/// `u8 type | u32 id` that follow it.
pub const MIN_LENGTH: u32 = 5;

/// Convert a field length to the `u32` the wire format has room for.
pub(crate) fn len_u32(n: usize, field: &'static str) -> Result<u32, FrameError> {
    u32::try_from(n).map_err(|_| FrameError::OutOfRange {
        field,
        detail: "longer than u32::MAX",
    })
}

/// Convert a count to the `u16` the wire format has room for.
pub(crate) fn len_u16(n: usize, field: &'static str) -> Result<u16, FrameError> {
    u16::try_from(n).map_err(|_| FrameError::OutOfRange {
        field,
        detail: "larger than u16::MAX",
    })
}

/// Destination for an encoded payload.
///
/// Two implementations exist: [`Vec<u8>`] writes the bytes for real, while
/// [`Counter`] only measures them. Both share the per-field validation, so a
/// payload can be sized before a single byte of it is written.
pub(crate) trait Sink {
    fn put_u8(&mut self, value: u8);
    fn put_u16(&mut self, value: u16);
    fn put_u32(&mut self, value: u32);
    fn put_u64(&mut self, value: u64);
    /// Write `bytes` without any length prefix.
    fn put_raw(&mut self, bytes: &[u8]);
    /// Write `bytes` prefixed by their `u32` length.
    fn put_bytes32(&mut self, bytes: &[u8], field: &'static str) -> Result<(), FrameError> {
        self.put_u32(len_u32(bytes.len(), field)?);
        self.put_raw(bytes);
        Ok(())
    }
    /// Write a UTF-8 string prefixed by its `u32` byte length.
    fn put_str32(&mut self, value: &str, field: &'static str) -> Result<(), FrameError> {
        self.put_bytes32(value.as_bytes(), field)
    }
}

impl Sink for Vec<u8> {
    fn put_u8(&mut self, value: u8) {
        self.push(value);
    }

    fn put_u16(&mut self, value: u16) {
        self.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32(&mut self, value: u32) {
        self.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u64(&mut self, value: u64) {
        self.extend_from_slice(&value.to_le_bytes());
    }

    fn put_raw(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

/// A [`Sink`] that discards the values and keeps only the total length.
pub(crate) struct Counter(pub usize);

impl Sink for Counter {
    fn put_u8(&mut self, _value: u8) {
        self.0 += 1;
    }

    fn put_u16(&mut self, _value: u16) {
        self.0 += 2;
    }

    fn put_u32(&mut self, _value: u32) {
        self.0 += 4;
    }

    fn put_u64(&mut self, _value: u64) {
        self.0 += 8;
    }

    fn put_raw(&mut self, bytes: &[u8]) {
        self.0 += bytes.len();
    }
}

/// Cursor over a complete frame payload.
///
/// Every accessor returns an error instead of panicking when the payload ends
/// early, and [`Reader::finish`] reports bytes left over after the last field.
pub(crate) struct Reader<'a> {
    ty: &'static str,
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Start reading the payload of a frame of kind `ty`.
    pub(crate) fn new(ty: &'static str, buf: &'a [u8]) -> Self {
        Self { ty, buf, pos: 0 }
    }

    /// Number of payload bytes not consumed yet.
    pub(crate) fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    fn take(&mut self, n: usize, field: &'static str) -> Result<&'a [u8], FrameError> {
        if self.remaining() < n {
            return Err(FrameError::Truncated { field });
        }
        let start = self.pos;
        self.pos += n;
        Ok(&self.buf[start..self.pos])
    }

    pub(crate) fn u8(&mut self, field: &'static str) -> Result<u8, FrameError> {
        Ok(self.take(1, field)?[0])
    }

    pub(crate) fn u16(&mut self, field: &'static str) -> Result<u16, FrameError> {
        let bytes = self.take(2, field)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub(crate) fn u32(&mut self, field: &'static str) -> Result<u32, FrameError> {
        let bytes = self.take(4, field)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(crate) fn u64(&mut self, field: &'static str) -> Result<u64, FrameError> {
        let bytes = self.take(8, field)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub(crate) fn i32(&mut self, field: &'static str) -> Result<i32, FrameError> {
        let bytes = self.take(4, field)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read a `u32`-length-prefixed byte string.
    pub(crate) fn bytes32(&mut self, field: &'static str) -> Result<&'a [u8], FrameError> {
        let len = self.u32(field)? as usize;
        self.take(len, field)
    }

    /// Read a `u32`-length-prefixed UTF-8 string.
    pub(crate) fn string32(&mut self, field: &'static str) -> Result<&'a str, FrameError> {
        let bytes = self.bytes32(field)?;
        core::str::from_utf8(bytes).map_err(|_| FrameError::NotUtf8 { field })
    }

    /// Consume and return every payload byte that is left.
    ///
    /// Used by the body frames, whose whole payload is one opaque field.
    pub(crate) fn take_rest(&mut self) -> &'a [u8] {
        let bytes = &self.buf[self.pos..];
        self.pos = self.buf.len();
        bytes
    }

    /// Fail when the payload has bytes left over.
    pub(crate) fn finish(&self) -> Result<(), FrameError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(FrameError::TrailingBytes {
                ty: self.ty,
                extra: self.remaining(),
            })
        }
    }
}
