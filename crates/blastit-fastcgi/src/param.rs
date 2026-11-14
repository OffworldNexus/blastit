//! The `PARAMS` name-value codec (design §4.5.2, research §D).
//!
//! A pair is `nameLen`, `valueLen`, name, value, with no per-pair padding: a
//! length below 128 is one byte, a longer one is four bytes with the high bit
//! set. A stream ends with the empty-name terminator (the pair `00 00`), and a
//! pair never straddles two records, so a truncated or straddling pair is
//! [`RecordError::Truncated`] — there is no separate param error.

use core::fmt;

use crate::record::RecordError;

/// Largest length the four-byte form carries: 31 bits.
const MAX_LEN: usize = 0x7fff_ffff;

/// One decoded name-value pair, borrowed from the record content.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Pair<'a> {
    /// The name bytes, never empty for a real pair.
    pub name: &'a [u8],
    /// The value bytes, possibly empty.
    pub value: &'a [u8],
}

impl fmt::Debug for Pair<'_> {
    /// The log-safe view: the name and the value's length, never the value,
    /// because a param value routinely carries a cookie or a token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pair")
            .field("name", &String::from_utf8_lossy(self.name))
            .field("value_len", &self.value.len())
            .finish()
    }
}

/// Append one encoded pair to `out`.
///
/// An empty name is refused: it is the stream terminator, never a param.
pub fn encode_pair(name: &[u8], value: &[u8], out: &mut Vec<u8>) -> Result<(), RecordError> {
    if name.is_empty() {
        return Err(RecordError::Truncated);
    }
    encode_len(name.len(), out)?;
    encode_len(value.len(), out)?;
    out.extend_from_slice(name);
    out.extend_from_slice(value);
    Ok(())
}

/// Append one encoded length: one byte below 128, otherwise four with the high
/// bit set.
fn encode_len(len: usize, out: &mut Vec<u8>) -> Result<(), RecordError> {
    if len < 0x80 {
        out.push(u8::try_from(len).map_err(|_| RecordError::Truncated)?);
    } else {
        let len = u32::try_from(len)
            .ok()
            .filter(|len| *len as usize <= MAX_LEN)
            .ok_or(RecordError::Truncated)?;
        out.extend_from_slice(&(len | 0x8000_0000).to_be_bytes());
    }
    Ok(())
}

/// A stream decoder: one record's content in, pairs out.
#[derive(Debug, Clone)]
pub struct Decoder<'a> {
    rest: &'a [u8],
    done: bool,
}

impl<'a> Decoder<'a> {
    /// Decode the `PARAMS` content of one record.
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self {
            rest: bytes,
            done: false,
        }
    }

    /// The next pair, or `Ok(None)` at the empty-name terminator and at the end
    /// of the content.
    ///
    /// Bytes after the terminator, and a terminator that carries a value, are
    /// [`RecordError::Truncated`]: the stream already ended.
    pub fn next_pair(&mut self) -> Result<Option<Pair<'a>>, RecordError> {
        if self.done || self.rest.is_empty() {
            self.done = true;
            return Ok(None);
        }
        let (name_len, rest) = read_len(self.rest)?;
        let (value_len, rest) = read_len(rest)?;
        if name_len == 0 {
            if value_len != 0 || !rest.is_empty() {
                return Err(RecordError::Truncated);
            }
            self.done = true;
            return Ok(None);
        }
        let name = rest.get(..name_len).ok_or(RecordError::Truncated)?;
        let rest = rest.get(name_len..).ok_or(RecordError::Truncated)?;
        let value = rest.get(..value_len).ok_or(RecordError::Truncated)?;
        self.rest = rest.get(value_len..).ok_or(RecordError::Truncated)?;
        Ok(Some(Pair { name, value }))
    }
}

/// Decode every pair of one record's `PARAMS` content.
pub fn decode_all(bytes: &[u8]) -> Result<Vec<Pair<'_>>, RecordError> {
    let mut decoder = Decoder::new(bytes);
    let mut pairs = Vec::new();
    while let Some(pair) = decoder.next_pair()? {
        pairs.push(pair);
    }
    Ok(pairs)
}

