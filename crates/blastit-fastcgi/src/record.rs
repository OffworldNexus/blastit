//! FastCGI version-1 record framing (design §4.5.2, research §A–§G).
//!
//! A record is an 8-byte header — `version`, `type`, big-endian `requestId`,
//! big-endian `contentLength`, `paddingLength`, `reserved` — followed by the
//! content and by zero padding to an 8-byte boundary. Every decoder here is
//! total: bytes that are not one complete version-1 record are
//! [`RecordError::Truncated`], never a panic and never an over-read.

use core::fmt;

use crate::{FCGI_MAX_CONTENT, FCGI_VERSION_1};

/// Length of the fixed record header, in bytes.
pub const HEADER_LEN: usize = 8;

/// `FCGI_BEGIN_REQUEST`.
pub const FCGI_BEGIN_REQUEST: u8 = 1;
/// `FCGI_ABORT_REQUEST`.
pub const FCGI_ABORT_REQUEST: u8 = 2;
/// `FCGI_END_REQUEST`.
pub const FCGI_END_REQUEST: u8 = 3;
/// `FCGI_PARAMS`.
pub const FCGI_PARAMS: u8 = 4;
/// `FCGI_STDIN`.
pub const FCGI_STDIN: u8 = 5;
/// `FCGI_STDOUT`.
pub const FCGI_STDOUT: u8 = 6;
/// `FCGI_STDERR`.
pub const FCGI_STDERR: u8 = 7;

/// `FCGI_RESPONDER`, the only role this client ever sends.
pub const FCGI_RESPONDER: u16 = 1;
/// `FCGI_KEEP_CONN`: clear on every request, because one connection carries one
/// request (design §4.5.2).
pub const FCGI_KEEP_CONN: u8 = 1;
/// `FCGI_REQUEST_COMPLETE`, the only `protocolStatus` a healthy pool returns.
pub const FCGI_REQUEST_COMPLETE: u8 = 0;

/// The record type byte, named for the seven types a responder client uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    /// `FCGI_BEGIN_REQUEST`.
    BeginRequest,
    /// `FCGI_ABORT_REQUEST`.
    AbortRequest,
    /// `FCGI_END_REQUEST`.
    EndRequest,
    /// `FCGI_PARAMS`.
    Params,
    /// `FCGI_STDIN`.
    Stdin,
    /// `FCGI_STDOUT`.
    Stdout,
    /// `FCGI_STDERR`.
    Stderr,
    /// A type byte this crate does not name (`GET_VALUES`, `GET_VALUES_RESULT`,
    /// `DATA`, `UNKNOWN_TYPE`). A responder never expects one; the adapter
    /// faults on it, and the codec keeps the byte so it can still be framed.
    Unnamed(u8),
}

impl RecordType {
    /// The type byte for this kind.
    pub const fn code(self) -> u8 {
        match self {
            Self::BeginRequest => FCGI_BEGIN_REQUEST,
            Self::AbortRequest => FCGI_ABORT_REQUEST,
            Self::EndRequest => FCGI_END_REQUEST,
            Self::Params => FCGI_PARAMS,
            Self::Stdin => FCGI_STDIN,
            Self::Stdout => FCGI_STDOUT,
            Self::Stderr => FCGI_STDERR,
            Self::Unnamed(code) => code,
        }
    }

    /// The kind for a type byte, [`RecordType::Unnamed`] outside the seven.
    pub const fn from_code(code: u8) -> Self {
        match code {
            FCGI_BEGIN_REQUEST => Self::BeginRequest,
            FCGI_ABORT_REQUEST => Self::AbortRequest,
            FCGI_END_REQUEST => Self::EndRequest,
            FCGI_PARAMS => Self::Params,
            FCGI_STDIN => Self::Stdin,
            FCGI_STDOUT => Self::Stdout,
            FCGI_STDERR => Self::Stderr,
            other => Self::Unnamed(other),
        }
    }
}

/// A decoded record header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// The record type.
    pub kind: RecordType,
    /// The request this record belongs to; every request of this client uses
    /// `1` (design §4.5.2).
    pub request_id: u16,
    /// Content bytes that follow the header.
    pub content_length: u16,
    /// Padding bytes after the content; the decoder skips them.
    pub padding_length: u8,
}

