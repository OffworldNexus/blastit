//! The request's param map → the exact `PARAMS` bytes (design §4.6, §4.7).
//!
//! [`build`] is the only producer of the param stream: it pins
//! `SCRIPT_FILENAME`/`SCRIPT_NAME`/`DOCUMENT_ROOT` to [`FastCgiPaths`], derives
//! the CGI variables from [`RequestFacts`], and folds request headers into
//! `HTTP_*` params. A request byte can never name a param: the non-`HTTP_` set
//! is a closed list ([`resolve`] answers [`LockdownError::UnknownParam`]),
//! `PHP_VALUE`, `PHP_ADMIN_VALUE`, `PHP_ADMIN_*`, `auto_prepend_file`,
//! `allow_url_include`, `open_basedir`, `disable_functions`, `REDIRECT_STATUS`
//! and `HTTP_PROXY` are never emitted, and the query string stays one verbatim
//! `QUERY_STRING` value, so no `-d …` argument (CVE-2012-1823) and no best-fit
//! byte (CVE-2024-4577) can reach a param.
//!
//! Header names fold to upper case with `-`→`_`; an unrepresentable name, a
//! `Proxy`/hop-by-hop name or a value above 8 KiB is dropped and recorded in
//! [`Params::dropped`]; a colliding `HTTP_*` name keeps the first; and
//! [`MAX_PARAMS`], [`crate::MAX_PARAMS_BYTES`] and [`crate::MAX_PARAM_PAIR_BYTES`]
//! bound the set. `Content-Type`/`Content-Length` become
//! `CONTENT_TYPE`/`CONTENT_LENGTH`, never `HTTP_*`.
//!
//! Nothing here maps to an HTTP status: [`LockdownError`] is the codec-local
//! error the adapter (`MB.3.6.2.1`) maps to its own refusal variants and
//! `UpstreamError`.

use core::fmt;
use std::path::{Component, Path, PathBuf};

use crate::param::encode_pair;
use crate::record::RecordError;
use crate::{MAX_PARAM_PAIR_BYTES, MAX_PARAMS_BYTES};

/// Largest number of params one request may carry (design §4.6.2).
pub const MAX_PARAMS: usize = 128;

/// Largest client value that becomes an `HTTP_*` param or `CONTENT_TYPE`; a
/// longer value is dropped, never refused (design §4.6.2).
pub const MAX_PARAM_VALUE_BYTES: usize = 8_192;

/// The pinned paths a FastCGI route was validated against at boot.
///
/// Built by `FastCgiBuilder` (`MB.3.6.2.1`) after the R2 `openat2` check; this
/// module performs no filesystem access and never canonicalises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FastCgiPaths {
    /// Absolute `SCRIPT_FILENAME`, `openat2`-checked at boot.
    pub script_filename: PathBuf,
    /// Absolute `DOCUMENT_ROOT`: `static.root`, else the entry's parent.
    pub document_root: PathBuf,
    /// `SCRIPT_NAME`, e.g. `/index.php`.
    pub script_name: String,
}

/// The request scheme, from TLS or a trusted `X-Forwarded-Proto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Plain HTTP.
    Http,
    /// TLS-terminated HTTP.
    Https,
}

/// The request facts the builder derives params from (design §4.6.1).
#[derive(Debug, Clone, Copy)]
pub struct RequestFacts<'a> {
    /// The request method, e.g. `GET`.
    pub method: &'a [u8],
    /// The request target verbatim: path, `?` and query.
    pub target: &'a [u8],
    /// Request headers in arrival order.
    pub headers: &'a [(&'a [u8], &'a [u8])],
    /// The resolved server name.
    pub server_name: &'a str,
    /// The listening port.
    pub server_port: u16,
    /// The resolved client address.
    pub remote_addr: &'a str,
    /// The client port.
    pub remote_port: u16,
    /// The request scheme.
    pub scheme: Scheme,
    /// `CONTENT_TYPE` after ingress transcode; absent when the head has none.
    pub content_type: Option<&'a [u8]>,
    /// The exact `STDIN` byte count; absent when the head declares no body.
    pub content_length: Option<u64>,
}

