//! Protected critical tests: the CGI head parser and the record/param walls
//! (design `docs/design/php-fastcgi.md` §4.5.1–§4.5.2; the codec half of §4.7
//! row 24).
//!
//! Every test is binary: response bytes in, one typed [`CgiError`] /
//! [`RecordError`] or the exact parsed value out, and every one pins the denied
//! path *and* its accepted twin, so a parser that refused everything (or
//! accepted everything) could not pass. The status mapping (`Fault` → `502`)
//! is `MB.3.6.2`'s and is deliberately absent here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_fastcgi::cgi::{
    CgiError, DEFAULT_CONTENT_TYPE, DEFAULT_STATUS, Field, Head, MAX_HEAD_BYTES, MAX_HEADERS,
    MAX_VALUE_BYTES, head_end, parse_head,
};
use blastit_fastcgi::param::{decode_all, encode_pair};
use blastit_fastcgi::record::{EndRequest, FCGI_REQUEST_COMPLETE, Header, RecordError, RecordType};

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

/// The four proxy-control names are the sendfile escape out of `open_basedir`:
/// they must be refused on the token name, ASCII case-insensitively (§4.5.2
/// `STDOUT`, §4.7 row 24).
#[test]
fn the_four_proxy_control_names_are_refused_case_insensitively() {
    for line in [
        "X-aCcEl-ReDiReCt: /etc/passwd",
        "x-SENDFILE: /srv/secret",
        "X-LIGHTTPD-send-file: /srv/secret",
        "X-rewrite-URL: /internal",
    ] {
        assert_eq!(
            parse(&[line]),
            Err(CgiError::ForbiddenProxyHeader),
            "{line}"
        );
    }
    // Accepted twin: refusal is exact-name only. A longer name that merely
    // contains a control name is an ordinary forwarded field.
    let lookalike = parse(&["X-Sendfile-Extra: 1", "X-Rewrite-URL2: 2"]).expect("parses");
    assert_eq!(lookalike.status, DEFAULT_STATUS);
    assert_eq!(lookalike.header("X-Sendfile-Extra"), Some("1"));
    assert_eq!(lookalike.header("X-Rewrite-URL2"), Some("2"));
    // An added `x-accel-*` name is dropped by the prefix rule, never forwarded.
    let dropped = parse(&["X-Accel-Redirect-Extra: /x"]).expect("parses");
    assert!(dropped.headers.is_empty());
}

/// A benign SSE directive (`X-Accel-Buffering: no`) must keep the response
/// `200`: every other `X-Accel-*` name is dropped, never a fault (§4.5.2,
/// MINOR-19).
#[test]
fn benign_x_accel_directives_are_dropped_not_faulted() {
    let head = parse(&[
        "X-Accel-Buffering: no",
        "X-Accel-Expires: 0",
        "X-Accel-Charset: utf-8",
        "X-Accel-Limit-Rate: 1m",
        "X-Test: 1",
    ])
    .expect("a benign X-Accel directive must not become a 502");
    assert_eq!(head.status, DEFAULT_STATUS);
    assert_eq!(
        head.headers,
        vec![Field {
            name: "X-Test".to_owned(),
            value: "1".to_owned(),
        }]
    );
    // The drop rule never swallows a control name.
    assert_eq!(
        parse(&["X-Accel-Redirect: /x"]),
        Err(CgiError::ForbiddenProxyHeader)
    );
}

/// A header name that is not an RFC 7230 `tchar` token is `NonTokenName`
/// (§4.5.2 caps row, MINOR-35): NUL and a leading space are refused, never
/// trimmed into a control name.
#[test]
fn non_token_header_names_are_refused() {
    assert_eq!(
        parse(&["X-Accel-Redirect\0: /x"]),
        Err(CgiError::NonTokenName)
    );
    assert_eq!(
        parse(&[" X-Accel-Redirect: /x"]),
        Err(CgiError::NonTokenName)
    );
    // Accepted twin: the same names once the NUL/space is gone reach the
    // proxy-control wall (not the token wall), and a benign token parses.
    assert_eq!(
        parse(&["X-Accel-Redirect: /x"]),
        Err(CgiError::ForbiddenProxyHeader)
    );
    let benign = parse(&["X-Accel-Buffering: no"]).expect("token name parses");
    assert!(benign.headers.is_empty());
}

