//! Strict RFC 8259 JSON: parse into a canonical tree, then re-emit it.
//!
//! `serde_json`'s own parser is deliberately not used for decoding. It accepts
//! a few things this gate must reject (it keeps the last duplicate key, and it
//! turns an integer lexeme that overflows `u64` into an `f64`, silently
//! rounding `u64::MAX + 1`), and it does not tell a caller which of those
//! happened. A hand-written parser over the already UTF-8-validated bytes
//! gives one typed [`IngressReject`] per failure and an exact canonical
//! re-encoding: object keys sorted, no insignificant whitespace, integers
//! emitted as their exact decimal value, `f64`s through `serde_json`'s own
//! number formatting.
//!
//! The parser is recursive, bounded by [`MAX_JSON_DEPTH`], and never indexes
//! without a bounds check.

use std::collections::BTreeMap;

use crate::{IngressReject, MAX_JSON_DEPTH};

/// A JSON value, kept only long enough to be re-emitted canonically.
pub(crate) enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// An integer lexeme that fits `i64`.
    Int(i64),
    /// A non-negative integer lexeme that fits `u64` but not `i64`.
    Uint(u64),
    /// A finite `f64`, already rendered the way `serde_json` renders it.
    Float(String),
    /// A string, decoded from its escapes.
    Str(String),
    /// An array, in source order.
    Array(Vec<Value>),
    /// An object, keyed by the decoded name (so keys come out sorted).
    Object(BTreeMap<String, Value>),
}

/// Parses one complete JSON document, rejecting a BOM and trailing bytes.
pub(crate) fn parse(input: &[u8]) -> Result<Value, IngressReject> {
    if input.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(IngressReject::JsonBom);
    }
    if std::str::from_utf8(input).is_err() {
        return Err(IngressReject::JsonNotUtf8);
    }
    let mut parser = Parser { input, pos: 0 };
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.pos != input.len() {
        return Err(IngressReject::JsonTrailingBytes);
    }
    Ok(value)
}

/// Appends the canonical encoding of `value` to `out`.
pub(crate) fn emit(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Int(number) => out.extend_from_slice(number.to_string().as_bytes()),
        Value::Uint(number) => out.extend_from_slice(number.to_string().as_bytes()),
        Value::Float(number) => out.extend_from_slice(number.as_bytes()),
        Value::Str(text) => emit_string(text, out),
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                emit(item, out);
            }
            out.push(b']');
        }
        Value::Object(members) => {
            out.push(b'{');
            for (index, (name, member)) in members.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                emit_string(name, out);
                out.push(b':');
                emit(member, out);
            }
            out.push(b'}');
        }
    }
}

