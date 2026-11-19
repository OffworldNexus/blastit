//! The `BLASTIT_*` value parsers (§8.2).

use std::time::Duration;

use blastit_frame::{TrustedProxies, is_header_name};

use super::types::Immortalize;
use super::{ConfigError, invalid};

pub(super) fn parse_u16(name: &str, value: &str) -> Result<u16, ConfigError> {
    value
        .trim()
        .parse::<u16>()
        .map_err(|error| invalid(name, format!("expected a port: {error}")))
}

pub(super) fn parse_u32(name: &str, value: &str) -> Result<u32, ConfigError> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|error| invalid(name, format!("expected a whole number: {error}")))
}

pub(super) fn parse_i32(name: &str, value: &str) -> Result<i32, ConfigError> {
    value
        .trim()
        .parse::<i32>()
        .map_err(|error| invalid(name, format!("expected a whole number: {error}")))
}

pub(super) fn parse_u64(name: &str, value: &str) -> Result<u64, ConfigError> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|error| invalid(name, format!("expected a whole number: {error}")))
}

pub(super) fn parse_usize(name: &str, value: &str) -> Result<usize, ConfigError> {
    value
        .trim()
        .parse::<usize>()
        .map_err(|error| invalid(name, format!("expected a whole number: {error}")))
}

/// Parses a prefix length or any other small unsigned number.
pub(super) fn parse_u8(name: &str, value: &str) -> Result<u8, ConfigError> {
    value
        .trim()
        .parse::<u8>()
        .map_err(|error| invalid(name, format!("expected a whole number under 256: {error}")))
}

/// Splits a comma- or whitespace-separated list, dropping empty entries.
pub(super) fn split_list(value: &str) -> Vec<String> {
    value
        .split([',', ' ', '\t', '\n'])
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

/// `BLASTIT_CLIENT_IP_HEADER`: a header name, lower-cased.
pub(super) fn parse_client_ip_header(value: &str) -> Result<String, ConfigError> {
    let header = value.trim().to_ascii_lowercase();
    if !is_header_name(&header) {
        return Err(invalid(
            "BLASTIT_CLIENT_IP_HEADER",
            "must be a header name, without its colon".to_string(),
        ));
    }
    Ok(header)
}

/// `BLASTIT_TRUSTED_PROXIES`: IP addresses, CIDR networks or `*`.
pub(super) fn parse_trusted_proxies(value: &str) -> Result<Vec<String>, ConfigError> {
    let list = split_list(value);
    check_trusted_proxies(&list).map_err(|reason| invalid("BLASTIT_TRUSTED_PROXIES", reason))?;
    Ok(list)
}

/// Why a trusted proxy list cannot be used, in one sentence and without its
/// entries (§8.1: an error names the variable, never a value).
pub(super) fn check_trusted_proxies(list: &[String]) -> Result<(), String> {
    TrustedProxies::parse(list)
        .map(|_| ())
        .map_err(|_| TRUSTED_PROXIES_REASON.to_string())
}

/// What can be wrong with a `BLASTIT_TRUSTED_PROXIES` entry. The port is named
/// because that is the mistake an operator makes: one peer is not a network.
const TRUSTED_PROXIES_REASON: &str =
    "every entry must be an IP address, a CIDR network or `*`; a port is not a network";

/// Parses `0`/`1`/`true`/`false`/`yes`/`no`/`on`/`off` (§8.2 uses `0` and
/// `1`).
pub(super) fn parse_bool(name: &str, value: &str) -> Result<bool, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(invalid(name, format!("expected 0 or 1, not `{other}`"))),
    }
}

/// Parses `auto`, or any `parse_bool` spelling, as [`Immortalize`] (§8.2
/// writes the booleans as `1` and `0`).
pub(super) fn parse_immortalize(name: &str, value: &str) -> Result<Immortalize, ConfigError> {
    let text = value.trim();
    if text.eq_ignore_ascii_case("auto") {
        return Ok(Immortalize::Auto);
    }
    match parse_bool(name, text) {
        Ok(true) => Ok(Immortalize::On),
        Ok(false) => Ok(Immortalize::Off),
        // `parse_bool` names the spellings it knows; `auto` is the one it does
        // not, so the message here is this variable's own.
        Err(_) => Err(invalid(
            name,
            format!("expected auto, 0 or 1, not `{text}`"),
        )),
    }
}