/// Every failure the param builder can report (design §4.5.1).
///
/// `MB.3.6.2.1` maps these: `UnknownParam` is the internal `500` builder guard,
/// `ParamsTooLarge`/`DuplicateParam` a `400`, `Malformed` a `400`. None is an
/// HTTP status here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockdownError {
    /// A path, name or value cannot be a param byte sequence (NUL/CR/LF, a
    /// relative or `..`-bearing path, a non-token method).
    Malformed,
    /// A bare name outside the closed allowlist: a builder bug, never a client
    /// status (design §4.7 row 18).
    UnknownParam,
    /// The count, total-byte or single-pair budget was exceeded (design §4.6.2).
    ParamsTooLarge,
    /// The builder inserted one name twice: a builder bug, never a client
    /// status (design §4.6.2).
    DuplicateParam,
}

impl fmt::Display for LockdownError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "malformed request value",
            Self::UnknownParam => "unknown param name",
            Self::ParamsTooLarge => "param set exceeds its budget",
            Self::DuplicateParam => "duplicate param name",
        })
    }
}

impl std::error::Error for LockdownError {}

/// The built param set: the ordered pairs plus the names that were dropped.
///
/// [`Params::pairs`] is the exact stream, in wire order; [`Params::dropped`] is
/// what the adapter turns into `Observation::Param { decision: Dropped }`.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Params {
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    dropped: Vec<Vec<u8>>,
}

impl fmt::Debug for Params {
    /// The log-safe view: names only, never a value (a value carries cookies).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self
            .pairs
            .iter()
            .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
            .collect();
        f.debug_struct("Params")
            .field("names", &names)
            .field("dropped", &self.dropped)
            .finish()
    }
}

impl Params {
    /// An empty set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pairs: Vec::new(),
            dropped: Vec::new(),
        }
    }

    /// The ordered `(name, value)` pairs, in the order they go on the wire.
    #[must_use]
    pub fn pairs(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.pairs
    }

    /// The folded names that were dropped (never refused), in arrival order.
    #[must_use]
    pub fn dropped(&self) -> &[Vec<u8>] {
        &self.dropped
    }

    /// The value of one emitted param, if present.
    #[must_use]
    pub fn get(&self, name: &[u8]) -> Option<&[u8]> {
        self.pairs
            .iter()
            .find(|(existing, _)| existing.as_slice() == name)
            .map(|(_, value)| value.as_slice())
    }

    /// How many params are emitted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Whether no param is emitted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// The encoded `PARAMS` content length, terminator and padding excluded.
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        self.pairs
            .iter()
            .map(|(name, value)| encoded_pair_len(name, value))
            .sum()
    }

    /// Append the encoded pairs to `out` (the terminator is the caller's).
    pub fn encode_to(&self, out: &mut Vec<u8>) -> Result<(), RecordError> {
        for (name, value) in &self.pairs {
            encode_pair(name, value, out)?;
        }
        Ok(())
    }

    /// Encode the pairs into one `PARAMS` record's content.
    pub fn encode(&self) -> Result<Vec<u8>, RecordError> {
        let mut out = Vec::with_capacity(self.encoded_len());
        self.encode_to(&mut out)?;
        Ok(out)
    }

    /// The builder's only insertion point, and crate-private: [`build`] is the
    /// only producer, so no caller outside this module can name a param. The
    /// §4.6.1 reserved names, the name alphabet, the value byte rule, the
    /// duplicate guard and the pair/count/total budgets all hold, so a `Params`
    /// that exists is always wire-safe by construction.
    pub(crate) fn insert(&mut self, name: &[u8], value: &[u8]) -> Result<(), LockdownError> {
        if is_reserved_name(name) {
            return Err(LockdownError::UnknownParam);
        }
        if !is_param_name(name) || has_forbidden_value_byte(value) {
            return Err(LockdownError::Malformed);
        }
        if self.get(name).is_some() {
            return Err(LockdownError::DuplicateParam);
        }
        let pair = encoded_pair_len(name, value);
        if pair > MAX_PARAM_PAIR_BYTES
            || self.pairs.len() >= MAX_PARAMS
            || self.encoded_len().saturating_add(pair) > MAX_PARAMS_BYTES
        {
            return Err(LockdownError::ParamsTooLarge);
        }
        self.pairs.push((name.to_vec(), value.to_vec()));
        Ok(())
    }
}

