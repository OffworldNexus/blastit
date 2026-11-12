//! Request-body ingress validation and canonical re-encoding (design §4.2 A,
//! `ADR-0006`, `R14..R16`).
//!
//! A manifest route may declare `[route.ingress]` with `body = "json"`,
//! `"form"`, `"none"` or `"raw"`. The scheduler asks this crate what to do with
//! a request body, and for the structured codecs (`json`, `form`) hands the
//! bytes over to be decoded, validated and re-encoded before a view process
//! ever sees them. Every step fails closed: a body this crate cannot prove
//! canonical is a `400`, never a pass-through.
//!
//! The work is split in two so `raw` uploads never buffer:
//!
//! * [`plan_body`] reads only the request head. [`BodyCodec::Raw`] becomes
//!   [`BodyPlan::StreamPassthrough`] — the scheduler forwards frames through
//!   its `1 MiB` credit windows with **zero** body buffer (`R16`). A route with
//!   no `[route.ingress]` or `body = "none"` becomes
//!   [`BodyPlan::AllowEmptyOnly`], and any declared byte is a `413`. A
//!   non-identity `Content-Encoding` on `json`/`form` is a `400`, because
//!   compressed bytes would otherwise bypass validation.
//! * [`transcode_buffered`] runs on the buffered bytes of a `json` or `form`
//!   body and returns the canonical re-encoding plus the one `Content-Type` the
//!   view should see. [`apply_transcoded_headers`] then strips every stale
//!   framing header and appends exactly one `Content-Type` and one
//!   `Content-Length`.
//!
//! The `json` codec is strict RFC 8259 over UTF-8: a BOM, trailing bytes, a
//! duplicate object key, nesting deeper than [`MAX_JSON_DEPTH`], a lone
//! surrogate, or an integer lexeme outside `[i64::MIN, u64::MAX]` is a `400`.
//! The `form` codec validates every `%XX` escape, the decoded UTF-8 and the
//! absence of NUL bytes. `blastit check` samples bodies with [`probe_sample`]
//! and turns them into a manifest suggestion with [`suggest_fix`].
//!
//! Nothing in this crate may read a credential or log a body: the body sample
//! carries a MIME essence and three booleans, never bytes, and every rejection
//! names the rule, never the payload.
//!
//! ```
//! use blastit_ingress::{BodyCodec, BodyPlan, IngressPolicy, plan_body};
//!
//! let policy = IngressPolicy {
//!     body: BodyCodec::Json,
//!     allow_raw: false,
//! };
//! assert_eq!(
//!     plan_body("POST", Some(policy), Some("application/json"), None, Some(2)),
//!     Ok(BodyPlan::Buffer {
//!         codec: BodyCodec::Json
//!     })
//! );
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod form;
mod json;
mod mime;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Deepest JSON nesting the `json` codec accepts.
///
/// `serde_json`'s own parser is recursive; this bound keeps a hostile document
/// from reaching it with a nesting depth that would exhaust a thread's stack.
/// A document nested exactly `64` deep passes; `65` is a `400`.
pub const MAX_JSON_DEPTH: usize = 64;

/// Longest MIME essence kept in a body-sample report, in bytes.
///
/// The essence is lowercased ASCII and cut at this many bytes, so a hostile
/// `Content-Type` cannot grow a `blastit check` report without bound.
pub const MAX_MIME_REPORT_BYTES: usize = 64;

/// Most request-body bytes `blastit check` probes for its body sample.
///
/// A body longer than this is sampled as truncated: `valid_json` and
/// `valid_form` are forced `false`, and the suggested codecs fall back to
/// `raw`. The WSGI gate spools at most this many bytes per request.
pub const MAX_CHECK_SAMPLE_BYTES: usize = 64 * 1024;

/// How one route's request body is decoded and re-encoded.
///
/// The serialised spelling is the manifest's: `"json"`, `"form"`, `"none"`,
/// `"raw"` (`body = …` in `[route.ingress]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyCodec {
    /// Strict RFC 8259 JSON, re-encoded canonically with sorted keys.
    Json,
    /// `application/x-www-form-urlencoded`, re-encoded canonically.
    Form,
    /// No body at all: any body byte is a `413`.
    None,
    /// The body is streamed through unchanged with no scheduler buffer.
    Raw,
}