/// Parses `10s`, `500ms`, `1.5m` or a bare number of seconds.
pub(super) fn parse_duration(name: &str, value: &str) -> Result<Duration, ConfigError> {
    let text = value.trim();
    let (number, scale) = if let Some(rest) = text.strip_suffix("ms") {
        (rest, 0.001_f64)
    } else if let Some(rest) = text.strip_suffix('s') {
        (rest, 1.0)
    } else if let Some(rest) = text.strip_suffix('m') {
        (rest, 60.0)
    } else if let Some(rest) = text.strip_suffix('h') {
        (rest, 3600.0)
    } else {
        (text, 1.0)
    };
    let number = number
        .trim()
        .parse::<f64>()
        .map_err(|error| invalid(name, format!("expected a duration: {error}")))?;
    if !number.is_finite() || number < 0.0 {
        return Err(invalid(
            name,
            "expected a duration that is zero or more".to_string(),
        ));
    }
    seconds(name, number * scale)
}

/// A `Duration` from a number of seconds, refusing values that cannot be one.
///
/// `Duration::from_secs_f64` panics on a value it cannot represent, and the
/// value comes from outside the process, so it is bounded here instead.
pub(super) fn seconds(name: &str, seconds: f64) -> Result<Duration, ConfigError> {
    const LIMIT: f64 = 1e15;
    if !seconds.is_finite() || !(0.0..=LIMIT).contains(&seconds) {
        return Err(invalid(
            name,
            "expected a duration under about 31 million years".to_string(),
        ));
    }
    Ok(Duration::from_secs_f64(seconds))
}

/// Parses `64 KiB`, `100 MiB`, `1GB` or a bare byte count.
pub(super) fn parse_size(name: &str, value: &str) -> Result<u64, ConfigError> {
    let text = value.trim();
    let lower = text.to_ascii_lowercase();
    let (number, scale): (&str, f64) = if let Some(rest) = lower.strip_suffix("kib") {
        (rest, 1024.0)
    } else if let Some(rest) = lower.strip_suffix("mib") {
        (rest, 1024.0 * 1024.0)
    } else if let Some(rest) = lower.strip_suffix("gib") {
        (rest, 1024.0 * 1024.0 * 1024.0)
    } else if let Some(rest) = lower.strip_suffix("kb") {
        (rest, 1000.0)
    } else if let Some(rest) = lower.strip_suffix("mb") {
        (rest, 1_000_000.0)
    } else if let Some(rest) = lower.strip_suffix("gb") {
        (rest, 1_000_000_000.0)
    } else if let Some(rest) = lower.strip_suffix('b') {
        (rest, 1.0)
    } else {
        (lower.as_str(), 1.0)
    };
    // The slice above always points into `lower`, so indexing back into `text`
    // is not needed: parse the lower-cased remainder directly.
    let number = number
        .trim()
        .parse::<f64>()
        .map_err(|error| invalid(name, format!("expected a size: {error}")))?;
    if !number.is_finite() || number < 0.0 {
        return Err(invalid(
            name,
            "expected a size that is zero or more".to_string(),
        ));
    }
    let bytes = number * scale;
    if bytes > u64::MAX as f64 {
        return Err(invalid(name, "the size is too large".to_string()));
    }
    Ok(bytes as u64)
}

/// [`parse_size`] narrowed to `usize`, for byte counts held in memory.
pub(super) fn parse_size_usize(name: &str, value: &str) -> Result<usize, ConfigError> {
    let bytes = parse_size(name, value)?;
    usize::try_from(bytes)
        .map_err(|_| invalid(name, "the size does not fit this platform".to_string()))
}

/// Parses `85%` or a bare number, as a percentage.
pub(super) fn parse_percent(name: &str, value: &str) -> Result<f64, ConfigError> {
    let text = value.trim();
    let text = text.strip_suffix('%').unwrap_or(text).trim();
    text.parse::<f64>()
        .map_err(|error| invalid(name, format!("expected a percentage: {error}")))
}