/// The non-`HTTP_` param names, in the order they are emitted (design §4.6.1).
const PARAM_ORDER: [&[u8]; 18] = [
    b"SCRIPT_FILENAME",
    b"SCRIPT_NAME",
    b"PATH_INFO",
    b"DOCUMENT_ROOT",
    b"REQUEST_URI",
    b"QUERY_STRING",
    b"REQUEST_METHOD",
    b"SERVER_PROTOCOL",
    b"CONTENT_TYPE",
    b"CONTENT_LENGTH",
    b"GATEWAY_INTERFACE",
    b"SERVER_SOFTWARE",
    b"SERVER_NAME",
    b"SERVER_PORT",
    b"REMOTE_ADDR",
    b"REMOTE_PORT",
    b"REQUEST_SCHEME",
    b"HTTPS",
];

/// The closed allowlist: one bare name → its value, or `Ok(None)` when the
/// request has none (`CONTENT_TYPE`/`CONTENT_LENGTH`).
///
/// `CONTENT_TYPE` is the one derived value that comes from a request header, so
/// it also honours the §4.6.2 value rule: a value above
/// [`MAX_PARAM_VALUE_BYTES`] answers `Ok(None)` — the same call [`build`]
/// records as a drop — and no caller of this public function ever receives an
/// over-long `CONTENT_TYPE`. `CONTENT_LENGTH` is a decimal `u64` (at most 20
/// bytes), and `REQUEST_URI`/`QUERY_STRING` are target bytes, not header
/// values, so they stay verbatim.
///
/// A name outside `PARAM_ORDER` is [`LockdownError::UnknownParam`]: there is
/// no caller-named-param API, so this is a builder-programming error, never a
/// client status (design §4.7 row 18).
pub fn resolve(
    name: &[u8],
    paths: &FastCgiPaths,
    facts: &RequestFacts<'_>,
) -> Result<Option<Vec<u8>>, LockdownError> {
    let value = match name {
        b"SCRIPT_FILENAME" => path_bytes(&paths.script_filename)?,
        b"SCRIPT_NAME" => script_name_bytes(&paths.script_name)?,
        b"PATH_INFO" => path_info(facts.target)?,
        b"DOCUMENT_ROOT" => path_bytes(&paths.document_root)?,
        b"REQUEST_URI" => fact_value(facts.target)?,
        b"QUERY_STRING" => fact_value(query_string(facts.target))?,
        b"REQUEST_METHOD" => {
            if !is_token(facts.method) {
                return Err(LockdownError::Malformed);
            }
            facts.method.to_vec()
        }
        b"SERVER_PROTOCOL" => b"HTTP/1.1".to_vec(),
        b"CONTENT_TYPE" => {
            return match facts.content_type {
                Some(value) if value.len() > MAX_PARAM_VALUE_BYTES => Ok(None),
                Some(value) => Ok(Some(fact_value(value)?)),
                None => Ok(None),
            };
        }
        b"CONTENT_LENGTH" => {
            return Ok(facts.content_length.map(|len| len.to_string().into_bytes()));
        }
        b"GATEWAY_INTERFACE" => b"CGI/1.1".to_vec(),
        b"SERVER_SOFTWARE" => format!("blastit/{}", env!("CARGO_PKG_VERSION")).into_bytes(),
        b"SERVER_NAME" => non_empty_value(facts.server_name.as_bytes())?,
        b"SERVER_PORT" => facts.server_port.to_string().into_bytes(),
        b"REMOTE_ADDR" => non_empty_value(facts.remote_addr.as_bytes())?,
        b"REMOTE_PORT" => facts.remote_port.to_string().into_bytes(),
        b"REQUEST_SCHEME" => match facts.scheme {
            Scheme::Http => b"http".to_vec(),
            Scheme::Https => b"https".to_vec(),
        },
        b"HTTPS" => match facts.scheme {
            Scheme::Http => b"off".to_vec(),
            Scheme::Https => b"on".to_vec(),
        },
        _ => return Err(LockdownError::UnknownParam),
    };
    Ok(Some(value))
}