impl Header {
    /// Build a header for `content_length` content bytes, padded so the whole
    /// record is a multiple of eight bytes where the cap allows it.
    ///
    /// php-fpm rejects a record whose `contentLength + paddingLength` is above
    /// [`FCGI_MAX_CONTENT`] (research §F), so a content length in
    /// `65 529..=65 535` — where 8-byte alignment would overflow the cap —
    /// carries **no padding** instead: a maximum-size record legitimately ends
    /// unaligned (research §A, §F). Only a content length above the cap is
    /// [`RecordError::Truncated`].
    pub fn new(
        kind: RecordType,
        request_id: u16,
        content_length: usize,
    ) -> Result<Self, RecordError> {
        if content_length > FCGI_MAX_CONTENT {
            return Err(RecordError::Truncated);
        }
        let content_length = u16::try_from(content_length).map_err(|_| RecordError::Truncated)?;
        let aligned = padding_for(content_length);
        let padding_length =
            if usize::from(content_length) + usize::from(aligned) > FCGI_MAX_CONTENT {
                0
            } else {
                aligned
            };
        Ok(Self {
            kind,
            request_id,
            content_length,
            padding_length,
        })
    }

    /// Decode the 8-byte header at the front of `bytes`.
    ///
    /// A version other than [`FCGI_VERSION_1`], a length above
    /// [`FCGI_MAX_CONTENT`] and a short header are all
    /// [`RecordError::Truncated`]: the bytes are not a version-1 record, so the
    /// caller must not read further.
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        let header: [u8; HEADER_LEN] = bytes
            .get(..HEADER_LEN)
            .and_then(|head| head.try_into().ok())
            .ok_or(RecordError::Truncated)?;
        let [
            version,
            kind,
            id_hi,
            id_lo,
            cl_hi,
            cl_lo,
            padding_length,
            _reserved,
        ] = header;
        if version != FCGI_VERSION_1 {
            return Err(RecordError::Truncated);
        }
        let content_length = u16::from_be_bytes([cl_hi, cl_lo]);
        if usize::from(content_length) + usize::from(padding_length) > FCGI_MAX_CONTENT {
            return Err(RecordError::Truncated);
        }
        Ok(Self {
            kind: RecordType::from_code(kind),
            request_id: u16::from_be_bytes([id_hi, id_lo]),
            content_length,
            padding_length,
        })
    }

    /// The header bytes, with `reserved` written as zero.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let [id_hi, id_lo] = self.request_id.to_be_bytes();
        let [cl_hi, cl_lo] = self.content_length.to_be_bytes();
        [
            FCGI_VERSION_1,
            self.kind.code(),
            id_hi,
            id_lo,
            cl_hi,
            cl_lo,
            self.padding_length,
            0,
        ]
    }

    /// Header, content and padding, in bytes.
    pub fn total_len(&self) -> usize {
        HEADER_LEN + usize::from(self.content_length) + usize::from(self.padding_length)
    }
}

/// One record borrowed from the bytes it was decoded from.
pub struct Record<'a> {
    /// The decoded header.
    pub header: Header,
    /// The content, padding excluded.
    pub content: &'a [u8],
}

impl<'a> Record<'a> {
    /// Decode the record at the front of `bytes`, returning it and how many
    /// bytes it used (header, content and padding).
    pub fn decode(bytes: &'a [u8]) -> Result<(Self, usize), RecordError> {
        let header = Header::decode(bytes)?;
        let total = header.total_len();
        let frame = bytes.get(..total).ok_or(RecordError::Truncated)?;
        let end = HEADER_LEN + usize::from(header.content_length);
        let content = frame.get(HEADER_LEN..end).ok_or(RecordError::Truncated)?;
        Ok((Self { header, content }, total))
    }
}

impl fmt::Debug for Record<'_> {
    /// The log-safe view: the kind, the id and the length, never the bytes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Record")
            .field("kind", &self.header.kind)
            .field("request_id", &self.header.request_id)
            .field("content_length", &self.content.len())
            .finish_non_exhaustive()
    }
}

/// Append one complete record — header, content, zero padding — to `out`.
pub fn encode(
    kind: RecordType,
    request_id: u16,
    content: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), RecordError> {
    let header = Header::new(kind, request_id, content.len())?;
    out.extend_from_slice(&header.encode());
    out.extend_from_slice(content);
    out.resize(out.len() + usize::from(header.padding_length), 0);
    Ok(())
}

