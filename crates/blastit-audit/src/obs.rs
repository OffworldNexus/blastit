//! The two streams one `blastit check` run produces, and the schema that ties
//! them together.
//!
//! `http_obs.jsonl` is one [`HttpObsLine`] per line — what the WSGI gate saw,
//! with a body sample that never carries the bytes — and `sql_obs.jsonl` one
//! `blastit_dbproxy::WireSqlObs` per line: the target it belonged to, the proxy
//! it ran on and whether the route's grants would have allowed it.
//! [`CheckObsV1`] refuses any `schema_version` but [`SCHEMA_VERSION`], so a
//! document from a newer gate is an error rather than a half-read run.

use blastit_dbproxy::WireSqlObs;
use blastit_ingress::BodySample;
use serde::{Deserialize, Serialize};

use crate::error::ObsError;
/// The only observations schema this build reads.
pub const SCHEMA_VERSION: u64 = 1;

/// One observed HTTP request (`http_obs.jsonl`).
///
/// `sample` is what the Python gate probed from the body: at most 64 KiB, and
/// never the bytes themselves. It is optional because a request with no body
/// has nothing to sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpObsLine {
    /// The request method, as the app saw it.
    pub method: String,
    /// `PATH_INFO`, the string the scheduler routed on.
    pub path: String,
    /// The raw `Content-Type` header, if any.
    #[serde(default)]
    pub content_type: Option<String>,
    /// How many body bytes the app was handed.
    #[serde(default)]
    pub body_len: u64,
    /// The bounded body probe, if the gate took one.
    #[serde(default)]
    pub sample: Option<BodySample>,
}

/// The complete observations of one `blastit check`/`--learn` run.
///
/// The schema version is part of the value, not of the files: the orchestrator
/// knows which plugin wrote them and states it explicitly. [`CheckObsV1::new`]
/// refuses anything but [`SCHEMA_VERSION`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckObsV1 {
    schema_version: u64,
    http: Vec<HttpObsLine>,
    sql: Vec<WireSqlObs>,
}

impl CheckObsV1 {
    /// A v1 observations set.
    ///
    /// # Errors
    ///
    /// [`ObsError::SchemaVersion`] when `schema_version != 1`.
    pub fn new(
        schema_version: u64,
        http: Vec<HttpObsLine>,
        sql: Vec<WireSqlObs>,
    ) -> Result<Self, ObsError> {
        if schema_version != SCHEMA_VERSION {
            return Err(ObsError::SchemaVersion {
                found: schema_version,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(Self {
            schema_version,
            http,
            sql,
        })
    }

    /// Reads both JSONL streams, naming the files in any error.
    ///
    /// # Errors
    ///
    /// [`ObsError::Malformed`] for the first line that is not a v1 record.
    pub fn from_jsonl(http_jsonl: &str, sql_jsonl: &str) -> Result<Self, ObsError> {
        Self::new(
            SCHEMA_VERSION,
            parse_http_obs("http_obs.jsonl", http_jsonl)?,
            parse_sql_obs("sql_obs.jsonl", sql_jsonl)?,
        )
    }

    /// The schema version, always [`SCHEMA_VERSION`] for a value that exists.
    pub fn schema_version(&self) -> u64 {
        self.schema_version
    }

    /// Every observed HTTP request.
    pub fn http(&self) -> &[HttpObsLine] {
        &self.http
    }

    /// Every observed statement.
    pub fn sql(&self) -> &[WireSqlObs] {
        &self.sql
    }
}

/// Reads `http_obs.jsonl`. `file` is what an error names.
///
/// # Errors
///
/// [`ObsError::Malformed`] naming `file`, the line and the reason.
pub fn parse_http_obs(file: &str, text: &str) -> Result<Vec<HttpObsLine>, ObsError> {
    parse_lines(file, text)
}

/// Reads `sql_obs.jsonl`. `file` is what an error names.
///
/// # Errors
///
/// [`ObsError::Malformed`] naming `file`, the line and the reason.
pub fn parse_sql_obs(file: &str, text: &str) -> Result<Vec<WireSqlObs>, ObsError> {
    parse_lines(file, text)
}

/// Reads one JSON document per non-empty line.
fn parse_lines<T: serde::de::DeserializeOwned>(file: &str, text: &str) -> Result<Vec<T>, ObsError> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str(line).map_err(|error| ObsError::Malformed {
            file: file.to_string(),
            line: index + 1,
            reason: error.to_string(),
        })?;
        out.push(value);
    }
    Ok(out)
}
