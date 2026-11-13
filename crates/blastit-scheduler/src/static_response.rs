//! Building one static response from an already-resolved [`StaticFile`]
//! (`R5`, `R9`, design `MB.2.4b` §4.2, §4.5–§4.6).
//!
//! [`StaticRoot::respond`] is the whole head half of a static answer: the closed
//! `Content-Type` table, `Content-Length`, the quoted strong `ETag`, the
//! IMF-fixdate `Last-Modified`, `X-Content-Type-Options: nosniff`, the
//! `(answer kind, answering URL path)` cache rule and the `If-None-Match`
//! `304`. It never sets `Date`, `Server`, `Accept-Ranges` or `Location`, never
//! redirects, never negotiates and never re-opens the path: the descriptor
//! moves into [`StaticBody`], unduplicated, exactly as `static_asset.rs` opened
//! it.
//!
//! It never reads a byte either. `HEAD` answers the `200` head with a
//! `Content-Length` and no body, a `304` carries no body and no
//! `Content-Length`, and a `GET` hands the scheduler a [`StaticBody`] it pumps
//! in `STATIC_CHUNK_BYTES` reads (`MB.2.5.2`), so no response buffers a whole
//! file and no truncated body is ever sent under a promised `Content-Length`.
//!
//! The cache rule is keyed on the **answering** file: only an
//! [`AnswerKind::Exact`] answer whose `url_path` matches an `immutable` glob
//! earns the year; a directory `index` and a SPA fallback are always
//! `no-cache`, whatever a pattern says. A hashed request path must therefore
//! never be globbed on its own — `/dashboard/settings` answers `index.html`,
//! and `index.html` is what the rule looks at.

use std::os::fd::OwnedFd;

use crate::static_asset::{AnswerKind, StaticFile, StaticMeta, StaticRoot};

/// The one year-long cache value the page documents, verbatim (§4.5).
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";
/// Every other static answer's cache value, the `index` included (§4.5).
const REVALIDATE_CACHE: &str = "no-cache";
/// A refusal's cache value: the scheduler writes it, and the one unreachable
/// arm of [`StaticRoot::respond`] mirrors it so a refusal is never cached.
const NO_STORE_CACHE: &str = "no-store";
/// The closed `Content-Type` table's fallback (§4.5).
const OCTET_STREAM: &str = "application/octet-stream";

/// One complete static head, plus the body source when there is a body.
///
/// `status` is `200` or `304`; the only other value is the `404` of the
/// defence-in-depth arm for a [`StaticFile`] already above
/// `static_max_bytes` (§4.6 says such a file is `404`, and `classify` refuses
/// it before it can become a [`StaticFile`], so the arm is unreachable through
/// `resolve` + `decide`).
pub struct StaticResponse {
    /// The status to answer with.
    pub status: u16,
    /// The reason phrase that goes with it.
    pub reason: &'static str,
    /// The head's headers, in the order the responder builds them.
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// `None` for `HEAD` and `304`; the read-open file otherwise.
    pub body: Option<StaticBody>,
}

/// Where the scheduler's body pump reads from: the descriptor the resolver
/// already opened, never a second one.
pub struct StaticBody {
    /// The read-open `O_RDONLY` descriptor, moved in from the [`StaticFile`].
    pub file: OwnedFd,
    /// The first byte of the body.
    pub offset: u64,
    /// How many bytes the body has.
    pub len: u64,
}

impl StaticRoot {
    /// Builds the complete answer for `file` from the head alone.
    ///
    /// `file` is consumed: its descriptor moves into [`StaticBody`] (a `GET`
    /// with a body) or is dropped (`HEAD`, `304`), never duplicated. `method`
    /// is `GET` or `HEAD` — `decide` has already refused everything else, so
    /// anything but `HEAD` is answered as a `GET`.
    ///
    /// `if_none_match` is the request's `If-None-Match` value: a listed strong
    /// or weak (`W/`) `ETag`, or `*`, answers `304`. `Range`,
    /// `If-Modified-Since`, `If-Match`, `If-Range` and `If-Unmodified-Since`
    /// are not parameters and are therefore ignored: a full `200`.
    ///
    /// A zero-length file is an ordinary `200` with `Content-Length: 0` and a
    /// [`StaticBody`] whose `len` is `0`; the pump pushes no body frame for it.
    pub fn respond(
        &self,
        file: StaticFile,
        method: &[u8],
        if_none_match: Option<&[u8]>,
    ) -> StaticResponse {
        // Defence in depth: `classify` already turned this into `Hidden`, so a
        // resolved file is never over the ceiling. If a caller still hands one
        // over, refuse it rather than promise bytes this responder may not
        // stream.
        if file.len > self.max_bytes || file.meta.size > self.max_bytes {
            return refusal_not_found();
        }

        let etag = etag(&file.meta);
        let last_modified = http_date(file.meta.mtime_secs);
        let cache = cache_control(self, &file);
        if if_none_match.is_some_and(|list| matches_etag(list, &etag)) {
            return StaticResponse {
                status: 304,
                reason: "Not Modified",
                headers: vec![
                    header(b"ETag", etag.as_str()),
                    header(b"Last-Modified", last_modified.as_str()),
                    header(b"Cache-Control", cache),
                    header(b"X-Content-Type-Options", "nosniff"),
                ],
                body: None,
            };
        }

        let headers = vec![
            header(b"Content-Type", content_type(&file.url_path)),
            header(b"Content-Length", file.len.to_string()),
            header(b"ETag", etag.as_str()),
            header(b"Last-Modified", last_modified.as_str()),
            header(b"X-Content-Type-Options", "nosniff"),
            header(b"Cache-Control", cache),
        ];
        // `HEAD` is the `200` head with no body: the same `Content-Length` of
        // the would-be body and not one byte written.
        let body = if method == b"HEAD" {
            None
        } else {
            Some(StaticBody {
                file: file.file,
                offset: 0,
                len: file.len,
            })
        };
        StaticResponse {
            status: 200,
            reason: "OK",
            headers,
            body,
        }
    }
}