/// `Status:` outside `200..=599` is `BadHead`; the accepted twins show the
/// range edges still parse (§4.5.1 `cgi` row).
#[test]
fn status_outside_200_599_is_bad_head() {
    for line in ["Status: 100", "Status: 199", "Status: 600", "Status: 099"] {
        assert_eq!(parse(&[line]), Err(CgiError::BadHead), "{line}");
    }
    for (line, want) in [("Status: 200", 200u16), ("Status: 599", 599)] {
        assert_eq!(parse(&[line]).expect("parses").status, want, "{line}");
    }
}

/// The `parse_status` digit filter: `':'` (58) minus `b'0'` (48) is 10, the
/// exact byte a `<= 10` filter would accept, so a malformed head would become
/// a different status instead of a `502`.
#[test]
fn status_with_a_non_digit_byte_is_bad_head() {
    for line in ["Status: 2:0", "Status: 20:", "Status: :00"] {
        assert_eq!(parse(&[line]), Err(CgiError::BadHead), "{line}");
    }
    for (line, want) in [("Status: 200 OK", 200u16), ("Status: 204 No Content", 204)] {
        assert_eq!(parse(&[line]).expect("parses").status, want, "{line}");
    }
}

/// A second `Status` line is `BadHead`; one `Status` is consumed and never
/// forwarded.
#[test]
fn duplicate_status_is_bad_head() {
    assert_eq!(
        parse(&["Status: 200", "status: 201"]),
        Err(CgiError::BadHead)
    );
    assert_eq!(
        parse(&["Status: 204", "Status: 204"]),
        Err(CgiError::BadHead)
    );
    let head = parse(&["Status: 204", "X-Test: 1"]).expect("parses");
    assert_eq!(head.status, 204);
    assert_eq!(head.header("Status"), None);
    assert_eq!(head.headers.len(), 1);
}

/// With no `Status:` the head is `200`/`text/html`; an explicit `Status:`
/// wins, and a field-less head is still `200` — the `head_end` early return.
#[test]
fn absent_status_is_200_text_html() {
    let head = parse(&["X-Test: 1"]).expect("parses");
    assert_eq!(head.status, DEFAULT_STATUS);
    assert_eq!(head.content_type, DEFAULT_CONTENT_TYPE);
    assert_eq!(
        parse(&["Status: 204 No Content"]).expect("parses").status,
        204
    );

    let raw = b"\r\n\r\nbody";
    let end = head_end(raw).expect("terminated");
    assert_eq!(end, 2);
    let fieldless = parse_head(&raw[..end]).expect("parses");
    assert_eq!(fieldless.status, DEFAULT_STATUS);
    assert_eq!(fieldless.content_type, DEFAULT_CONTENT_TYPE);
    assert!(fieldless.headers.is_empty());
}