/// Build the exact param stream for one request.
///
/// Pinned and derived params come first, in `PARAM_ORDER`; then every request
/// header is folded into an `HTTP_*` param or dropped. The query string is
/// never parsed, so it can neither name a param nor become an argument.
pub fn build(paths: &FastCgiPaths, facts: &RequestFacts<'_>) -> Result<Params, LockdownError> {
    let mut params = Params::new();
    for name in PARAM_ORDER {
        match resolve(name, paths, facts)? {
            Some(value) => params.insert(name, &value)?,
            // §4.6.2: a client header value above 8 KiB is dropped and
            // recorded, never refused. `resolve` owns the cap, so its `None`
            // for a *present* `CONTENT_TYPE` is exactly the value rule.
            None if name == b"CONTENT_TYPE" && facts.content_type.is_some() => {
                params.dropped.push(b"CONTENT_TYPE".to_vec());
            }
            None => {}
        }
    }
    for (name, value) in facts.headers {
        derive_http(name, value, &mut params)?;
    }
    Ok(params)
}

/// Fold one request header into an `HTTP_*` param, or record why it was
/// dropped (design §4.6.2). A forbidden value byte is refused, never dropped.
fn derive_http(name: &[u8], value: &[u8], params: &mut Params) -> Result<(), LockdownError> {
    if name.eq_ignore_ascii_case(b"content-type") || name.eq_ignore_ascii_case(b"content-length") {
        // They are the CONTENT_TYPE/CONTENT_LENGTH params, never HTTP_*.
        return Ok(());
    }
    let folded = fold_header_name(name);
    let full = http_param_name(&folded);
    if is_hop_by_hop(name) || full == b"HTTP_PROXY" || !is_representable_header(&folded) {
        params.dropped.push(full);
        return Ok(());
    }
    if value.len() > MAX_PARAM_VALUE_BYTES {
        params.dropped.push(full);
        return Ok(());
    }
    if has_forbidden_value_byte(value) {
        return Err(LockdownError::Malformed);
    }
    if params.get(&full).is_some() {
        params.dropped.push(full);
        return Ok(());
    }
    params.insert(&full, value)
}

/// The hop-by-hop and proxy names dropped before `HTTP_*` derivation.
fn is_hop_by_hop(name: &[u8]) -> bool {
    const HOP_BY_HOP: [&[u8]; 9] = [
        b"connection",
        b"keep-alive",
        b"proxy-authenticate",
        b"proxy-authorization",
        b"proxy-connection",
        b"te",
        b"trailer",
        b"transfer-encoding",
        b"upgrade",
    ];
    HOP_BY_HOP.iter().any(|hop| name.eq_ignore_ascii_case(hop))
}

/// Upper-case a header name and turn `-` into `_` (design §4.6.2).
fn fold_header_name(name: &[u8]) -> Vec<u8> {
    name.iter()
        .map(|&byte| match byte {
            b'-' => b'_',
            b'a'..=b'z' => byte.to_ascii_uppercase(),
            other => other,
        })
        .collect()
}

/// Prefix a folded header name with `HTTP_`.
fn http_param_name(folded: &[u8]) -> Vec<u8> {
    let mut full = Vec::with_capacity(folded.len() + 5);
    full.extend_from_slice(b"HTTP_");
    full.extend_from_slice(folded);
    full
}