/// One route's `[route.ingress]` policy.
///
/// `None` at the use site means the route declared no `[route.ingress]`, which
/// is fail-closed exactly like [`BodyCodec::None`]: the route may receive an
/// empty body and nothing else (`R12`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressPolicy {
    /// The codec the route declared.
    pub body: BodyCodec,
    /// Whether `body = "raw"` also carried `allow_raw = true` in the manifest.
    ///
    /// Only meaningful for [`BodyCodec::Raw`]; the manifest rejects the key on
    /// every other codec.
    pub allow_raw: bool,
}

/// What the scheduler does with a request body, decided from the head alone.
///
/// Computed by [`plan_body`] **before** any body frame is read, so a `raw`
/// upload never allocates a scheduler buffer (`R16`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyPlan {
    /// `BodyCodec::Raw`: dispatch on the head and stream the frames through
    /// the `1 MiB` credit windows (`0` B scheduler buffer).
    StreamPassthrough,
    /// `None` / `BodyCodec::None` with no positive `Content-Length`: allow a
    /// zero-byte body (`ReqEnd`), and answer `413` on any `ReqBody` byte.
    AllowEmptyOnly,
    /// `BodyCodec::Json` or `BodyCodec::Form`: buffer up to
    /// `BLASTIT_MAX_BODY_BYTES`, then call [`transcode_buffered`].
    Buffer {
        /// The codec that will read the buffered bytes.
        codec: BodyCodec,
    },
}

/// One observed request body, as `blastit check` records it.
///
/// The sample never carries body bytes: only the MIME essence, whether the
/// body was too long to probe, and whether it validated as JSON or as a form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodySample {
    /// The lowercase essence (`"application/json"`, `"multipart/form-data"`,
    /// `"none"`), at most [`MAX_MIME_REPORT_BYTES`] bytes.
    pub mime: String,
    /// `true` when the body was longer than [`MAX_CHECK_SAMPLE_BYTES`], which
    /// forces both validity flags to `false`.
    pub truncated: bool,
    /// Whether the sampled bytes are canonical JSON under the `json` codec.
    pub valid_json: bool,
    /// Whether the sampled bytes validate under the `form` codec.
    pub valid_form: bool,
}

/// The canonical body a view receives, plus the one `Content-Type` to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodedBody {
    /// The re-encoded body bytes.
    pub bytes: Vec<u8>,
    /// The canonical `Content-Type` for these bytes, or `None` for an empty
    /// body that carries no type.
    pub content_type: Option<&'static str>,
}

/// Why a request body was refused.
///
/// Every variant is a `400 Bad Request` except [`IngressReject::BodyNotAllowed`],
/// which is the `413` of `R12`. Callers match on the variant; the `Display`
/// text is for humans and never contains body bytes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IngressReject {
    /// A `json`/`form` route received a non-identity `Content-Encoding`.
    #[error(
        "content-encoding {observed} is not supported on a structured body; send identity-encoded bytes"
    )]
    ContentEncodingNotSupported {
        /// The offending `Content-Encoding`, capped for a report.
        observed: String,
    },
    /// The `Content-Type` essence is not the one the codec requires.
    #[error("expected content-type {expected}, observed {observed}")]
    ContentTypeMismatch {
        /// The essence the codec accepts.
        expected: &'static str,
        /// The observed essence, capped for a report.
        observed: String,
    },
    /// The `Content-Type` charset is not UTF-8.
    #[error("charset {observed} is not supported; send charset=utf-8")]
    CharsetNotUtf8 {
        /// The observed charset, capped for a report.
        observed: String,
    },
    /// A body byte arrived on a route that accepts no body (`413`).
    #[error("this route accepts no request body")]
    BodyNotAllowed,
    /// A `json` route received a zero-byte body.
    #[error("an empty body is not valid JSON")]
    EmptyJsonBody,
    /// The JSON body is not valid UTF-8.
    #[error("JSON body is not valid UTF-8")]
    JsonNotUtf8,
    /// The JSON body starts with a UTF-8 byte-order mark.
    #[error("JSON body must not start with a byte-order mark")]
    JsonBom,
    /// The JSON body has bytes after its top-level value.
    #[error("JSON body has trailing bytes after the top-level value")]
    JsonTrailingBytes,
    /// The JSON body nests deeper than [`MAX_JSON_DEPTH`].
    #[error("JSON body nests deeper than the 64-level limit")]
    JsonTooDeep,
    /// A JSON object repeats a key.
    #[error("JSON object repeats a key")]
    JsonDuplicateKey,
    /// An integer JSON lexeme is outside `[i64::MIN, u64::MAX]`.
    #[error("JSON integer is outside the exact 64-bit range")]
    JsonIntegerOutOfRange,
    /// A JSON number lexeme is not a finite `f64`.
    #[error("JSON number is outside the finite f64 range")]
    JsonNumberOutOfRange,
    /// A JSON string contains a lone UTF-16 surrogate escape.
    #[error("JSON string contains a lone surrogate escape")]
    JsonLoneSurrogate,
    /// The JSON body is not valid RFC 8259.
    #[error("JSON body is not valid RFC 8259")]
    JsonSyntax,
    /// A form component contains a `%` that is not followed by two hex digits.
    #[error("form body contains a malformed percent escape")]
    FormBadEscape,
    /// A form component does not decode to UTF-8.
    #[error("form body does not decode to UTF-8")]
    FormNotUtf8,
    /// A form component decodes to a NUL byte.
    #[error("form body must not contain a NUL byte")]
    FormNul,
}

