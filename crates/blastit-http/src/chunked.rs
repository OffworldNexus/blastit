//! De-chunking a `Transfer-Encoding: chunked` request body (RFC 7230 §4.1).
//!
//! The decoder is a pure state machine fed with whatever the connection has
//! just read: it hands back the body bytes it could decode and remembers where
//! it was, so a chunk may straddle any number of reads. Chunk extensions are
//! ignored and trailers are discarded — the frame protocol carries neither.

/// Longest chunk-size line (extensions included) we accept.
const MAX_SIZE_LINE: usize = 1024;

/// Most bytes of trailers we are willing to walk through.
const MAX_TRAILER_BYTES: usize = 8 * 1024;

/// What is expected next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// A chunk-size line.
    Size,
    /// `n` more bytes of chunk data.
    Data(u64),
    /// The CRLF after chunk data.
    DataEnd,
    /// The trailer section, until an empty line.
    Trailers,
}

/// Whether more input is needed, or the body is complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The body continues.
    More,
    /// The final chunk and its trailers have been seen.
    Done,
}

/// Why a chunked body was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkError {
    /// The chunk framing is not valid HTTP.
    Malformed,
    /// The body is larger than the configured maximum.
    TooLarge,
}

/// A streaming chunked-body decoder.
#[derive(Debug)]
pub struct Dechunker {
    state: State,
    /// Bytes of the line being read, without its CRLF.
    line: Vec<u8>,
    /// A CR that has been read but does not yet belong to a CRLF.
    pending_cr: bool,
    /// Trailer bytes walked through, so a trailer section cannot be unbounded.
    trailers: usize,
    total: u64,
    max_body: u64,
}

impl Dechunker {
    /// A decoder that refuses a body larger than `max_body` bytes.
    pub fn new(max_body: u64) -> Self {
        Self {
            state: State::Size,
            line: Vec::new(),
            pending_cr: false,
            trailers: 0,
            total: 0,
            max_body,
        }
    }

