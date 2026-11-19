//! The observations document a pytest session writes, and its reader.
//!
//! `method` and `path_info` are what the app saw — the same pair the scheduler
//! routed on — and `sql` is what counts: it is normalised before it is compared
//! to a grant, exactly like the text on the wire. `connection_setup` holds what
//! psycopg ran as a connection opened (no request owns it), `unrecognised` what
//! the plugin could not read, and `warnings` what the plugin itself could not
//! do.
//!
//! Unknown keys are ignored, so a plugin and an extension from different
//! versions still work together; the keys that carry meaning must be there and
//! must have the documented type.

use serde_json::{Map, Value};

use crate::VERSION;
use crate::error::AuditError;
/// One request as the plugin recorded it.
pub(crate) struct ObservedRequest {
    pub(crate) method: String,
    pub(crate) path_info: String,
    pub(crate) nodeid: Option<String>,
    pub(crate) transaction_control: bool,
    pub(crate) statements: Vec<ObservedStatement>,
    /// The outbound connections the request tried to open (§7.3).
    pub(crate) egress: Vec<ObservedEgress>,
    /// True when the request is known only from a *live server's* log (D3c):
    /// `"source": "live"` in the observations.
    ///
    /// A log records the requests that were served and the statements that were
    /// **denied** — never the statements that ran. So a route only such a log
    /// saw can be told what it is missing, and nothing about which of its grants
    /// matched, which is what the report refuses to guess at.
    pub(crate) live: bool,
}

/// One outbound connection attempt.
pub(crate) struct ObservedEgress {
    /// `AF_INET`, `AF_INET6`… as the socket named it.
    pub(crate) family: String,
    pub(crate) host: String,
    pub(crate) port: u16,
}

/// One statement, as it went on the wire.
pub(crate) struct ObservedStatement {
    /// The proxy it belongs to, when the plugin could tell.
    pub(crate) proxy: Option<String>,
    /// The wire SQL: `$n` placeholders, as psycopg sends it with server-side
    /// binding.
    pub(crate) sql: String,
}

impl ObservedEgress {
    /// What the attempt is keyed by: one destination, one line.
    pub(crate) fn key(&self) -> (String, u16) {
        (self.host.clone(), self.port)
    }
}

/// A statement psycopg itself ran: what goes on the wire as a connection is
/// established, before (or without) any Django cursor of a request.
pub(crate) struct ObservedSetup {
    /// The proxy it belongs to, when the plugin could tell.
    pub(crate) proxy: Option<String>,
    /// The wire SQL.
    pub(crate) sql: String,
    /// How many times it ran. Defaults to one.
    pub(crate) count: u64,
}

/// A statement the plugin could not read — a t-string `Template`, which has no
/// text before it is rendered with its values — counted rather than dropped.
pub(crate) struct ObservedUnrecognised {
    /// The alias it came from, when the plugin could tell.
    pub(crate) alias: Option<String>,
    /// What the plugin saw, as a Python repr, for a human to recognise.
    pub(crate) query: String,
    /// How many times it ran. Defaults to one.
    pub(crate) count: u64,
}

/// Everything one pytest session recorded: the requests, the statements psycopg
/// ran on its own to set a connection up, and what could not be read.
pub(crate) struct Observations {
    pub(crate) requests: Vec<ObservedRequest>,
    pub(crate) connection_setup: Vec<ObservedSetup>,
    pub(crate) unrecognised: Vec<ObservedUnrecognised>,
    /// What the plugin itself could not do — a connection that would inline its
    /// parameters, a worker report it did not understand. The report carries them
    /// so a reader of the JSON sees them too.
    pub(crate) warnings: Vec<String>,
}