/// The `BEGIN_REQUEST` body: `role`, `flags`, `reserved[5]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeginRequest {
    /// `FCGI_RESPONDER` (1) for this client.
    pub role: u16,
    /// `FCGI_KEEP_CONN` when set; always clear here.
    pub flags: u8,
}

impl BeginRequest {
    /// Body length of every `BEGIN_REQUEST`, in bytes.
    pub const LEN: usize = 8;

    /// The one body this client sends: `role = RESPONDER`, `flags = 0`, so the
    /// pool closes the connection after `END_REQUEST` (design §4.5.2).
    pub const fn responder() -> Self {
        Self {
            role: FCGI_RESPONDER,
            flags: 0,
        }
    }

    /// The 8 body bytes, `reserved[5]` zeroed.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let [role_hi, role_lo] = self.role.to_be_bytes();
        [role_hi, role_lo, self.flags, 0, 0, 0, 0, 0]
    }

    /// Decode an 8-byte body; any other length is [`RecordError::Truncated`].
    pub fn decode(content: &[u8]) -> Result<Self, RecordError> {
        let body: [u8; Self::LEN] = content.try_into().map_err(|_| RecordError::Truncated)?;
        let [role_hi, role_lo, flags, _, _, _, _, _] = body;
        Ok(Self {
            role: u16::from_be_bytes([role_hi, role_lo]),
            flags,
        })
    }
}

/// The `END_REQUEST` body: `appStatus`, `protocolStatus`, `reserved[3]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndRequest {
    /// The application status. php-fpm always sends `0`; it is not an exit code
    /// and the caller ignores it (research §G).
    pub app_status: u32,
    /// Must be [`FCGI_REQUEST_COMPLETE`].
    pub protocol_status: u8,
}

impl EndRequest {
    /// Body length of every `END_REQUEST`, in bytes.
    pub const LEN: usize = 8;

    /// Decode an 8-byte body. `reserved[3]` is uninitialised residue in
    /// php-fpm and is ignored; a `protocolStatus` other than
    /// [`FCGI_REQUEST_COMPLETE`] is [`RecordError::ProtocolStatus`].
    pub fn decode(content: &[u8]) -> Result<Self, RecordError> {
        let body: [u8; Self::LEN] = content.try_into().map_err(|_| RecordError::Truncated)?;
        let [a3, a2, a1, a0, protocol_status, _, _, _] = body;
        if protocol_status != FCGI_REQUEST_COMPLETE {
            return Err(RecordError::ProtocolStatus);
        }
        Ok(Self {
            app_status: u32::from_be_bytes([a3, a2, a1, a0]),
            protocol_status,
        })
    }
}

/// Everything a record decoder can refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    /// The bytes are not one complete version-1 record: a short header, a body
    /// shorter than the header promises, a `version` other than
    /// [`FCGI_VERSION_1`], or a length above [`FCGI_MAX_CONTENT`].
    Truncated,
    /// `END_REQUEST.protocolStatus` is not [`FCGI_REQUEST_COMPLETE`].
    ProtocolStatus,
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("fastcgi: record is truncated or over-long"),
            Self::ProtocolStatus => {
                f.write_str("fastcgi: END_REQUEST protocol status is not complete")
            }
        }
    }
}

impl std::error::Error for RecordError {}