    /// Decode as much of `input` as possible, appending body bytes to `out`.
    ///
    /// Returns how many bytes of `input` were consumed and whether the body is
    /// complete. Body bytes are appended as they are decoded: `out` grows by at
    /// most one call's worth of input, never by a whole chunk, let alone a
    /// whole body. Bytes that do not yet make up a whole line — including a CR
    /// that may or may not be the start of a CRLF — stay buffered in the
    /// decoder, so a line may be split anywhere.
    ///
    /// # Errors
    ///
    /// [`ChunkError::Malformed`] for anything that is not valid chunked
    /// framing, [`ChunkError::TooLarge`] once the decoded body passes the
    /// maximum.
    pub fn push(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(usize, Step), ChunkError> {
        let mut pos = 0;
        loop {
            match self.state {
                State::Size => match self.read_line(&input[pos..], MAX_SIZE_LINE)? {
                    None => return Ok((input.len(), Step::More)),
                    Some(consumed) => {
                        pos += consumed;
                        let size = parse_size(&self.line)?;
                        self.line.clear();
                        self.state = if size == 0 {
                            State::Trailers
                        } else {
                            State::Data(size)
                        };
                    }
                },
                State::Data(left) => {
                    let available = input.len() - pos;
                    if available == 0 {
                        return Ok((pos, Step::More));
                    }
                    let take = left.min(available as u64) as usize;
                    if self.total.saturating_add(take as u64) > self.max_body {
                        return Err(ChunkError::TooLarge);
                    }
                    out.extend_from_slice(&input[pos..pos + take]);
                    self.total += take as u64;
                    pos += take;
                    self.state = if left == take as u64 {
                        State::DataEnd
                    } else {
                        State::Data(left - take as u64)
                    };
                }
                State::DataEnd => {
                    if input.len() - pos < 2 {
                        return Ok((pos, Step::More));
                    }
                    if &input[pos..pos + 2] != b"\r\n" {
                        return Err(ChunkError::Malformed);
                    }
                    pos += 2;
                    self.state = State::Size;
                }
                State::Trailers => match self.read_line(&input[pos..], MAX_SIZE_LINE)? {
                    None => {
                        self.trailers = self.trailers.saturating_add(input.len() - pos);
                        if self.trailers > MAX_TRAILER_BYTES {
                            return Err(ChunkError::Malformed);
                        }
                        return Ok((input.len(), Step::More));
                    }
                    Some(consumed) => {
                        pos += consumed;
                        self.trailers = self.trailers.saturating_add(consumed);
                        let empty = self.line.is_empty();
                        self.line.clear();
                        if self.trailers > MAX_TRAILER_BYTES {
                            return Err(ChunkError::Malformed);
                        }
                        if empty {
                            return Ok((pos, Step::Done));
                        }
                    }
                },
            }
        }
    }

    /// Read one CRLF-terminated line into `self.line`.
    ///
    /// Returns how many bytes of `input` the line needs, or `None` when the
    /// line is not complete yet. `self.line` and `self.pending_cr` carry the
    /// part that has arrived across calls, so a CRLF split between two reads is
    /// seen for what it is.
    ///
    /// # Errors
    ///
    /// [`ChunkError::Malformed`] for a bare CR, a bare LF, or a line longer
    /// than `limit`.
    fn read_line(&mut self, input: &[u8], limit: usize) -> Result<Option<usize>, ChunkError> {
        for (index, byte) in input.iter().enumerate() {
            if self.pending_cr {
                if *byte != b'\n' {
                    return Err(ChunkError::Malformed);
                }
                self.pending_cr = false;
                return Ok(Some(index + 1));
            }
            match *byte {
                b'\r' => self.pending_cr = true,
                b'\n' => return Err(ChunkError::Malformed),
                _ => {
                    if self.line.len() >= limit {
                        return Err(ChunkError::Malformed);
                    }
                    self.line.push(*byte);
                }
            }
        }
        Ok(None)
    }
}

/// Parse a chunk-size line: hex digits, optionally followed by extensions.
fn parse_size(line: &[u8]) -> Result<u64, ChunkError> {
    let digits = match line.iter().position(|byte| *byte == b';') {
        Some(end) => &line[..end],
        None => line,
    };
    let digits = crate::headers::trim(digits);
    if digits.is_empty() || digits.len() > 16 || !digits.iter().all(u8::is_ascii_hexdigit) {
        return Err(ChunkError::Malformed);
    }
    let text = std::str::from_utf8(digits).map_err(|_| ChunkError::Malformed)?;
    u64::from_str_radix(text, 16).map_err(|_| ChunkError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed `input` in one go and report the decoded body.
    fn decode(input: &[u8], max: u64) -> Result<(Vec<u8>, Step, usize), ChunkError> {
        let mut dechunker = Dechunker::new(max);
        let mut out = Vec::new();
        let (used, step) = dechunker.push(input, &mut out)?;
        Ok((out, step, used))
    }

    #[test]
    fn a_simple_body_is_dechunked() {
        let (body, step, used) = decode(b"5\r\nhello\r\n0\r\n\r\n", 100).unwrap();
        assert_eq!(body, b"hello");
        assert_eq!(step, Step::Done);
        assert_eq!(used, 15);
    }

    #[test]
    fn several_chunks_and_extensions_and_trailers() {
        let input =
            b"4;name=value\r\nWiki\r\n5\r\npedia\r\n0;last\r\nX-Trailer: 1\r\nAnother: 2\r\n\r\n";
        let (body, step, used) = decode(input, 100).unwrap();
        assert_eq!(body, b"Wikipedia");
        assert_eq!(step, Step::Done);
        assert_eq!(used, input.len());
    }

    #[test]
    fn an_empty_body_is_done_immediately() {
        let (body, step, used) = decode(b"0\r\n\r\n", 100).unwrap();
        assert!(body.is_empty());
        assert_eq!(step, Step::Done);
        assert_eq!(used, 5);
    }

    #[test]
    fn hexadecimal_sizes_are_read() {
        let input = b"10\r\n0123456789abcdef\r\n0\r\n\r\n";
        let (body, step, _) = decode(input, 100).unwrap();
        assert_eq!(body, b"0123456789abcdef");
        assert_eq!(step, Step::Done);

        // Upper case, and leading zeros.
        let (body, step, _) = decode(b"000A\r\n0123456789\r\n0\r\n\r\n", 100).unwrap();
        assert_eq!(body, b"0123456789");
        assert_eq!(step, Step::Done);
    }

    #[test]
    fn the_body_may_arrive_one_byte_at_a_time() {
        let input = b"4;x\r\nWiki\r\n5\r\npedia\r\n0\r\nT: v\r\n\r\n";
        let mut dechunker = Dechunker::new(100);
        let mut body = Vec::new();
        let mut window: Vec<u8> = Vec::new();
        let mut next = 0usize;
        let mut step = Step::More;
        while step == Step::More {
            // Never hand the decoder more than two bytes at a time.
            if window.len() <= 1 && next < input.len() {
                window.push(input[next]);
                next += 1;
            }
            let mut out = Vec::new();
            let (used, next_step) = dechunker.push(&window, &mut out).unwrap();
            body.extend_from_slice(&out);
            window.drain(..used);
            step = next_step;
            assert!(
                used > 0 || next < input.len(),
                "the decoder stalled with {} bytes buffered",
                window.len()
            );
        }
        assert_eq!(body, b"Wikipedia");
        assert_eq!(next, input.len());
    }

    #[test]
    fn the_body_is_never_buffered_whole() {
        // A one-mebibyte chunk arriving in 4 KiB pieces: the output never grows
        // beyond the input of one call.
        let payload = vec![b'x'; 1024 * 1024];
        let mut dechunker = Dechunker::new(u64::MAX);
        let mut header = b"100000\r\n".to_vec();
        header.extend_from_slice(&payload);
        header.extend_from_slice(b"\r\n0\r\n\r\n");
        let mut offset = 0;
        let mut decoded = 0;
        while offset < header.len() {
            let end = (offset + 4096).min(header.len());
            let mut out = Vec::new();
            let (used, step) = dechunker.push(&header[offset..end], &mut out).unwrap();
            assert!(out.len() <= end - offset);
            decoded += out.len();
            offset += used;
            if step == Step::Done {
                break;
            }
        }
        assert_eq!(decoded, payload.len());
    }

    #[test]
    fn malformed_framing_is_refused() {
        let cases: [&[u8]; 8] = [
            b"zz\r\n",
            b"4\rWiki\r\n0\r\n\r\n",
            b"4\r\nWiki\rX\r\n0\r\n\r\n",
            b"-1\r\n\r\n",
            b"11111111111111111\r\n\r\n",
            b"\r\n",
            b"4;x\r\nWiki\r\nX\r\n",
            b"4\r\nWiki\r\nz\r\n",
        ];
        for input in cases {
            let result = Dechunker::new(1000).push(input, &mut Vec::new());
            assert_eq!(result.err(), Some(ChunkError::Malformed), "{input:?}");
        }
    }

    #[test]
    fn an_oversized_body_is_refused_while_streaming() {
        let mut dechunker = Dechunker::new(5);
        let mut out = Vec::new();
        let err = dechunker
            .push(b"6\r\nhello!\r\n0\r\n\r\n", &mut out)
            .unwrap_err();
        assert_eq!(err, ChunkError::TooLarge);
        // Exactly at the limit is fine.
        let mut dechunker = Dechunker::new(6);
        let mut out = Vec::new();
        assert_eq!(
            dechunker
                .push(b"6\r\nhello!\r\n0\r\n\r\n", &mut out)
                .unwrap()
                .1,
            Step::Done
        );
        assert_eq!(out, b"hello!");
    }

    #[test]
    fn a_giant_size_line_does_not_allocate_without_bound() {
        let mut input = vec![b'a'; MAX_SIZE_LINE * 4];
        input.push(b'\r');
        input.push(b'\n');
        assert_eq!(
            Dechunker::new(u64::MAX).push(&input, &mut Vec::new()).err(),
            Some(ChunkError::Malformed)
        );
    }

    #[test]
    fn endless_trailers_are_refused() {
        let mut input = b"0\r\n".to_vec();
        for _ in 0..2000 {
            input.extend_from_slice(b"X: v\r\n");
        }
        assert_eq!(
            Dechunker::new(u64::MAX).push(&input, &mut Vec::new()).err(),
            Some(ChunkError::Malformed)
        );
    }

    #[test]
    fn total_of_the_decoded_body_is_its_length() {
        let mut dechunker = Dechunker::new(100);
        let mut out = Vec::new();
        let (used, step) = dechunker
            .push(b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n", &mut out)
            .unwrap();
        assert_eq!(step, Step::Done);
        assert_eq!(used, 20);
        assert_eq!(out, b"abcde");
    }
}