/// The response head carries at most 100 fields (§4.5.2).
#[test]
fn header_count_cap_is_exactly_100() {
    // Name the literal: `MAX_HEADERS` drifting would otherwise keep this green.
    assert_eq!(MAX_HEADERS, 100);
    let many: Vec<String> = (0..MAX_HEADERS).map(|i| format!("X-H{i}: v")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let at_cap = parse(&refs).expect("exactly MAX_HEADERS fields parse");
    assert_eq!(at_cap.headers.len(), MAX_HEADERS);
    let expected: Vec<Field> = (0..MAX_HEADERS)
        .map(|i| Field {
            name: format!("X-H{i}"),
            value: "v".to_owned(),
        })
        .collect();
    assert_eq!(
        at_cap.headers, expected,
        "every field survives in wire order"
    );
    let mut over = refs.clone();
    over.push("X-Over: v");
    assert_eq!(parse(&over), Err(CgiError::BadHead));
}

/// A single header value is at most 8 192 bytes (§4.5.2).
#[test]
fn value_cap_is_exactly_8192() {
    assert_eq!(MAX_VALUE_BYTES, 8_192);
    let fits = format!("X-Big: {}", "v".repeat(MAX_VALUE_BYTES));
    let parsed = parse(&[&fits]).expect("a value at the cap parses");
    assert_eq!(parsed.headers.len(), 1);
    assert_eq!(
        parsed.header("X-Big").expect("X-Big survives").len(),
        MAX_VALUE_BYTES
    );
    let too_big = format!("X-Big: {}", "v".repeat(MAX_VALUE_BYTES + 1));
    assert_eq!(parse(&[&too_big]), Err(CgiError::BadHead));
}

/// The whole head is at most [`MAX_HEAD_BYTES`], terminator included.
#[test]
fn head_total_cap_is_exactly_64_kib() {
    let mut at_cap = Vec::new();
    for _ in 0..7 {
        at_cap.extend_from_slice(format!("X-F: {}\r\n", "v".repeat(MAX_VALUE_BYTES)).as_bytes());
    }
    at_cap.extend_from_slice(format!("X-F: {}\r\n", "v".repeat(8_134)).as_bytes());
    at_cap.extend_from_slice(b"\r\n");
    // No literal 65 536 needed: this buffer is a fixed 7·8 199 + 8 141 + 2, so
    // this line already fails if `MAX_HEAD_BYTES` drifts.
    assert_eq!(at_cap.len(), MAX_HEAD_BYTES);
    // §4.5.2 is "≤ 64 KiB total", so an end offset *equal* to the cap is a
    // complete head: a `<` on that boundary would fault a legal 64 KiB head.
    assert_eq!(head_end(&at_cap), Some(at_cap.len()));
    let parsed = parse_head(&at_cap).expect("exactly MAX_HEAD_BYTES parses");
    assert_eq!(parsed.headers.len(), 8);

    let mut over_cap = Vec::new();
    for _ in 0..7 {
        over_cap.extend_from_slice(format!("X-F: {}\r\n", "v".repeat(MAX_VALUE_BYTES)).as_bytes());
    }
    over_cap.extend_from_slice(format!("X-F: {}\r\n", "v".repeat(8_135)).as_bytes());
    over_cap.extend_from_slice(b"\r\n");
    assert_eq!(over_cap.len(), MAX_HEAD_BYTES + 1);
    assert_eq!(parse_head(&over_cap), Err(CgiError::BadHead));
    // Its terminator lies one byte past the searched window: still incomplete.
    assert_eq!(head_end(&over_cap), None);
}

/// `END_REQUEST.protocolStatus` other than Complete is
/// [`RecordError::ProtocolStatus`]; Complete decodes (§4.5.1 `END_REQUEST`).
#[test]
fn end_request_requires_complete_protocol_status() {
    let complete = [0, 0, 0, 7, FCGI_REQUEST_COMPLETE, b'A', b'A', b'A'];
    let end = EndRequest::decode(&complete).expect("complete decodes");
    assert_eq!(end.app_status, 7);
    assert_eq!(end.protocol_status, FCGI_REQUEST_COMPLETE);
    for status in [1u8, 2, 3, 0xff] {
        let content = [0, 0, 0, 0, status, 0, 0, 0];
        assert_eq!(
            EndRequest::decode(&content),
            Err(RecordError::ProtocolStatus),
            "protocolStatus {status}"
        );
    }
}

/// A record header whose `CL + PL` exceeds 65 535 is `Truncated`, exactly
/// 65 535 is legal; a truncated name-value pair is `Truncated` too (§4.5.1).
#[test]
fn overlong_record_header_and_truncated_pair_are_refused() {
    let mut wire = Header::new(RecordType::Stdout, 1, 0)
        .expect("fits")
        .encode();
    wire[4] = 0xff;
    wire[5] = 0xff;
    wire[6] = 0;
    assert_eq!(
        Header::decode(&wire).map(|h| usize::from(h.content_length)),
        Ok(65_535)
    );
    wire[6] = 1;
    assert_eq!(Header::decode(&wire), Err(RecordError::Truncated));

    let mut pair = Vec::new();
    encode_pair(b"NAME", b"value", &mut pair).expect("encodes");
    let decoded = decode_all(&pair).expect("decodes");
    assert_eq!(decoded.len(), 1);
    assert_eq!(
        (decoded[0].name, decoded[0].value),
        (&b"NAME"[..], &b"value"[..])
    );
    for cut in 1..pair.len() {
        assert_eq!(
            decode_all(&pair[..cut]).map(|_| ()),
            Err(RecordError::Truncated),
            "cut {cut}"
        );
    }
}