impl IngressReject {
    /// The HTTP status the scheduler answers with: `413` for a body on a route
    /// that takes none, `400` for every other rejection.
    pub fn status(&self) -> u16 {
        match self {
            Self::BodyNotAllowed => 413,
            _ => 400,
        }
    }
}

/// Splits `Content-Type` into `(lowercase_essence_capped_64b, optional_charset)`.
///
/// The essence is the part before the first `;`, trimmed and ASCII-lowercased.
/// Every parameter is dropped except `charset` (so `multipart/form-data;
/// boundary=…` yields a bare `multipart/form-data`); the first `charset` is
/// returned trimmed, unquoted and ASCII-lowercased. A missing or empty
/// `Content-Type` reports the essence `"none"`.
pub fn parse_mime(content_type: Option<&str>) -> (String, Option<String>) {
    mime::parse(content_type)
}

/// Decides, from the request head alone, how the body must be handled.
///
/// See [`BodyPlan`] for the three outcomes. The rules, in order:
///
/// 1. [`BodyCodec::Raw`] always streams ([`BodyPlan::StreamPassthrough`]),
///    whatever the method, length or encoding: the route asked for the bytes.
/// 2. A `json`/`form` route with a non-identity `Content-Encoding` is a `400`.
/// 3. A route with no ingress, [`BodyCodec::None`], or a bodyless-safe method
///    (`GET`, `HEAD`, `OPTIONS`) allows only an empty body: a positive
///    `Content-Length` is a `413`. A chunked body (`content_length` `None`)
///    becomes [`BodyPlan::AllowEmptyOnly`], and the scheduler answers `413` on
///    the first non-empty frame.
/// 4. A `json`/`form` route must carry its codec's `Content-Type` and a UTF-8
///    charset, or the head is a `400` before a byte is buffered.
///
/// `DELETE` is not bodyless-safe: a `DELETE` with bytes follows the `POST`
/// codec rules. A `DELETE` whose `Content-Length` is exactly zero never reaches
/// a codec, so it is [`BodyPlan::AllowEmptyOnly`] whatever its `Content-Type`.
pub fn plan_body(
    method: &str,
    policy: Option<IngressPolicy>,
    content_type: Option<&str>,
    content_encoding: Option<&str>,
    content_length: Option<u64>,
) -> Result<BodyPlan, IngressReject> {
    let codec = policy.map_or(BodyCodec::None, |policy| policy.body);
    if codec == BodyCodec::Raw {
        return Ok(BodyPlan::StreamPassthrough);
    }
    if matches!(codec, BodyCodec::Json | BodyCodec::Form)
        && let Some(encoding) = content_encoding
        && !is_identity(encoding)
    {
        return Err(IngressReject::ContentEncodingNotSupported {
            observed: mime::cap(encoding),
        });
    }
    if codec == BodyCodec::None || is_bodyless_safe(method) {
        return match content_length {
            Some(length) if length > 0 => Err(IngressReject::BodyNotAllowed),
            _ => Ok(BodyPlan::AllowEmptyOnly),
        };
    }
    if content_length == Some(0) && method.eq_ignore_ascii_case("DELETE") {
        // A `DELETE` with no body never reaches the codec, so its headers are
        // irrelevant (§4.2 A row 1).
        return Ok(BodyPlan::AllowEmptyOnly);
    }
    let (essence, charset) = mime::parse(content_type);
    check_charset(charset.as_deref())?;
    match codec {
        BodyCodec::Json if !mime::is_json_essence(&essence) => {
            Err(IngressReject::ContentTypeMismatch {
                expected: mime::JSON_ESSENCE,
                observed: essence,
            })
        }
        BodyCodec::Form if !mime::is_form_essence(&essence) => {
            Err(IngressReject::ContentTypeMismatch {
                expected: mime::FORM_ESSENCE,
                observed: essence,
            })
        }
        _ => Ok(BodyPlan::Buffer { codec }),
    }
}

