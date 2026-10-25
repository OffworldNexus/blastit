//! Canonical reason phrases.
//!
//! A view may send a response with no reason at all (`RespHead.reason` empty),
//! in which case the front end fills in the registered phrase so that the
//! status line reads like every other server's (ARCHITECTURE.md §2.3).

/// The registered reason phrase for `status`, when there is one.
///
/// Statuses outside the table — including an application's own `5xx` code —
/// have no phrase, and the status line is written with an empty one.
///
/// ```
/// use blastit_http::canonical_reason;
///
/// assert_eq!(canonical_reason(404), Some("Not Found"));
/// assert_eq!(canonical_reason(599), None);
/// ```
pub fn canonical_reason(status: u16) -> Option<&'static str> {
    Some(match status {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a Teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Content",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_the_front_end_sends_has_a_phrase() {
        for status in [400u16, 408, 413, 417, 431, 501, 502, 505] {
            let phrase = canonical_reason(status).unwrap();
            assert!(!phrase.is_empty(), "{status} needs a phrase");
            assert!(
                phrase
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() || byte == b' '),
                "{phrase} must be printable"
            );
        }
    }

    #[test]
    fn unknown_statuses_have_no_phrase() {
        for status in [0u16, 99, 199, 306, 599, 999] {
            assert_eq!(canonical_reason(status), None, "status {status}");
        }
    }
}
