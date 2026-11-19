//! Why an audit could not run, and why an observation set could not be read.
//!
//! Every variant is a problem in what the caller handed over — the manifest, the
//! observations, a proxy name — never something blastit could not do: no report
//! is partially produced. [`ObsError`] is `blastit check`'s half (it names the
//! file and line a JSONL record failed on), [`AuditError`] the plugin report's.
//!
//! No message carries a statement's literals: a refusal names the place
//! (`requests[3].egress[0]`) and the rule (`port: expected an integer between 0
//! and 65535`), and the statements a report prints are normalised or redacted
//! before they get here.

use std::fmt;
/// Why the observations (or the manifest handed with them) could not be read.
///
/// Every variant is a problem in the input, never something blastit could not
/// do: no report is partially produced.
#[derive(Debug, thiserror::Error)]
pub enum ObsError {
    /// One JSONL line is not the document this schema names.
    #[error("{file}: line {line}: {reason}")]
    Malformed {
        /// The file the line came from, as the caller names it.
        file: String,
        /// One-based line number.
        line: usize,
        /// Why the line did not parse.
        reason: String,
    },
    /// The document is a schema this build does not read.
    #[error("schema_version {found} is not {expected}")]
    SchemaVersion {
        /// The version found.
        found: u64,
        /// The version this build reads.
        expected: u64,
    },
    /// The manifest itself is invalid.
    #[error("the manifest is invalid: {0}")]
    Manifest(String),
    /// The report could not be rendered.
    #[error("the report could not be rendered: {0}")]
    Report(String),
}

/// Why an audit could not run.
///
/// Every variant is a problem in what the caller handed over — the manifest or
/// the observations — never something blastit could not do.
#[derive(Debug)]
pub enum AuditError {
    /// The manifest is not a valid blastit manifest.
    Manifest(String),
    /// The observations are not the document this module reads.
    Observations(String),
    /// A statement names a proxy the manifest never declares.
    UnknownProxy(String),
    /// A statement names a proxy that is not a `postgresql` one.
    NotPostgresql(String),
    /// The manifest declares no `postgresql` proxy at all, so nothing can be
    /// granted and no statement can be attributed.
    NoProxy,
    /// A statement names no proxy and the manifest declares several, so there is
    /// no way to tell which grant applies.
    AmbiguousProxy(Vec<String>),
    /// The report could not be rendered as JSON. It always can in practice.
    Report(String),
}

impl AuditError {
    /// Wraps a rendered [`blastit_manifest::ManifestErrors`].
    pub(crate) fn manifest(errors: &impl fmt::Display) -> Self {
        Self::Manifest(errors.to_string())
    }
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(message) => write!(f, "the manifest is invalid: {message}"),
            Self::Observations(message) => write!(f, "the observations are invalid: {message}"),
            Self::UnknownProxy(proxy) => write!(
                f,
                "the observations name the proxy {proxy:?}, which the manifest does not declare"
            ),
            Self::NotPostgresql(proxy) => write!(
                f,
                "the observations name the proxy {proxy:?}, which is not a postgresql proxy"
            ),
            Self::NoProxy => f.write_str(
                "the manifest declares no postgresql proxy, so no observed statement can be checked",
            ),
            Self::AmbiguousProxy(proxies) => write!(
                f,
                "the manifest declares {} postgresql proxies ({}), and the observations do not say which one a statement used",
                proxies.len(),
                proxies.join(", ")
            ),
            Self::Report(message) => write!(f, "the report could not be rendered: {message}"),
        }
    }
}

impl std::error::Error for AuditError {}