/// The one `404` this responder can build: a file above the ceiling.
fn refusal_not_found() -> StaticResponse {
    StaticResponse {
        status: 404,
        reason: "Not Found",
        headers: vec![
            header(b"Content-Length", "0"),
            header(b"Cache-Control", NO_STORE_CACHE),
        ],
        body: None,
    }
}

/// One `(name, value)` header pair in the form the frame head carries.
fn header(name: &[u8], value: impl Into<Vec<u8>>) -> (Vec<u8>, Vec<u8>) {
    (name.to_vec(), value.into())
}

/// The cache rule of §4.5, keyed on `(answer kind, answering path)`.
///
/// Only an exact-file answer whose **answering** `url_path` matches an
/// `immutable` glob gets the year. A directory `index` and the SPA fallback are
/// always `no-cache`, however a pattern is written: `/` and `/dashboard`
/// answer `index.html`, so a `/index.html` pattern must not year-cache them.
fn cache_control(root: &StaticRoot, file: &StaticFile) -> &'static str {
    if file.kind == AnswerKind::Exact
        && root
            .immutable
            .iter()
            .any(|pattern| pattern.matches(&file.url_path))
    {
        IMMUTABLE_CACHE
    } else {
        REVALIDATE_CACHE
    }
}

/// The quoted strong `ETag` of a file: `"<size:x>-<mtime_ns:x>"` (§4.5).
///
/// No inode or device, so the header discloses nothing about the tree. It
/// changes when a rewrite changes the size or the mtime, which a same-byte
/// rewrite may not; that is the design's accepted trade-off.
fn etag(meta: &StaticMeta) -> String {
    let nanos = i128::from(meta.mtime_secs) * 1_000_000_000 + i128::from(meta.mtime_nsec);
    format!("\"{:x}-{:x}\"", meta.size, nanos as u128)
}

/// Whether an `If-None-Match` value lists `etag` (or is `*`), ignoring a weak
/// `W/` prefix on either side (§4.5).
///
/// The value is a comma-separated list of entities; whitespace around each is
/// insignificant. A malformed entity simply matches nothing.
fn matches_etag(if_none_match: &[u8], etag: &str) -> bool {
    let wanted = etag.as_bytes();
    if_none_match.split(|byte| *byte == b',').any(|entity| {
        let entity = trim_ows(entity);
        if entity == b"*" {
            return true;
        }
        let entity = entity
            .strip_prefix(b"W/")
            .or_else(|| entity.strip_prefix(b"w/"))
            .unwrap_or(entity);
        entity == wanted
    })
}

/// Strips optional whitespace (space and tab) from both ends.
fn trim_ows(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| *byte != b' ' && *byte != b'\t')
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| *byte != b' ' && *byte != b'\t')
        .map_or(start, |last| last + 1);
    &bytes[start..end]
}

/// The closed `Content-Type` table of §4.5, from the **answering** file's
/// extension lowercased first (`.PNG` is `image/png`).
///
/// Anything else, including a name with no extension, is
/// `application/octet-stream`. There is no sniffing and no `charset` beyond
/// the table's own values.
fn content_type(url_path: &str) -> &'static str {
    let extension = match url_path.rsplit('/').next() {
        Some(name) => match name.rsplit_once('.') {
            Some((_, extension)) => extension,
            None => return OCTET_STREAM,
        },
        None => return OCTET_STREAM,
    };
    match extension.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        _ => OCTET_STREAM,
    }
}

/// The IMF-fixdate of `unix_secs`, in UTC (the `Last-Modified` of §4.5).
///
/// The inverse of days-from-civil, so no dependency is needed:
///
/// ```text
/// http_date(0)          == "Thu, 01 Jan 1970 00:00:00 GMT"
/// http_date(784111777)  == "Sun, 06 Nov 1994 08:49:37 GMT"
/// http_date(1700000000) == "Tue, 14 Nov 2023 22:13:20 GMT"
/// ```
pub fn http_date(unix_secs: i64) -> String {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let days = unix_secs.div_euclid(86_400);
    let seconds_of_day = unix_secs.rem_euclid(86_400);
    let weekday = usize::try_from((days + 4).rem_euclid(7)).unwrap_or(0);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!(
        "{}, {day:02} {} {year:04} {hour:02}:{minute:02}:{second:02} GMT",
        WEEKDAYS[weekday],
        MONTHS[usize::from(month - 1)]
    )
}

/// The civil date of a count of days since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`), total for every `i64`.
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = u64::try_from(shifted - era * 146_097).unwrap_or(0);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = i64::try_from(year_of_era).unwrap_or(0) + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = u8::try_from(day_of_year - (153 * month_prime + 2) / 5 + 1).unwrap_or(0);
    let month_prime = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let month = u8::try_from(month_prime).unwrap_or(0);
    (if month <= 2 { year + 1 } else { year }, month, day)
}
