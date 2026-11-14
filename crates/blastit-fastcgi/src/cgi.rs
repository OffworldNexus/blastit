//! The CGI response head on `STDOUT` (design §4.5.2, §5).
//!
//! The first `\r\n\r\n` of the stream ends the head; [`head_end`] finds it and
//! [`parse_head`] reads it. The head is untrusted pool output, so:
//!
//! * `Status:` must be three digits in `200..=599` (a `1xx` or anything else is
//!   [`CgiError::BadHead`], never forwarded) and defaults to `200`;
//! * a header name must be an RFC 7230 `tchar` token ([`CgiError::NonTokenName`]);
//! * a value may not hold `0x00..=0x08`, `0x0a..=0x1f` or `0x7f`;
//! * `X-Accel-Redirect`, `X-Sendfile`, `X-Lighttpd-Send-File` and
//!   `X-Rewrite-URL` are [`CgiError::ForbiddenProxyHeader`], while every other
//!   `X-Accel-*` name is dropped without a fault;
//! * a head is at most [`MAX_HEAD_BYTES`], [`MAX_HEADERS`] fields and
//!   [`MAX_VALUE_BYTES`] per value.
//!
//! There is never a partial head: any refusal is an `Err`.

use core::fmt;

/// Largest head this crate parses, terminator included: 64 KiB.
pub const MAX_HEAD_BYTES: usize = 64 * 1024;
/// Largest number of header fields in one head.
pub const MAX_HEADERS: usize = 100;
/// Largest single header value, in bytes.
pub const MAX_VALUE_BYTES: usize = 8 * 1024;
/// The status a script that sends no `Status:` is served with.
pub const DEFAULT_STATUS: u16 = 200;
/// The `Content-Type` a script that sends none is served with.
pub const DEFAULT_CONTENT_TYPE: &str = "text/html";

/// The four names that would make a host send a file or rewrite a URL outside
/// the route's `open_basedir`; compared on the token name, ASCII
/// case-insensitively.
const PROXY_CONTROL: [&[u8]; 4] = [
    b"x-accel-redirect",
    b"x-sendfile",
    b"x-lighttpd-send-file",
    b"x-rewrite-url",
];

/// The prefix of every nginx response header that is dropped, not refused.
const X_ACCEL_PREFIX: &[u8] = b"x-accel-";

/// One forwarded header field.
#[derive(Clone, PartialEq, Eq)]
pub struct Field {
    /// The name exactly as sent; it is a `tchar` token.
    pub name: String,
    /// The value with surrounding optional whitespace removed.
    pub value: String,
}

impl fmt::Debug for Field {
    /// The log-safe view: the name and the value's length, never the value,
    /// because a header value routinely carries a cookie or a token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Field")
            .field("name", &self.name)
            .field("value_len", &self.value.len())
            .finish()
    }
}

/// A parsed CGI response head.
#[derive(Clone, PartialEq, Eq)]
pub struct Head {
    /// `Status:` if the script sent one, otherwise [`DEFAULT_STATUS`].
    pub status: u16,
    /// Every forwarded field in wire order. `Status` is consumed here and never
    /// forwarded; hop-by-hop stripping and the `Content-Length` removal stay in
    /// the scheduler.
    pub headers: Vec<Field>,
    /// The `Content-Type` **essence** — the media type before the first `;`,
    /// with optional whitespace trimmed — lossy-converted, or
    /// [`DEFAULT_CONTENT_TYPE`] when the script sent none. The full field value
    /// (parameters included) stays in [`Head::headers`] for forwarding.
    ///
    /// An empty essence (`Content-Type: ; charset=utf-8`, or a bare
    /// `Content-Type:`) yields an empty string, never a fault: the essence is
    /// not validated here, and treating it as absent is the caller's decision.
    pub content_type: String,
}

impl Head {
    /// The first value of `name`, ASCII case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|field| field.name.eq_ignore_ascii_case(name))
            .map(|field| field.value.as_str())
    }
}

impl fmt::Debug for Head {
    /// The log-safe view: the status, the `Content-Type` essence (the one
    /// recorded header field, `R18`) and the header *names*, never a value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self
            .headers
            .iter()
            .map(|field| field.name.as_str())
            .collect();
        f.debug_struct("Head")
            .field("status", &self.status)
            .field("content_type", &self.content_type)
            .field("headers", &names)
            .finish()
    }
}