/// Decodes, validates and canonically re-encodes a buffered `json`/`form` body.
///
/// A bodyless-safe method (`GET`, `HEAD`, `OPTIONS`) or a `DELETE` with zero
/// bytes is `Ok` with an empty body and no `Content-Type`. Any other zero-byte
/// body is `Ok` for [`BodyCodec::Form`] (an empty form is a valid form) and a
/// `400` for [`BodyCodec::Json`]. [`BodyCodec::None`] with bytes, and a
/// bodyless-safe method with bytes, are the `413` of [`IngressReject::BodyNotAllowed`].
/// [`BodyCodec::Raw`] is never transcoded by the scheduler; it is returned
/// unchanged here so a caller cannot accidentally validate it twice.
pub fn transcode_buffered(
    codec: BodyCodec,
    method: &str,
    content_type: Option<&str>,
    raw: &[u8],
) -> Result<TranscodedBody, IngressReject> {
    match codec {
        BodyCodec::Raw => Ok(TranscodedBody {
            bytes: raw.to_vec(),
            content_type: None,
        }),
        BodyCodec::None => {
            if raw.is_empty() {
                Ok(empty_body())
            } else {
                Err(IngressReject::BodyNotAllowed)
            }
        }
        BodyCodec::Json | BodyCodec::Form => {
            let bodyless = is_bodyless_safe(method);
            if raw.is_empty() && (bodyless || method.eq_ignore_ascii_case("DELETE")) {
                return Ok(empty_body());
            }
            if bodyless {
                return Err(IngressReject::BodyNotAllowed);
            }
            if codec == BodyCodec::Json {
                transcode_json(content_type, raw)
            } else {
                transcode_form(content_type, raw)
            }
        }
    }
}

/// Strips every stale framing header, then appends one canonical pair.
///
/// `Content-Length`, `Content-Type`, `Transfer-Encoding`, `Content-Encoding`
/// and `Expect` are removed case-insensitively from `headers`; then the
/// canonical `Content-Type` (when `content_type` is `Some`) and exactly one
/// `Content-Length` for `body_len` are appended. A view therefore never sees a
/// stale `HTTP_TRANSFER_ENCODING=chunked`, a duplicate length, or an `Expect`
/// the body no longer satisfies.
pub fn apply_transcoded_headers(
    headers: &mut Vec<(Vec<u8>, Vec<u8>)>,
    content_type: Option<&'static str>,
    body_len: usize,
) {
    headers.retain(|(name, _)| !is_framing_header(name));
    if let Some(content_type) = content_type {
        headers.push((b"Content-Type".to_vec(), content_type.as_bytes().to_vec()));
    }
    headers.push((
        b"Content-Length".to_vec(),
        body_len.to_string().into_bytes(),
    ));
}

/// Records one observed request body for `blastit check`.
///
/// `raw_prefix` is the first [`MAX_CHECK_SAMPLE_BYTES`] of the body and
/// `total_len` is its real length. A body longer than the cap is `truncated`,
/// and both validity flags are forced `false`. Validity is judged **against the
/// declared `Content-Type`**: `valid_json` requires a JSON essence (and a UTF-8
/// charset) plus canonical JSON bytes, and `valid_form` requires the form
/// essence plus a body that passes [`transcode_buffered`]'s form validation.
/// A codec suggestion is only safe when declaring it would accept the request
/// that was observed, so the essence gates the flag.
pub fn probe_sample(content_type: Option<&str>, raw_prefix: &[u8], total_len: u64) -> BodySample {
    let (essence, charset) = mime::parse(content_type);
    let truncated = total_len > MAX_CHECK_SAMPLE_BYTES as u64;
    let charset_ok = mime::is_utf8_charset(charset.as_deref());
    let valid_json = !truncated
        && charset_ok
        && !raw_prefix.is_empty()
        && mime::is_json_essence(&essence)
        && json::parse(raw_prefix).is_ok();
    let valid_form = !truncated
        && charset_ok
        && mime::is_form_essence(&essence)
        && form::decode(raw_prefix).is_ok();
    BodySample {
        mime: essence,
        truncated,
        valid_json,
        valid_form,
    }
}

