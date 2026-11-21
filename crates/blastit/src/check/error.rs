//! Why `blastit check` could not read its observations.
//!
//! Every variant is a problem in what the caller handed over — the manifest or
//! the two JSONL streams — never something blastit could not do: no report is
//! partially produced, and [`ObsError`] names the file and line a JSONL record
//! failed on.
//!
//! No message carries a statement's literals: a refusal names the place and the
//! rule, and the statements a report prints are normalised or redacted before
//! they get here.
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