/// A folded header name is representable when it starts with `A-Z` and holds
/// only `A-Z`, `0-9` and `_` (design §4.6.2).
fn is_representable_header(folded: &[u8]) -> bool {
    match folded.split_first() {
        Some((first, rest)) => {
            first.is_ascii_uppercase()
                && rest
                    .iter()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
        }
        None => false,
    }
}

/// The param name alphabet: non-empty, `A-Z`, `0-9` and `_` only.
fn is_param_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// A name §4.6.1 declares "not emitted": it must never reach the wire, not even
/// through the builder's own insertion point. `PHP_ADMIN_*` is a prefix.
fn is_reserved_name(name: &[u8]) -> bool {
    const RESERVED: [&[u8]; 10] = [
        b"FCGI_ROLE",
        b"PATH_TRANSLATED",
        b"REDIRECT_STATUS",
        b"PHP_VALUE",
        b"PHP_ADMIN_VALUE",
        b"auto_prepend_file",
        b"allow_url_include",
        b"open_basedir",
        b"disable_functions",
        b"HTTP_PROXY",
    ];
    RESERVED
        .iter()
        .any(|reserved| name.eq_ignore_ascii_case(reserved))
        || name
            .get(..b"PHP_ADMIN_".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"PHP_ADMIN_"))
        || name.eq_ignore_ascii_case(b"SERVER_ADDR")
}

/// The value byte rule: no NUL, CR or LF (design §4.6.2).
fn has_forbidden_value_byte(value: &[u8]) -> bool {
    value.iter().any(|byte| matches!(byte, 0x00 | 0x0d | 0x0a))
}

/// An RFC 7230 `tchar` token, used for the method.
fn is_token(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes.iter().all(|byte| {
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
        })
}

/// A non-empty value with no forbidden byte.
fn non_empty_value(bytes: &[u8]) -> Result<Vec<u8>, LockdownError> {
    if bytes.is_empty() {
        return Err(LockdownError::Malformed);
    }
    fact_value(bytes)
}

/// A value with no forbidden byte.
fn fact_value(bytes: &[u8]) -> Result<Vec<u8>, LockdownError> {
    if has_forbidden_value_byte(bytes) {
        return Err(LockdownError::Malformed);
    }
    Ok(bytes.to_vec())
}

/// An absolute, `..`-free, `?`-free path as param bytes.
fn path_bytes(path: &Path) -> Result<Vec<u8>, LockdownError> {
    let text = path.to_str().ok_or(LockdownError::Malformed)?;
    let bytes = text.as_bytes();
    if !path.is_absolute()
        || bytes.contains(&b'?')
        || has_forbidden_value_byte(bytes)
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(LockdownError::Malformed);
    }
    Ok(bytes.to_vec())
}

/// An absolute, `..`-free, `?`-free `SCRIPT_NAME`.
fn script_name_bytes(name: &str) -> Result<Vec<u8>, LockdownError> {
    let bytes = name.as_bytes();
    if !name.starts_with('/')
        || bytes.contains(&b'?')
        || has_forbidden_value_byte(bytes)
        || Path::new(name)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(LockdownError::Malformed);
    }
    Ok(bytes.to_vec())
}

/// The target's path and query, split at the first `?`.
fn split_target(target: &[u8]) -> (&[u8], &[u8]) {
    match target.iter().position(|&byte| byte == b'?') {
        Some(at) => (
            target.get(..at).unwrap_or_default(),
            target.get(at.saturating_add(1)..).unwrap_or_default(),
        ),
        None => (target, &[]),
    }
}

/// The verbatim bytes after the first `?`; empty when there is none.
fn query_string(target: &[u8]) -> &[u8] {
    split_target(target).1
}