/// Padding that puts a record on an 8-byte boundary.
const fn padding_for(content_length: u16) -> u8 {
    ((8 - (content_length as usize % 8)) % 8) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: RecordType, content: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        encode(kind, 1, content, &mut out).expect("encodes");
        out
    }

    #[test]
    fn header_round_trips_and_pads_to_eight() {
        for len in [0usize, 1, 7, 8, 9, 8_184, 65_527, 65_528] {
            let header = Header::new(RecordType::Stdout, 1, len).expect("fits");
            assert_eq!(Header::decode(&header.encode()).expect("decodes"), header);
            assert_eq!(usize::from(header.content_length), len);
            assert_eq!(header.total_len() % 8, 0, "len {len}");
            assert_eq!(
                record(RecordType::Stdout, &vec![b'x'; len]).len(),
                header.total_len()
            );
        }
    }

    #[test]
    fn over_long_content_is_refused() {
        // The cap is 65 535 content bytes. Both boundaries, in both directions:
        // `Header::new` (encode) and `Header::decode` (wire).
        for kind in [
            RecordType::BeginRequest,
            RecordType::AbortRequest,
            RecordType::EndRequest,
            RecordType::Params,
            RecordType::Stdin,
            RecordType::Stdout,
            RecordType::Stderr,
        ] {
            assert!(Header::new(kind, 1, 65_535).is_ok(), "{kind:?}");
            assert_eq!(
                Header::new(kind, 1, 65_536),
                Err(RecordError::Truncated),
                "{kind:?}"
            );
            let wire = Header::new(kind, 1, 65_535).expect("fits").encode();
            assert_eq!(
                Header::decode(&wire).map(|header| header.content_length),
                Ok(65_535),
                "{kind:?}"
            );
        }
        let mut out = Vec::new();
        assert_eq!(
            encode(RecordType::Params, 1, &[0; 65_536], &mut out),
            Err(RecordError::Truncated)
        );
        assert!(out.is_empty());
        // A length read off the wire that overflows the maximum is refused too:
        // `CL = 65 535` with one padding byte is 65 536.
        let mut bytes = Header::new(RecordType::Stdout, 1, 0)
            .expect("fits")
            .encode();
        bytes[4] = 0xff;
        bytes[5] = 0xff;
        bytes[6] = 1;
        assert_eq!(Header::decode(&bytes), Err(RecordError::Truncated));
    }

    #[test]
    fn the_maximum_record_carries_no_padding() {
        // 65 529..=65 535 would need 1..=7 padding bytes to align, which would
        // push `CL + PL` past 65 535. The record is legal and simply unaligned
        // (research §F: "a maximum-size record must have zero padding").
        for len in 65_529usize..=65_535 {
            let header = Header::new(RecordType::Params, 1, len).expect("fits");
            assert_eq!(usize::from(header.content_length), len);
            assert_eq!(header.padding_length, 0, "len {len}");
            assert_eq!(header.total_len(), HEADER_LEN + len, "len {len}");
            assert_eq!(Header::decode(&header.encode()).expect("decodes"), header);
            // The full cap-sized record decodes in one pass, content intact.
            let content = vec![b'x'; len];
            let wire = record(RecordType::Params, &content);
            assert_eq!(wire.len(), HEADER_LEN + len, "len {len}");
            let (decoded, used) = Record::decode(&wire).expect("decodes");
            assert_eq!(used, wire.len(), "len {len}");
            assert_eq!(usize::from(decoded.header.content_length), len, "len {len}");
            assert_eq!(decoded.content.len(), len, "len {len}");
            assert_eq!(decoded.content, content.as_slice(), "len {len}");
        }
        // The last aligned record below the cap still pads to eight bytes.
        let aligned = Header::new(RecordType::Params, 1, 65_528).expect("fits");
        assert_eq!(aligned.padding_length, 0);
        assert_eq!(aligned.total_len() % 8, 0);
        let padded = Header::new(RecordType::Params, 1, 65_527).expect("fits");
        assert_eq!(padded.padding_length, 1);
        assert_eq!(padded.total_len() % 8, 0);
    }

    #[test]
    fn the_decoder_is_total_on_every_prefix() {
        let wire = record(RecordType::Stdout, b"hello");
        assert_eq!(wire.len(), 16);
        for cut in 0..wire.len() {
            assert_eq!(
                Record::decode(&wire[..cut]).map(|_| ()),
                Err(RecordError::Truncated),
                "cut {cut}"
            );
        }
        let (decoded, used) = Record::decode(&wire).expect("decodes");
        assert_eq!(used, wire.len());
        assert_eq!(decoded.header.kind, RecordType::Stdout);
        assert_eq!(decoded.content, b"hello");
        // Trailing bytes are the next record's problem, not this one's.
        let mut two = wire.clone();
        two.extend_from_slice(&wire);
        assert_eq!(Record::decode(&two).expect("decodes").1, wire.len());
    }

    #[test]
    fn a_non_v1_header_is_refused() {
        let mut bytes = record(RecordType::Params, b"");
        bytes[0] = 2;
        assert_eq!(Header::decode(&bytes), Err(RecordError::Truncated));
        bytes[0] = 0;
        assert_eq!(Header::decode(&bytes), Err(RecordError::Truncated));
    }

    #[test]
    fn padding_is_written_as_zero_and_skipped() {
        let wire = record(RecordType::Params, b"abc");
        assert_eq!(wire.len(), 16);
        assert_eq!(&wire[11..], &[0, 0, 0, 0, 0]);
        assert_eq!(Record::decode(&wire).expect("decodes").0.content, b"abc");
    }

    #[test]
    fn record_type_codes_round_trip() {
        for (code, kind) in [
            (1, RecordType::BeginRequest),
            (2, RecordType::AbortRequest),
            (3, RecordType::EndRequest),
            (4, RecordType::Params),
            (5, RecordType::Stdin),
            (6, RecordType::Stdout),
            (7, RecordType::Stderr),
        ] {
            assert_eq!(RecordType::from_code(code), kind);
            assert_eq!(kind.code(), code);
        }
        assert_eq!(RecordType::from_code(9), RecordType::Unnamed(9));
        assert_eq!(RecordType::Unnamed(9).code(), 9);
        assert_eq!(RecordType::from_code(0), RecordType::Unnamed(0));
    }

    #[test]
    fn an_unnamed_record_type_is_framed_for_the_adapter() {
        let mut wire = record(RecordType::Params, b"");
        wire[1] = 9;
        let (decoded, used) = Record::decode(&wire).expect("decodes");
        assert_eq!(decoded.header.kind, RecordType::Unnamed(9));
        assert_eq!(decoded.content, b"");
        assert_eq!(used, wire.len());
        // The codec keeps the byte and frames the record; refusing an unnamed
        // type on a responder stream is the adapter's obligation
        // (`MB.3.6.2.1`), not a codec error.
    }

    #[test]
    fn begin_request_is_the_pinned_responder_body() {
        let body = BeginRequest::responder().encode();
        assert_eq!(body, [0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(BeginRequest::decode(&body), Ok(BeginRequest::responder()));
        // keepConn stays clear: one request per connection.
        assert_eq!(BeginRequest::responder().flags & FCGI_KEEP_CONN, 0);
        for wrong in [&body[..7], &[0, 1, 0, 0, 0, 0, 0, 0, 0][..]] {
            assert_eq!(BeginRequest::decode(wrong), Err(RecordError::Truncated));
        }
    }

    #[test]
    fn end_request_ignores_app_status_and_reserved_bytes() {
        let content = [0, 0, 0, 7, 0, b'A', b'A', b'A'];
        let end = EndRequest::decode(&content).expect("complete");
        assert_eq!(end.app_status, 7);
        assert_eq!(end.protocol_status, FCGI_REQUEST_COMPLETE);
    }

    #[test]
    fn end_request_refuses_an_incomplete_protocol_status() {
        for status in [1u8, 2, 3, 0xff] {
            let content = [0, 0, 0, 0, status, 0, 0, 0];
            assert_eq!(
                EndRequest::decode(&content),
                Err(RecordError::ProtocolStatus)
            );
        }
        assert_eq!(
            EndRequest::decode(&[0, 0, 0, 0, 0]),
            Err(RecordError::Truncated)
        );
        assert_eq!(
            EndRequest::decode(&[0, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(RecordError::Truncated)
        );
    }

    #[test]
    fn an_abort_request_carries_no_body() {
        let wire = record(RecordType::AbortRequest, b"");
        assert_eq!(wire, [1, 2, 0, 1, 0, 0, 0, 0]);
        let (decoded, used) = Record::decode(&wire).expect("decodes");
        assert_eq!(decoded.header.kind, RecordType::AbortRequest);
        assert_eq!(decoded.content, b"");
        assert_eq!(used, 8);
    }

    #[test]
    fn the_debug_view_never_prints_content() {
        let wire = record(RecordType::Stdout, b"secret");
        let text = format!("{:?}", Record::decode(&wire).expect("decodes").0);
        assert!(text.contains("content_length: 6"), "{text}");
        assert!(!text.contains("secret"), "{text}");
    }

    #[test]
    fn errors_print_without_bytes() {
        assert_eq!(
            RecordError::Truncated.to_string(),
            "fastcgi: record is truncated or over-long"
        );
        assert_eq!(
            RecordError::ProtocolStatus.to_string(),
            "fastcgi: END_REQUEST protocol status is not complete"
        );
    }
}