/// The offset just past the head, or `None` while it is still incomplete.
///
/// The head ends at the first `\r\n\r\n`, or at a stream that begins with the
/// blank line — a head with no fields at all. Only the first
/// [`MAX_HEAD_BYTES`] bytes are searched, so a head that has not terminated by
/// then reads as incomplete: a caller holding that many bytes without a
/// delimiter must fault instead of waiting.
pub fn head_end(buf: &[u8]) -> Option<usize> {
    if buf.starts_with(b"\r\n") {
        return Some(2);
    }
    let limit = buf.len().min(MAX_HEAD_BYTES);
    let hay = buf.get(..limit)?;
    let end = hay.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    (end <= MAX_HEAD_BYTES).then_some(end)
}

/// Parse a complete head, terminator included.
pub fn parse_head(head: &[u8]) -> Result<Head, CgiError> {
    if head.len() > MAX_HEAD_BYTES {
        return Err(CgiError::BadHead);
    }
    // The head ends with the blank line; every field line before it ends CRLF.
    let body = head.strip_suffix(b"\r\n").ok_or(CgiError::BadHead)?;
    if !body.is_empty() && !body.ends_with(b"\r\n") {
        return Err(CgiError::BadHead);
    }

    let mut status = None;
    let mut headers: Vec<Field> = Vec::new();
    let mut content_type = None;
    let mut framing = (0usize, 0usize);
    let mut count = 0usize;

    let mut rest = body;
    while !rest.is_empty() {
        count += 1;
        if count > MAX_HEADERS {
            return Err(CgiError::BadHead);
        }
        let end = rest
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or(CgiError::BadHead)?;
        let line = rest.get(..end).ok_or(CgiError::BadHead)?;
        rest = rest.get(end + 2..).ok_or(CgiError::BadHead)?;
        if line.is_empty() {
            return Err(CgiError::BadHead);
        }
        let colon = line
            .iter()
            .position(|&byte| byte == b':')
            .ok_or(CgiError::BadHead)?;
        let name = line.get(..colon).ok_or(CgiError::BadHead)?;
        let value = line.get(colon + 1..).ok_or(CgiError::BadHead)?;
        if !is_token(name) {
            return Err(CgiError::NonTokenName);
        }
        let value = trim_ows(value);
        if value.len() > MAX_VALUE_BYTES || value.iter().any(|&b| is_forbidden(b)) {
            return Err(CgiError::BadHead);
        }
        if PROXY_CONTROL
            .iter()
            .any(|control| name.eq_ignore_ascii_case(control))
        {
            return Err(CgiError::ForbiddenProxyHeader);
        }
        if name
            .get(..X_ACCEL_PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(X_ACCEL_PREFIX))
        {
            continue;
        }
        if name.eq_ignore_ascii_case(b"status") {
            if status.is_some() {
                return Err(CgiError::BadHead);
            }
            status = Some(parse_status(value)?);
            continue;
        }
        if name.eq_ignore_ascii_case(b"content-length") {
            framing.0 += 1;
        } else if name.eq_ignore_ascii_case(b"transfer-encoding") {
            framing.1 += 1;
        }
        if framing.0 > 1 || framing.1 > 1 {
            return Err(CgiError::BadHead);
        }
        if content_type.is_none() && name.eq_ignore_ascii_case(b"content-type") {
            content_type = Some(String::from_utf8_lossy(essence(value)).into_owned());
        }
        let value = String::from_utf8_lossy(value).into_owned();
        headers.push(Field {
            name: String::from_utf8_lossy(name).into_owned(),
            value,
        });
    }

    Ok(Head {
        status: status.unwrap_or(DEFAULT_STATUS),
        headers,
        content_type: content_type.unwrap_or_else(|| DEFAULT_CONTENT_TYPE.to_owned()),
    })
}

/// Three digits, optionally followed by one SP and a reason phrase.
fn parse_status(value: &[u8]) -> Result<u16, CgiError> {
    let mut code = 0u16;
    for &byte in value.get(..3).ok_or(CgiError::BadHead)? {
        let digit = byte.checked_sub(b'0').filter(|digit| *digit < 10);
        code = code * 10 + u16::from(digit.ok_or(CgiError::BadHead)?);
    }
    if let Some(rest) = value.get(3..)
        && !rest.is_empty()
        && rest.first() != Some(&b' ')
    {
        return Err(CgiError::BadHead);
    }
    if !(200..=599).contains(&code) {
        return Err(CgiError::BadHead);
    }
    Ok(code)
}