/// `PATH_INFO`: the request path percent-decoded exactly once, empty for `/`.
fn path_info(target: &[u8]) -> Result<Vec<u8>, LockdownError> {
    let (path, _) = split_target(target);
    if path == b"/" {
        return Ok(Vec::new());
    }
    decode_path(path)
}

/// Percent-decode a path exactly once.
///
/// `%HH` with two hex digits decodes; any other `%` is kept literally (the
/// front end refuses a malformed target before this point). A decoded NUL, CR
/// or LF is [`LockdownError::Malformed`], never a param byte.
fn decode_path(path: &[u8]) -> Result<Vec<u8>, LockdownError> {
    let mut out = Vec::with_capacity(path.len());
    let mut rest = path;
    while let Some((&first, tail)) = rest.split_first() {
        if first == b'%'
            && let Some(decoded) = tail.get(..2).and_then(hex_pair)
        {
            if has_forbidden_value_byte(&[decoded]) {
                return Err(LockdownError::Malformed);
            }
            out.push(decoded);
            rest = tail.get(2..).ok_or(LockdownError::Malformed)?;
            continue;
        }
        if has_forbidden_value_byte(&[first]) {
            return Err(LockdownError::Malformed);
        }
        out.push(first);
        rest = tail;
    }
    Ok(out)
}

/// Two hex digits as one byte.
fn hex_pair(bytes: &[u8]) -> Option<u8> {
    let [high, low] = <[u8; 2]>::try_from(bytes).ok()?;
    Some(hex_digit(high)? << 4 | hex_digit(low)?)
}

/// One hex digit.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// One pair's encoded length, its length prefixes included.
fn encoded_pair_len(name: &[u8], value: &[u8]) -> usize {
    len_prefix(name.len()) + len_prefix(value.len()) + name.len() + value.len()
}