/// Read one encoded length: one byte below 128, otherwise four with the high
/// bit set.
fn read_len(bytes: &[u8]) -> Result<(usize, &[u8]), RecordError> {
    let first = *bytes.first().ok_or(RecordError::Truncated)?;
    if first & 0x80 == 0 {
        let rest = bytes.get(1..).ok_or(RecordError::Truncated)?;
        return Ok((usize::from(first), rest));
    }
    let four: [u8; 4] = bytes
        .get(..4)
        .and_then(|head| head.try_into().ok())
        .ok_or(RecordError::Truncated)?;
    let len = u32::from_be_bytes(four) & 0x7fff_ffff;
    let rest = bytes.get(4..).ok_or(RecordError::Truncated)?;
    Ok((
        usize::try_from(len).map_err(|_| RecordError::Truncated)?,
        rest,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(name: &[u8], value: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        encode_pair(name, value, &mut out).expect("encodes");
        out
    }

    #[test]
    fn short_lengths_are_one_byte_and_long_ones_four() {
        assert_eq!(pair(b"NAME", b"v"), [4, 1, b'N', b'A', b'M', b'E', b'v']);
        assert_eq!(
            pair(&[b'n'; 127], b""),
            [&[127u8][..], &[0u8], &[b'n'; 127]].concat()
        );
        assert_eq!(
            pair(&[b'n'; 128], b""),
            [&[0x80u8, 0, 0, 128][..], &[0u8], &[b'n'; 128]].concat()
        );
        assert_eq!(
            pair(b"n", &[b'v'; 128]),
            [&[1u8, 0x80, 0, 0, 128][..], b"n", &[b'v'; 128]].concat()
        );
    }

    #[test]
    fn pairs_round_trip_through_the_decoder() {
        let cases: [(&[u8], &[u8]); 4] = [
            (b"SCRIPT_FILENAME", b"/app/public/index.php"),
            (b"QUERY_STRING", b""),
            (&[b'n'; 127], &[b'v'; 127]),
            (&[b'n'; 128], &[b'v'; 128]),
        ];
        let mut wire = Vec::new();
        for (name, value) in cases {
            encode_pair(name, value, &mut wire).expect("encodes");
        }
        let decoded = decode_all(&wire).expect("decodes");
        assert_eq!(decoded.len(), cases.len());
        for (got, (name, value)) in decoded.iter().zip(cases) {
            assert_eq!((got.name, got.value), (name, value));
        }
    }

    #[test]
    fn the_empty_name_terminator_ends_the_stream() {
        let mut wire = pair(b"A", b"1");
        wire.extend_from_slice(&[0, 0]);
        let decoded = decode_all(&wire).expect("decodes");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].name, b"A");
        assert!(decode_all(&[0, 0]).expect("decodes").is_empty());
        assert!(decode_all(&[]).expect("decodes").is_empty());
        // A terminator with a value is not a terminator.
        assert_eq!(decode_all(&[0, 1, b'x']), Err(RecordError::Truncated));
        // Bytes after the terminator mean the stream already ended.
        let mut trailing = vec![0, 0];
        trailing.extend_from_slice(&pair(b"A", b"1"));
        assert_eq!(decode_all(&trailing), Err(RecordError::Truncated));
    }

    #[test]
    fn a_truncated_pair_is_an_error() {
        let wire = pair(b"NAME", b"value");
        for cut in 1..wire.len() {
            assert_eq!(
                decode_all(&wire[..cut]).map(|_| ()),
                Err(RecordError::Truncated),
                "cut {cut}"
            );
        }
        assert_eq!(decode_all(&wire).expect("decodes").len(), 1);
        // A four-byte length whose tail is short.
        assert_eq!(decode_all(&[0x80, 0, 0]), Err(RecordError::Truncated));
        assert_eq!(decode_all(&[0x80, 0, 0, 1]), Err(RecordError::Truncated));
        assert_eq!(decode_all(&[0x80, 0, 0, 0, 1]), Err(RecordError::Truncated));
    }

    #[test]
    fn an_empty_name_is_not_a_param() {
        let mut out = Vec::new();
        assert_eq!(
            encode_pair(b"", b"x", &mut out),
            Err(RecordError::Truncated)
        );
        assert!(out.is_empty());
    }

    #[test]
    fn a_length_beyond_the_four_byte_form_is_refused() {
        let mut out = Vec::new();
        assert_eq!(
            encode_len(MAX_LEN + 1, &mut out),
            Err(RecordError::Truncated)
        );
        assert!(out.is_empty());
        assert_eq!(encode_len(MAX_LEN, &mut out), Ok(()));
        assert_eq!(out, [0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn the_debug_view_never_prints_a_value() {
        let pair = Pair {
            name: b"HTTP_COOKIE",
            value: b"session=secret",
        };
        let text = format!("{pair:?}");
        assert!(text.contains("HTTP_COOKIE"), "{text}");
        assert!(text.contains("value_len: 14"), "{text}");
        assert!(!text.contains("secret"), "{text}");
    }
}