/// An RFC 7230 `tchar` token: non-empty and every byte a `tchar`.
fn is_token(name: &[u8]) -> bool {
    !name.is_empty() && name.iter().all(|&byte| is_tchar(byte))
}

fn is_tchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// The control bytes a value may never hold: NUL through BS, LF through US, and
/// DEL. HTAB (0x09) and obs-text (0x80..=0xff) are allowed.
fn is_forbidden(byte: u8) -> bool {
    matches!(byte, 0x00..=0x08 | 0x0a..=0x1f | 0x7f)
}

/// Strip leading and trailing optional whitespace (SP and HTAB).
fn trim_ows(mut value: &[u8]) -> &[u8] {
    while let [b' ' | b'\t', rest @ ..] = value {
        value = rest;
    }
    while let [rest @ .., b' ' | b'\t'] = value {
        value = rest;
    }
    value
}

/// The media-type essence of a header value: everything before the first `;`,
/// with optional whitespace trimmed (`R18`). An empty essence stays empty.
fn essence(value: &[u8]) -> &[u8] {
    let before = value
        .iter()
        .position(|&byte| byte == b';')
        .and_then(|semi| value.get(..semi))
        .unwrap_or(value);
    trim_ows(before)
}

/// Everything a CGI head parser can refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgiError {
    /// The head is not a CGI response head: no `\r\n\r\n`, a line without a
    /// colon, an empty line inside the head, a `Status:` that is not three
    /// digits in `200..=599`, a duplicate `Status`/`Content-Length`/
    /// `Transfer-Encoding`, a forbidden control byte, or a cap broken.
    BadHead,
    /// A header name is not an RFC 7230 `tchar` token (empty, NUL/CTL, space,
    /// `:`, obs-fold).
    NonTokenName,
    /// The head carries one of the four proxy-control names.
    ForbiddenProxyHeader,
}

impl fmt::Display for CgiError {
    /// Never prints a header name or value: both are untrusted pool output.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadHead => f.write_str("fastcgi: CGI response head is malformed"),
            Self::NonTokenName => f.write_str("fastcgi: CGI response header name is not a token"),
            Self::ForbiddenProxyHeader => {
                f.write_str("fastcgi: CGI response carries a proxy-control header")
            }
        }
    }
}