/// The encoded length of one length prefix: one byte below 128, else four.
const fn len_prefix(len: usize) -> usize {
    if len < 0x80 { 1 } else { 4 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The insertion point is crate-private, so these boundaries are exercised
    /// here rather than in the public-API `critical_fastcgi_lockdown` suite.
    #[test]
    fn insert_enforces_the_pair_total_and_count_budgets() {
        // The single-pair boundary: 65 531 encoded bytes, prefixes included.
        let mut params = Params::new();
        assert_eq!(
            params.insert(b"A", &vec![b'v'; 65_526]),
            Err(LockdownError::ParamsTooLarge)
        );
        assert_eq!(params.insert(b"A", &vec![b'v'; 65_525]), Ok(()));
        assert_eq!(params.encoded_len(), 65_531);

        // The length-prefix boundary: 127 bytes is one prefix byte, 128 is four.
        let mut params = Params::new();
        params.insert(b"A", &[b'v'; 127]).expect("short");
        assert_eq!(params.encoded_len(), 130);
        let mut params = Params::new();
        params.insert(b"A", &[b'v'; 128]).expect("long");
        assert_eq!(params.encoded_len(), 134);

        // The total boundary: 65 535 fits, 65 536 is refused.
        let mut params = Params::new();
        params.insert(b"A", &vec![b'v'; 65_525]).expect("pair");
        params.insert(b"B", b"x").expect("4 bytes");
        assert_eq!(params.encoded_len(), 65_535);
        assert_eq!(
            params.insert(b"C", b"x"),
            Err(LockdownError::ParamsTooLarge)
        );
        let mut params = Params::new();
        params.insert(b"A", &vec![b'v'; 65_525]).expect("pair");
        assert_eq!(
            params.insert(b"B", b"xy"),
            Err(LockdownError::ParamsTooLarge)
        );

        // The count boundary: 128 fits, 129 is refused.
        let mut params = Params::new();
        for index in 0..128 {
            params
                .insert(format!("A{index}").as_bytes(), b"1")
                .expect("128 params");
        }
        assert_eq!(
            params.insert(b"LAST", b"1"),
            Err(LockdownError::ParamsTooLarge)
        );
    }

    #[test]
    fn insert_rejects_duplicates_and_malformed_names() {
        let mut params = Params::new();
        params.insert(b"A", b"1").expect("first");
        assert_eq!(
            params.insert(b"A", b"2"),
            Err(LockdownError::DuplicateParam)
        );
        assert_eq!(params.get(b"A"), Some(&b"1"[..]));
        for bad in [&b""[..], b"a", b"A-B", b"A B"] {
            assert_eq!(
                params.insert(bad, b"1"),
                Err(LockdownError::Malformed),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
        assert_eq!(params.insert(b"X", b"a\0b"), Err(LockdownError::Malformed));
    }

    /// §4.6.1's "not emitted" names are refused even from inside the crate, so
    /// a builder bug cannot smuggle one onto the wire.
    #[test]
    fn insert_refuses_reserved_names_even_inside_the_crate() {
        for name in [
            &b"PHP_VALUE"[..],
            b"PHP_ADMIN_VALUE",
            b"PHP_ADMIN_EXTRA",
            b"auto_prepend_file",
            b"allow_url_include",
            b"open_basedir",
            b"disable_functions",
            b"REDIRECT_STATUS",
            b"HTTP_PROXY",
            b"FCGI_ROLE",
            b"PATH_TRANSLATED",
            b"SERVER_ADDR",
        ] {
            let mut params = Params::new();
            assert_eq!(
                params.insert(name, b"1"),
                Err(LockdownError::UnknownParam),
                "{}",
                String::from_utf8_lossy(name)
            );
            assert!(params.is_empty());
        }
    }

    /// One set of facts that differ only in `CONTENT_TYPE`.
    fn facts_with_content_type(content_type: Option<&[u8]>) -> RequestFacts<'_> {
        RequestFacts {
            method: b"GET",
            target: b"/",
            headers: &[],
            server_name: "example.test",
            server_port: 80,
            remote_addr: "192.0.2.1",
            remote_port: 44_444,
            scheme: Scheme::Http,
            content_type,
            content_length: None,
        }
    }

    /// §4.6.2's value rule belongs to `resolve`, not only to `build`: the
    /// public lookup answers `Ok(None)` for an over-long `CONTENT_TYPE`, and
    /// `build` records that same value as a drop — once — while an 8 192-byte
    /// value is emitted unchanged.
    #[test]
    fn resolve_applies_the_value_cap_to_content_type() {
        let paths = FastCgiPaths {
            script_filename: PathBuf::from("/app/public/index.php"),
            document_root: PathBuf::from("/app/public"),
            script_name: "/index.php".to_string(),
        };
        let exact = vec![b'v'; MAX_PARAM_VALUE_BYTES];
        let over = vec![b'v'; MAX_PARAM_VALUE_BYTES + 1];

        // The boundary itself: 8 192 bytes is emitted, not dropped.
        let at_limit = facts_with_content_type(Some(&exact));
        assert_eq!(
            resolve(b"CONTENT_TYPE", &paths, &at_limit),
            Ok(Some(exact.clone()))
        );
        let params = build(&paths, &at_limit).expect("builds");
        assert_eq!(params.get(b"CONTENT_TYPE"), Some(exact.as_slice()));
        assert!(params.dropped().is_empty());

        // One byte over: the public lookup returns no value, and `build`
        // records exactly one `CONTENT_TYPE` drop.
        let over_limit = facts_with_content_type(Some(&over));
        assert_eq!(resolve(b"CONTENT_TYPE", &paths, &over_limit), Ok(None));
        let params = build(&paths, &over_limit).expect("builds");
        assert!(params.get(b"CONTENT_TYPE").is_none());
        assert_eq!(params.dropped(), [b"CONTENT_TYPE".to_vec()]);

        // An absent header is `Ok(None)` too, but is not a drop.
        let absent = facts_with_content_type(None);
        assert_eq!(resolve(b"CONTENT_TYPE", &paths, &absent), Ok(None));
        assert!(build(&paths, &absent).expect("builds").dropped().is_empty());
    }
}