/// Turns observed [`BodySample`]s into the codecs a route should declare.
///
/// Returns the suggested codecs in [`BodyCodec`] order (deduplicated) and
/// whether `allow_raw = true` must accompany them. A sample that validates as
/// neither JSON nor a form — including a truncated one — suggests `raw`, since
/// no structured codec can be proven to accept it. No samples suggest nothing.
pub fn suggest_fix(samples: &[BodySample]) -> (Vec<BodyCodec>, bool) {
    let mut codecs = BTreeSet::new();
    for sample in samples {
        if sample.valid_json {
            codecs.insert(BodyCodec::Json);
        } else if sample.valid_form {
            codecs.insert(BodyCodec::Form);
        } else {
            codecs.insert(BodyCodec::Raw);
        }
    }
    let allow_raw = codecs.contains(&BodyCodec::Raw);
    (codecs.into_iter().collect(), allow_raw)
}

/// Decodes one JSON body and re-emits it canonically.
fn transcode_json(content_type: Option<&str>, raw: &[u8]) -> Result<TranscodedBody, IngressReject> {
    let (essence, charset) = mime::parse(content_type);
    check_charset(charset.as_deref())?;
    if !mime::is_json_essence(&essence) {
        return Err(IngressReject::ContentTypeMismatch {
            expected: mime::JSON_ESSENCE,
            observed: essence,
        });
    }
    if raw.is_empty() {
        return Err(IngressReject::EmptyJsonBody);
    }
    let value = json::parse(raw)?;
    let mut bytes = Vec::new();
    json::emit(&value, &mut bytes);
    Ok(TranscodedBody {
        bytes,
        content_type: Some(mime::JSON_ESSENCE),
    })
}

/// Validates one form body and re-encodes it canonically.
fn transcode_form(content_type: Option<&str>, raw: &[u8]) -> Result<TranscodedBody, IngressReject> {
    let (essence, charset) = mime::parse(content_type);
    check_charset(charset.as_deref())?;
    if !mime::is_form_essence(&essence) {
        return Err(IngressReject::ContentTypeMismatch {
            expected: mime::FORM_ESSENCE,
            observed: essence,
        });
    }
    let pairs = form::decode(raw)?;
    Ok(TranscodedBody {
        bytes: form::encode(&pairs).into_bytes(),
        content_type: Some(mime::FORM_ESSENCE),
    })
}

/// Rejects a charset that is present and is not UTF-8.
fn check_charset(charset: Option<&str>) -> Result<(), IngressReject> {
    if mime::is_utf8_charset(charset) {
        Ok(())
    } else {
        Err(IngressReject::CharsetNotUtf8 {
            observed: mime::cap(charset.unwrap_or_default()),
        })
    }
}

/// Whether the method may never carry a request body.
fn is_bodyless_safe(method: &str) -> bool {
    ["GET", "HEAD", "OPTIONS"]
        .iter()
        .any(|safe| method.eq_ignore_ascii_case(safe))
}

/// Whether a `Content-Encoding` value encodes nothing.
fn is_identity(encoding: &str) -> bool {
    encoding
        .split(',')
        .map(str::trim)
        .all(|token| token.is_empty() || token.eq_ignore_ascii_case("identity"))
}

/// Whether `name` is one of the framing headers a transcode invalidates.
fn is_framing_header(name: &[u8]) -> bool {
    name.eq_ignore_ascii_case(b"content-length")
        || name.eq_ignore_ascii_case(b"content-type")
        || name.eq_ignore_ascii_case(b"transfer-encoding")
        || name.eq_ignore_ascii_case(b"content-encoding")
        || name.eq_ignore_ascii_case(b"expect")
}

/// The body a view receives when there is nothing to send.
fn empty_body() -> TranscodedBody {
    TranscodedBody {
        bytes: Vec::new(),
        content_type: None,
    }
}