/// Reads the observations document the plugin writes.
///
/// Unknown keys are ignored, so a plugin and an extension from different
/// versions still work together; the keys that carry meaning must be there and
/// must have the documented type.
pub(crate) fn parse_observations(source: &str) -> Result<Observations, AuditError> {
    let value: Value = serde_json::from_str(source)
        .map_err(|error| AuditError::Observations(format!("not valid JSON: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| bad("the observations", "expected a JSON object"))?;
    if let Some(version) = object.get("version")
        && version.as_u64() != Some(VERSION)
    {
        return Err(bad(
            "version",
            &format!("expected {VERSION}, found {}", compact(version)),
        ));
    }
    let requests = object
        .get("requests")
        .ok_or_else(|| bad("the observations", "missing `requests`"))?
        .as_array()
        .ok_or_else(|| bad("requests", "expected an array"))?;

    let mut out = Vec::with_capacity(requests.len());
    for (index, request) in requests.iter().enumerate() {
        out.push(parse_request(request, index)?);
    }

    let setup = match object.get("connection_setup") {
        None | Some(Value::Null) => Vec::new(),
        Some(setup) => {
            let list = setup
                .as_array()
                .ok_or_else(|| bad("connection_setup", "expected an array"))?;
            let mut out = Vec::with_capacity(list.len());
            for (index, statement) in list.iter().enumerate() {
                out.push(parse_setup(statement, index)?);
            }
            out
        }
    };

    let unrecognised = match object.get("unrecognised") {
        None | Some(Value::Null) => Vec::new(),
        Some(entries) => {
            let list = entries
                .as_array()
                .ok_or_else(|| bad("unrecognised", "expected an array"))?;
            let mut out = Vec::with_capacity(list.len());
            for (index, entry) in list.iter().enumerate() {
                out.push(parse_unrecognised(entry, index)?);
            }
            out
        }
    };

    let warnings = match object.get("warnings") {
        None | Some(Value::Null) => Vec::new(),
        Some(entries) => {
            let list = entries
                .as_array()
                .ok_or_else(|| bad("warnings", "expected an array"))?;
            let mut out = Vec::with_capacity(list.len());
            for (index, entry) in list.iter().enumerate() {
                out.push(
                    entry
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| bad(&format!("warnings[{index}]"), "expected a string"))?,
                );
            }
            out
        }
    };

    Ok(Observations {
        requests: out,
        connection_setup: setup,
        unrecognised,
        warnings,
    })
}

/// Reads one `unrecognised[]` entry.
fn parse_unrecognised(value: &Value, index: usize) -> Result<ObservedUnrecognised, AuditError> {
    let at = format!("unrecognised[{index}]");
    let object = value
        .as_object()
        .ok_or_else(|| bad(&at, "expected an object"))?;
    let count = match object.get("count") {
        None | Some(Value::Null) => 1,
        Some(count) => count
            .as_u64()
            .ok_or_else(|| bad(&at, "count: expected a non-negative integer"))?,
    };
    Ok(ObservedUnrecognised {
        alias: optional_string(object, "alias", &at)?,
        query: required_string(object, "query", &at)?,
        count,
    })
}

/// Reads one `connection_setup[]` entry.
fn parse_setup(value: &Value, index: usize) -> Result<ObservedSetup, AuditError> {
    let at = format!("connection_setup[{index}]");
    let object = value
        .as_object()
        .ok_or_else(|| bad(&at, "expected an object"))?;
    let count = match object.get("count") {
        None | Some(Value::Null) => 1,
        Some(count) => count
            .as_u64()
            .ok_or_else(|| bad(&at, "count: expected a non-negative integer"))?,
    };
    Ok(ObservedSetup {
        proxy: optional_string(object, "proxy", &at)?,
        sql: required_string(object, "sql", &at)?,
        count,
    })
}

/// Reads one `requests[]` entry.
fn parse_request(value: &Value, index: usize) -> Result<ObservedRequest, AuditError> {
    let at = format!("requests[{index}]");
    let object = value
        .as_object()
        .ok_or_else(|| bad(&at, "expected an object"))?;
    let method = required_string(object, "method", &at)?;
    let path_info = required_string(object, "path_info", &at)?;
    let nodeid = optional_string(object, "nodeid", &at)?;
    let transaction_control = optional_bool(object, "transaction_control", &at)?;
    // Where the request is known from: a live server's log (`"live"`), or this
    // process's own Django (anything else, absence included).
    let live = optional_string(object, "source", &at)?.as_deref() == Some("live");
    let statements = match object.get("statements") {
        None => Vec::new(),
        Some(statements) => {
            let list = statements
                .as_array()
                .ok_or_else(|| bad(&at, "statements: expected an array"))?;
            let mut out = Vec::with_capacity(list.len());
            for (index, statement) in list.iter().enumerate() {
                let at = format!("{at}.statements[{index}]");
                let object = statement
                    .as_object()
                    .ok_or_else(|| bad(&at, "expected an object"))?;
                out.push(ObservedStatement {
                    proxy: optional_string(object, "proxy", &at)?,
                    sql: required_string(object, "sql", &at)?,
                });
            }
            out
        }
    };
    let egress = match object.get("egress") {
        None | Some(Value::Null) => Vec::new(),
        Some(attempts) => {
            let list = attempts
                .as_array()
                .ok_or_else(|| bad(&at, "egress: expected an array"))?;
            let mut out = Vec::with_capacity(list.len());
            for (index, attempt) in list.iter().enumerate() {
                out.push(parse_egress(attempt, index, &at)?);
            }
            out
        }
    };
    Ok(ObservedRequest {
        method,
        path_info,
        nodeid,
        transaction_control,
        statements,
        egress,
        live,
    })
}

/// Reads one `egress[]` entry of a request.
fn parse_egress(value: &Value, index: usize, at: &str) -> Result<ObservedEgress, AuditError> {
    let at = format!("{at}.egress[{index}]");
    let object = value
        .as_object()
        .ok_or_else(|| bad(&at, "expected an object"))?;
    let host = required_string(object, "host", &at)?;
    let port = match object.get("port") {
        None => return Err(bad(&at, "missing `port`")),
        Some(port) => port
            .as_u64()
            .filter(|port| *port <= u64::from(u16::MAX))
            .ok_or_else(|| bad(&at, "port: expected an integer between 0 and 65535"))?,
    };
    Ok(ObservedEgress {
        family: optional_string(object, "family", &at)?.unwrap_or_else(|| "AF_INET".to_string()),
        host,
        port: port as u16,
    })
}

/// A required string field.
fn required_string(object: &Map<String, Value>, key: &str, at: &str) -> Result<String, AuditError> {
    match object.get(key) {
        None => Err(bad(at, &format!("missing `{key}`"))),
        Some(value) => value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| bad(at, &format!("{key}: expected a string"))),
    }
}

/// An optional string field; `null` and absence are both `None`.
fn optional_string(
    object: &Map<String, Value>,
    key: &str,
    at: &str,
) -> Result<Option<String>, AuditError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|text| Some(text.to_string()))
            .ok_or_else(|| bad(at, &format!("{key}: expected a string or null"))),
    }
}

/// An optional boolean field, `false` when absent.
fn optional_bool(object: &Map<String, Value>, key: &str, at: &str) -> Result<bool, AuditError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| bad(at, &format!("{key}: expected a boolean"))),
    }
}

/// One observations problem, naming where it is.
fn bad(at: &str, what: &str) -> AuditError {
    AuditError::Observations(format!("{at}: {what}"))
}

/// Renders a JSON value on one line for an error message.
fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unrenderable>".to_string())
}