impl std::error::Error for CgiError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(lines: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for line in lines {
            out.extend_from_slice(line.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        out
    }

    fn parse(lines: &[&str]) -> Result<Head, CgiError> {
        parse_head(&head(lines))
    }

    #[test]
    fn an_empty_head_is_200_text_html() {
        let head = parse(&[]).expect("parses");
        assert_eq!(head.status, DEFAULT_STATUS);
        assert_eq!(head.content_type, DEFAULT_CONTENT_TYPE);
        assert!(head.headers.is_empty());
        assert_eq!(head.header("content-type"), None);
    }

    #[test]
    fn status_accepts_200_to_599_and_is_never_forwarded() {
        for status in [200u16, 201, 204, 302, 404, 500, 599] {
            let with_phrase = format!("Status: {status} OK");
            let head = parse(&[&with_phrase]).expect("parses");
            assert_eq!(head.status, status);
            assert!(head.headers.is_empty(), "Status is consumed");
            let bare = format!("Status: {status}");
            assert_eq!(parse(&[&bare]).expect("parses").status, status);
        }
    }

    #[test]
    fn status_refuses_1xx_and_out_of_range() {
        for line in [
            "Status: 100 Continue",
            "Status: 199",
            "Status: 600",
            "Status: 099",
            "Status: 99",
            "Status: abc",
            "Status: 2000",
            "Status: 200OK",
            "Status:",
            "Status: 200\tOK",
            "Status: 2 0 0",
        ] {
            assert_eq!(parse(&[line]), Err(CgiError::BadHead), "{line}");
        }
        assert_eq!(
            parse(&["Status: 200", "status: 201"]),
            Err(CgiError::BadHead)
        );
    }

    #[test]
    fn forbidden_value_bytes_are_refused() {
        for byte in [0u8, 1, 8, 10, 11, 12, 13, 31, 127] {
            let mut raw = b"X-Test: a".to_vec();
            raw.push(byte);
            raw.extend_from_slice(b"\r\n\r\n");
            assert_eq!(
                parse_head(&raw).err(),
                Some(CgiError::BadHead),
                "byte {byte}"
            );
        }
        // HTAB and obs-text are not control bytes.
        let mut raw = b"X-Test: a\tb".to_vec();
        raw.extend_from_slice(b"\xc3\xa9\r\n\r\n");
        let head = parse_head(&raw).expect("parses");
        assert_eq!(head.header("X-Test"), Some("a\tb\u{e9}"));
    }

    #[test]
    fn non_token_header_name_is_fault() {
        for line in [
            "X-Accel-Redirect\0: /x",
            " X-Accel-Redirect: /x",
            "X Accel: /x",
            ": /x",
            "X\tAccel: /x",
            "X-Accel-Redirect : /x",
            "X(Accel): /x",
        ] {
            assert_eq!(parse(&[line]), Err(CgiError::NonTokenName), "{line:?}");
        }
        // An obs-fold continuation line has an empty name.
        assert_eq!(parse(&["X-Test: a", " b: c"]), Err(CgiError::NonTokenName));
    }

    #[test]
    fn the_four_proxy_control_names_are_refused() {
        for name in [
            "X-Accel-Redirect",
            "x-sendfile",
            "X-Lighttpd-Send-File",
            "X-REWRITE-URL",
        ] {
            let line = format!("{name}: /srv/secret");
            assert_eq!(
                parse(&[&line]),
                Err(CgiError::ForbiddenProxyHeader),
                "{name}"
            );
        }
    }

    #[test]
    fn other_x_accel_names_are_dropped() {
        let head = parse(&[
            "X-Accel-Buffering: no",
            "X-Accel-Expires: 0",
            "X-Accel-Charset: utf-8",
            "X-Test: 1",
        ])
        .expect("parses");
        assert_eq!(head.status, 200);
        assert_eq!(
            head.headers,
            vec![Field {
                name: "X-Test".to_owned(),
                value: "1".to_owned(),
            }]
        );
    }

    #[test]
    fn duplicate_framing_headers_are_refused() {
        assert_eq!(
            parse(&["Content-Length: 1", "content-length: 1"]),
            Err(CgiError::BadHead)
        );
        assert_eq!(
            parse(&["Transfer-Encoding: chunked", "transfer-encoding: chunked"]),
            Err(CgiError::BadHead)
        );
        assert!(parse(&["Content-Length: 1", "Transfer-Encoding: chunked"]).is_ok());
    }

    #[test]
    fn missing_colon_and_empty_line_are_refused() {
        assert_eq!(parse(&["X-Test"]), Err(CgiError::BadHead));
        assert_eq!(
            parse(&["X-Test: 1", "", "X-Other: 2"]),
            Err(CgiError::BadHead)
        );
        assert_eq!(parse_head(b"X-Test: 1"), Err(CgiError::BadHead));
    }

    #[test]
    fn the_caps_are_enforced() {
        let many: Vec<String> = (0..MAX_HEADERS).map(|i| format!("X-H{i}: v")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(
            parse(&refs).is_ok(),
            "exactly {MAX_HEADERS} fields are allowed"
        );
        let mut over = refs.clone();
        over.push("X-Over: v");
        assert_eq!(parse(&over), Err(CgiError::BadHead));

        let fits = format!("X-Big: {}", "v".repeat(MAX_VALUE_BYTES));
        assert!(parse(&[&fits]).is_ok());
        let too_big = format!("X-Big: {}", "v".repeat(MAX_VALUE_BYTES + 1));
        assert_eq!(parse(&[&too_big]), Err(CgiError::BadHead));

        // The total head cap, exactly: eight field lines of individually legal
        // values (seven at the 8 KiB value cap, one shorter) plus the blank line
        // land on exactly `MAX_HEAD_BYTES` and parse; one more byte is refused.
        let mut at_cap = Vec::new();
        for _ in 0..7 {
            at_cap
                .extend_from_slice(format!("X-F: {}\r\n", "v".repeat(MAX_VALUE_BYTES)).as_bytes());
        }
        at_cap.extend_from_slice(format!("X-F: {}\r\n", "v".repeat(8_134)).as_bytes());
        at_cap.extend_from_slice(b"\r\n");
        assert_eq!(at_cap.len(), MAX_HEAD_BYTES);
        let head = parse_head(&at_cap).expect("exactly MAX_HEAD_BYTES parses");
        assert_eq!(head.headers.len(), 8);

        let mut over_cap = Vec::new();
        for _ in 0..7 {
            over_cap
                .extend_from_slice(format!("X-F: {}\r\n", "v".repeat(MAX_VALUE_BYTES)).as_bytes());
        }
        over_cap.extend_from_slice(format!("X-F: {}\r\n", "v".repeat(8_135)).as_bytes());
        over_cap.extend_from_slice(b"\r\n");
        assert_eq!(over_cap.len(), MAX_HEAD_BYTES + 1);
        assert_eq!(parse_head(&over_cap), Err(CgiError::BadHead));
    }

    #[test]
    fn head_end_finds_the_terminator() {
        assert_eq!(head_end(b"X-Test: 1\r\n\r\n"), Some(13));
        assert_eq!(head_end(b"X-Test: 1\r\n"), None);
        assert_eq!(head_end(b""), None);
        assert_eq!(head_end(b"A: b\r\n\r\nbody"), Some(8));
        assert_eq!(head_end(&vec![b'v'; MAX_HEAD_BYTES + 1]), None);
        let raw = b"Status: 404 Not Found\r\nContent-Type: text/plain\r\n\r\nbody";
        let end = head_end(raw).expect("terminated");
        let head = parse_head(&raw[..end]).expect("parses");
        assert_eq!(
            (head.status, head.content_type.as_str()),
            (404, "text/plain")
        );
    }

    #[test]
    fn content_type_is_lossy_and_defaulted() {
        assert_eq!(
            parse(&["Content-Type: application/json"])
                .expect("parses")
                .content_type,
            "application/json"
        );
        let mut raw = b"Content-Type: text/".to_vec();
        raw.push(0x80);
        raw.extend_from_slice(b"\r\n\r\n");
        let head = parse_head(&raw).expect("parses");
        assert_eq!(head.content_type, "text/\u{fffd}");
        assert_eq!(head.header("content-type"), Some("text/\u{fffd}"));
    }

    #[test]
    fn content_type_is_the_essence_only() {
        // A parameterised value records the media type before the first `;`,
        // OWS-trimmed, while the forwarded field keeps the whole value (`R18`).
        for (line, essence) in [
            ("Content-Type: text/html; charset=utf-8", "text/html"),
            ("Content-Type: text/html;charset=utf-8", "text/html"),
            ("Content-Type: text/html", "text/html"),
            ("Content-Type: text/html ; charset=utf-8", "text/html"),
            (
                "Content-Type: application/json; boundary=\"x;y\"",
                "application/json",
            ),
        ] {
            let head = parse(&[line]).expect("parses");
            assert_eq!(head.content_type, essence, "{line}");
            let full = line.split_once(": ").expect("header line").1;
            assert_eq!(head.header("content-type"), Some(full), "{line}");
        }
        // An empty essence is empty, never a fault (documented on `Head`).
        for line in ["Content-Type: ; charset=utf-8", "Content-Type:"] {
            let head = parse(&[line]).expect("parses");
            assert_eq!(head.content_type, "", "{line}");
        }
    }

    #[test]
    fn lookup_is_case_insensitive_and_ows_is_trimmed() {
        let head = parse(&["x-test:\t  a b  \t"]).expect("parses");
        assert_eq!(head.header("X-Test"), Some("a b"));
        assert_eq!(head.header("x-missing"), None);
        assert_eq!(head.headers.len(), 1);
    }

    #[test]
    fn the_debug_view_never_prints_a_value() {
        let head = parse(&["Set-Cookie: session=secret"]).expect("parses");
        let text = format!("{head:?}");
        assert!(text.contains("Set-Cookie"), "{text}");
        assert!(!text.contains("secret"), "{text}");
    }

    #[test]
    fn a_field_debug_never_prints_its_value() {
        let head = parse(&["Set-Cookie: session=secret"]).expect("parses");
        let field = head.headers.first().expect("one field");
        let text = format!("{field:?}");
        assert!(text.contains("Set-Cookie"), "{text}");
        assert!(text.contains("value_len"), "{text}");
        assert!(!text.contains("secret"), "{text}");
    }

    #[test]
    fn errors_print_without_names_or_values() {
        for (error, expected) in [
            (CgiError::BadHead, "fastcgi: CGI response head is malformed"),
            (
                CgiError::NonTokenName,
                "fastcgi: CGI response header name is not a token",
            ),
            (
                CgiError::ForbiddenProxyHeader,
                "fastcgi: CGI response carries a proxy-control header",
            ),
        ] {
            assert_eq!(error.to_string(), expected);
        }
    }
}