/// Escapes `text` the way `serde_json` does: only quotes, backslashes and
/// control characters; everything else stays raw UTF-8.
fn emit_string(text: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for character in text.chars() {
        match character {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            control if (control as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", control as u32).as_bytes());
            }
            other => {
                let mut buffer = [0_u8; 4];
                out.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    out.push(b'"');
}

/// A cursor over one UTF-8-validated document.
struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    /// The byte at the cursor, if any.
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    /// Advances past `expected`, or fails with [`IngressReject::JsonSyntax`].
    fn expect(&mut self, expected: u8) -> Result<(), IngressReject> {
        if self.peek() == Some(expected) {
            self.pos += 1;
            Ok(())
        } else {
            Err(IngressReject::JsonSyntax)
        }
    }

    /// Skips the four RFC 8259 whitespace bytes.
    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// Parses one value; `depth` is the number of containers already open.
    fn value(&mut self, depth: usize) -> Result<Value, IngressReject> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'{') => {
                if depth >= MAX_JSON_DEPTH {
                    return Err(IngressReject::JsonTooDeep);
                }
                self.object(depth + 1)
            }
            Some(b'[') => {
                if depth >= MAX_JSON_DEPTH {
                    return Err(IngressReject::JsonTooDeep);
                }
                self.array(depth + 1)
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => {
                self.literal(b"true")?;
                Ok(Value::Bool(true))
            }
            Some(b'f') => {
                self.literal(b"false")?;
                Ok(Value::Bool(false))
            }
            Some(b'n') => {
                self.literal(b"null")?;
                Ok(Value::Null)
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(IngressReject::JsonSyntax),
        }
    }

    /// Consumes an exact bare literal such as `true`.
    fn literal(&mut self, literal: &[u8]) -> Result<(), IngressReject> {
        for expected in literal {
            self.expect(*expected)?;
        }
        Ok(())
    }

    /// Parses an object, rejecting a repeated key.
    fn object(&mut self, depth: usize) -> Result<Value, IngressReject> {
        self.expect(b'{')?;
        let mut members = BTreeMap::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return Err(IngressReject::JsonSyntax);
            }
            let name = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            let member = self.value(depth)?;
            if members.insert(name, member).is_some() {
                return Err(IngressReject::JsonDuplicateKey);
            }
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(members));
                }
                _ => return Err(IngressReject::JsonSyntax),
            }
        }
    }

    /// Parses an array.
    fn array(&mut self, depth: usize) -> Result<Value, IngressReject> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value(depth)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(IngressReject::JsonSyntax),
            }
        }
    }

    /// Parses a string, decoding its escapes.
    fn string(&mut self) -> Result<String, IngressReject> {
        self.expect(b'"')?;
        let mut decoded = String::new();
        loop {
            let start = self.pos;
            while let Some(byte) = self.peek() {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            if self.pos > start {
                // The whole document is valid UTF-8 and the run stopped on an
                // ASCII byte, so both ends are `char` boundaries.
                let run = std::str::from_utf8(&self.input[start..self.pos])
                    .map_err(|_| IngressReject::JsonNotUtf8)?;
                decoded.push_str(run);
            }
            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(decoded);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    self.escape(&mut decoded)?;
                }
                _ => return Err(IngressReject::JsonSyntax),
            }
        }
    }

    /// Decodes one escape sequence after its backslash.
    fn escape(&mut self, decoded: &mut String) -> Result<(), IngressReject> {
        let escape = self.peek().ok_or(IngressReject::JsonSyntax)?;
        self.pos += 1;
        let character = match escape {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => self.unicode_escape()?,
            _ => return Err(IngressReject::JsonSyntax),
        };
        decoded.push(character);
        Ok(())
    }

    /// Decodes a `\uXXXX` escape, pairing surrogates and rejecting lone ones.
    fn unicode_escape(&mut self) -> Result<char, IngressReject> {
        let first = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&first) {
            if self.peek() != Some(b'\\') || self.input.get(self.pos + 1) != Some(&b'u') {
                return Err(IngressReject::JsonLoneSurrogate);
            }
            self.pos += 2;
            let second = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&second) {
                return Err(IngressReject::JsonLoneSurrogate);
            }
            let scalar =
                0x1_0000 + ((u32::from(first) - 0xD800) << 10) + (u32::from(second) - 0xDC00);
            char::from_u32(scalar).ok_or(IngressReject::JsonLoneSurrogate)
        } else if (0xDC00..=0xDFFF).contains(&first) {
            Err(IngressReject::JsonLoneSurrogate)
        } else {
            char::from_u32(u32::from(first)).ok_or(IngressReject::JsonLoneSurrogate)
        }
    }

    /// Reads exactly four hexadecimal digits.
    fn hex4(&mut self) -> Result<u16, IngressReject> {
        let mut value = 0_u16;
        for _ in 0..4 {
            let digit = hex_digit(self.peek())?;
            value = (value << 4) | u16::from(digit);
            self.pos += 1;
        }
        Ok(value)
    }

    /// Parses one number, keeping integers exact and rejecting overflow.
    fn number(&mut self) -> Result<Value, IngressReject> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => self.skip_digits(),
            _ => return Err(IngressReject::JsonSyntax),
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(IngressReject::JsonSyntax);
            }
            self.skip_digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(IngressReject::JsonSyntax);
            }
            self.skip_digits();
        }
        let lexeme = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| IngressReject::JsonNotUtf8)?;
        if is_float {
            let number: f64 = lexeme
                .parse()
                .map_err(|_| IngressReject::JsonNumberOutOfRange)?;
            let rendered =
                serde_json::Number::from_f64(number).ok_or(IngressReject::JsonNumberOutOfRange)?;
            return Ok(Value::Float(rendered.to_string()));
        }
        if lexeme.starts_with('-') {
            lexeme
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| IngressReject::JsonIntegerOutOfRange)
        } else {
            lexeme
                .parse::<u64>()
                .map(Value::Uint)
                .map_err(|_| IngressReject::JsonIntegerOutOfRange)
        }
    }

    /// Advances over a run of decimal digits.
    fn skip_digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }
}

/// One hexadecimal digit, or [`IngressReject::JsonSyntax`].
fn hex_digit(byte: Option<u8>) -> Result<u8, IngressReject> {
    match byte {
        Some(digit @ b'0'..=b'9') => Ok(digit - b'0'),
        Some(digit @ b'a'..=b'f') => Ok(digit - b'a' + 10),
        Some(digit @ b'A'..=b'F') => Ok(digit - b'A' + 10),
        _ => Err(IngressReject::JsonSyntax),
    }
}
