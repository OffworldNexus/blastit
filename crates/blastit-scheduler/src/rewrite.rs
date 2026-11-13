//! Deciding what one request to a static route answers (`R6`, `R8`, design
//! `MB.2.5a` §4.2–§4.4).
//!
//! [`decide`] is the whole rewrite precedence once the routing table has chosen
//! the route and [`crate::static_asset`] has resolved the path. It is a total
//! function over the five [`StaticOutcome`] shapes and the request's method and
//! declared body, so no path reaches it without an answer and no two rules claim
//! one path. The order it enforces is the design's (§4.3 steps 5–11), first
//! match wins:
//!
//! 1. a file this route serves — exactly, as a directory's `index`, or as the
//!    SPA fallback — is answered by this responder, **never** by the app;
//! 2. a path with nothing at it forwards to the manifest's `app`, with the
//!    route's own `methods` and `[route.ingress]`;
//! 3. without an `app`, that miss is a `404`;
//! 4. a refusal — a malformed path (`400`), a hidden entry (`404`), a lost SPA
//!    fallback (`404`) — is final: never the app, never `index`.
//!
//! The first four steps of §4.3 (the health path, a route with no
//! `[route.static]`, the one decode, the dotfile rule) run before this module:
//! their refusals arrive here as [`StaticOutcome::Refused`] and
//! [`StaticOutcome::Hidden`], and a route with no policy never reaches it at
//! all.
//!
//! Nothing here touches a filesystem, opens a descriptor, builds a header or
//! waits for a body. `static_asset.rs` resolves (`MB.2.4.1`),
//! `static_response.rs` responds (`MB.2.4.2`) and `scheduler.rs` dispatches
//! (`MB.2.5.2`).

use crate::static_asset::{StaticFile, StaticOutcome};

/// The exact `Allow` value a static `405` carries (`R8`, design §4.4).
///
/// Exported so the one helper that writes a rejection's headers and the
/// protected suite spell it once.
pub const STATIC_ALLOW: &[u8] = b"GET, HEAD";

/// What a static route answers with (design §4.2).
///
/// [`Rewrite::Serve`] carries the one [`StaticFile`] onward: the scheduler keeps
/// it as the request's single static target until `respond` consumes it, so no
/// descriptor is ever duplicated (design §4.6).
#[derive(Debug)]
pub enum Rewrite {
    /// The request named a file this responder serves: the caller runs
    /// `respond` on it, from the head, and never waits for the body.
    Serve(StaticFile),
    /// Nothing at this path: the request goes to the manifest's `app` with the
    /// route's own `methods` and `[route.ingress]`, unchanged.
    Forward,
    /// A scheduler-written refusal, with `Cache-Control: no-store` in the one
    /// helper that writes it: `400`, `404`, `405` or `413` — the four statuses
    /// design §4.4 names, and no others.
    Reject {
        /// The status to answer with.
        status: u16,
        /// The `Allow` header value, or `None`. Only the static `405` sets it.
        allow: Option<&'static [u8]>,
    },
}

/// Decides one request, consuming the resolution's outcome (design §4.2).
///
/// * `outcome` — what `static_asset` found for this route's path; consumed, so
///   the [`StaticFile`] of a [`Rewrite::Serve`] is the only copy.
/// * `method` — the request method, compared as exact bytes: `GET` and `HEAD`
///   are the two this responder answers, and `get` is just another method
///   (design §4.4).
/// * `has_body` — whether the head *declares* a body, from [`declares_body`].
///   Nothing is read to decide.
/// * `has_app` — whether the manifest declares an `app`, i.e. whether a miss
///   may forward (`manifest.app().is_some()`).
///
/// The method gate runs before the body gate, so `POST` with a body on a path
/// with a static target is `405`, and only a `GET`/`HEAD` that declares a body
/// is `413` (design §4.4). A `File` is never handed to the app, a refusal is
/// never masked by the SPA fallback, and a miss never answers `index` on a
/// route whose `spa` is unset.
pub fn decide(outcome: StaticOutcome, method: &[u8], has_body: bool, has_app: bool) -> Rewrite {
    match outcome {
        // Steps 5–7: a file answers this path. The responder owns it, so the
        // method and body gate apply here and the app's plan is irrelevant.
        StaticOutcome::File(file) => {
            if method != b"GET" && method != b"HEAD" {
                return Rewrite::Reject {
                    status: 405,
                    allow: Some(STATIC_ALLOW),
                };
            }
            if has_body {
                return Rewrite::Reject {
                    status: 413,
                    allow: None,
                };
            }
            Rewrite::Serve(file)
        }
        // Steps 9–10: nothing at this path, so the app's own plan decides —
        // for every method and every body, untouched.
        StaticOutcome::Missing => {
            if has_app {
                Rewrite::Forward
            } else {
                Rewrite::Reject {
                    status: 404,
                    allow: None,
                }
            }
        }
        // Step 8: `spa = true` and the fallback file is missing. Final, and
        // never the app.
        StaticOutcome::SpaLost => Rewrite::Reject {
            status: 404,
            allow: None,
        },
        // Step 11: the entry exists but is not servable. Final, like every
        // other refusal, so a hidden `.env` cannot be answered by `index` or
        // the app.
        StaticOutcome::Hidden => Rewrite::Reject {
            status: 404,
            allow: None,
        },
        // Steps 3–4: the path itself was refused (`400`, design §5.2 rows
        // 1–13). Every refusal variant shares the status; the variant only says
        // why.
        StaticOutcome::Refused(_) => Rewrite::Reject {
            status: 400,
            allow: None,
        },
    }
}

/// Whether a request head *declares* a body (design §4.4, `R8`).
///
/// `true` when a `Content-Length` header is greater than zero or when any
/// `Transfer-Encoding` header is present; header names compare ASCII
/// case-insensitively. Nothing is read, buffered or transcoded to decide.
///
/// A `Content-Length` this function cannot parse as a number is treated as a
/// declared body: the front end refuses such a head before it reaches the
/// scheduler, and answering `413` is the fail-closed reading of a length that
/// might not be zero.
pub fn declares_body(headers: &[(Vec<u8>, Vec<u8>)]) -> bool {
    headers.iter().any(|(name, value)| {
        if name.eq_ignore_ascii_case(b"transfer-encoding") {
            // Any `Transfer-Encoding` — `chunked` included — means the framing
            // is in the body itself, so the head declared one.
            return true;
        }
        if !name.eq_ignore_ascii_case(b"content-length") {
            return false;
        }
        !matches!(content_length(value), Some(0))
    })
}

/// `Content-Length > 0`, or `None` when the value is not a plain decimal
/// number.
fn content_length(value: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(value).ok()?;
    let text = text.trim_matches(|byte: char| byte == ' ' || byte == '\t');
    if text.is_empty() {
        return None;
    }
    text.parse::<u64>().ok()
}
