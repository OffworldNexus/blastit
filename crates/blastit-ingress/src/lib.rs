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
//! **This crate is the vocabulary, not the codec matrix yet.** It declares the
//! shared constants, [`BodyCodec`] and [`IngressPolicy`] so the manifest, the
//! scheduler and the `check` engine can agree on the types while the codec
//! matrix lands in a later leaf. Nothing here parses a body.
//!
//! Nothing in this crate may read a credential or log a body: the body sample
//! a later leaf adds carries a MIME essence and three booleans, never bytes.
//!
//! ```
//! use blastit_ingress::{BodyCodec, IngressPolicy};
//!
//! // A route that takes JSON request bodies and never streams raw ones.
//! let policy = IngressPolicy {
//!     body: BodyCodec::Json,
//!     allow_raw: false,
//! };
//! assert_eq!(
//!     serde_json::to_string(&policy).ok().as_deref(),
//!     Some(r#"{"body":"json","allow_raw":false}"#)
//! );
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

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
