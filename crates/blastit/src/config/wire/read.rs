//! The typed readers behind the fields of [`super::WireConfig`].
//!
//! Each reader reproduces one hand-written accessor: the same accepted JSON
//! shapes, the same reason it reported, and the field's own name in front of it,
//! so a document stage 0 did not write still prints exactly the sentence it
//! always did.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::time::Duration;

use blastit_events::Level;
use serde::Deserialize;
use serde::de::{Deserializer, Error as _};
use serde_json::Value;

use super::super::parse::seconds;
use super::super::types::{ConfigError, Immortalize, LogFormat, SpawnMode, Warning};
use super::field::JsonField;

/// The `<json field>: <reason>` sentence a field reports in its own words.
fn named<F: JsonField>(reason: impl std::fmt::Display) -> String {
    format!("{}: {reason}", F::NAME)
}

/// A [`ConfigError`]'s own one-line sentence, for a `serde` error.
fn error_text(error: ConfigError) -> String {
    match error {
        ConfigError::Document { field, reason } => format!("{field}: {reason}"),
        ConfigError::Invalid { variable, reason } => format!("{variable}: {reason}"),
        other => other.to_string(),
    }
}

/// A required string.
pub(super) struct Text<F: JsonField>(pub String, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for Text<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .map(|text| Self(text, PhantomData))
            .map_err(|_| D::Error::custom(named::<F>("must be a string")))
    }
}

/// A string that may be `null`, but must be there.
///
/// Fields that predate the first documents carry `#[serde(default)]` beside it,
/// which is what makes a *missing* key the same as `null` for those three —
/// `libphp`, `php_shim_dir` and D6's `privilege` — exactly what the accessors
/// they replaced did.
pub(super) struct OptText<F: JsonField>(pub Option<String>, PhantomData<F>);

impl<F: JsonField> Default for OptText<F> {
    fn default() -> Self {
        Self(None, PhantomData)
    }
}

impl<'de, F: JsonField> Deserialize<'de> for OptText<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer)
            .map(|text| Self(text, PhantomData))
            .map_err(|_| D::Error::custom(named::<F>("must be a string or null")))
    }
}

/// A non-negative whole number as `u64`.
pub(super) struct WireU64<F: JsonField>(pub u64, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for WireU64<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        u64::deserialize(deserializer)
            .map(|raw| Self(raw, PhantomData))
            .map_err(|_| D::Error::custom(named::<F>("must be a non-negative whole number")))
    }
}

/// A whole number that has to fit a narrower width.
macro_rules! narrower {
    ($name:ident, $ty:ty, $reason:literal) => {
        pub(super) struct $name<F: JsonField>(pub $ty, PhantomData<F>);

        impl<'de, F: JsonField> Deserialize<'de> for $name<F> {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let raw = u64::deserialize(deserializer).map_err(|_| {
                    D::Error::custom(named::<F>("must be a non-negative whole number"))
                })?;
                <$ty>::try_from(raw)
                    .map(|value| Self(value, PhantomData))
                    .map_err(|_| D::Error::custom(named::<F>($reason)))
            }
        }
    };
}

narrower!(WireU16, u16, "must be a port number");
narrower!(WireU32, u32, "does not fit in 32 bits");
narrower!(WireU8, u8, "does not fit in a byte");
narrower!(WireUsize, usize, "does not fit this platform");

/// A whole number that may be `null`, but must be there.
pub(super) struct OptU32<F: JsonField>(pub Option<u32>, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for OptU32<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Option::<u64>::deserialize(deserializer) {
            Ok(None) => Ok(Self(None, PhantomData)),
            Ok(Some(raw)) => u32::try_from(raw)
                .map(|value| Self(Some(value), PhantomData))
                .map_err(|_| D::Error::custom(named::<F>("does not fit in 32 bits"))),
            Err(_) => Err(D::Error::custom(named::<F>(
                "must be a non-negative whole number",
            ))),
        }
    }
}

/// A signed whole number, for the listen backlog.
pub(super) struct WireI32<F: JsonField>(pub i32, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for WireI32<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = i64::deserialize(deserializer)
            .map_err(|_| D::Error::custom(named::<F>("must be a whole number")))?;
        i32::try_from(raw)
            .map(|value| Self(value, PhantomData))
            .map_err(|_| D::Error::custom(named::<F>("does not fit in 32 bits")))
    }
}

/// A number.
pub(super) struct Float<F: JsonField>(pub f64, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for Float<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        f64::deserialize(deserializer)
            .map(|number| Self(number, PhantomData))
            .map_err(|_| D::Error::custom(named::<F>("must be a number")))
    }
}

/// A duration carried as a number of milliseconds, sub-second values included.
pub(super) struct Ms<F: JsonField>(pub Duration, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for Ms<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = f64::deserialize(deserializer)
            .map_err(|_| D::Error::custom(named::<F>("must be a number")))?;
        if !millis.is_finite() || millis < 0.0 {
            return Err(D::Error::custom(named::<F>(
                "must be a number of milliseconds that is zero or more",
            )));
        }
        seconds(F::NAME, millis / 1000.0)
            .map(|duration| Self(duration, PhantomData))
            .map_err(|_| D::Error::custom(named::<F>("is out of range")))
    }
}

/// `true` or `false`.
pub(super) struct Bool<F: JsonField>(pub bool, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for Bool<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        bool::deserialize(deserializer)
            .map(|value| Self(value, PhantomData))
            .map_err(|_| D::Error::custom(named::<F>("must be true or false")))
    }
}

/// A boolean that may be absent, or `null`.
pub(super) struct OptBool<F: JsonField>(pub Option<bool>, PhantomData<F>);

impl<F: JsonField> Default for OptBool<F> {
    fn default() -> Self {
        Self(None, PhantomData)
    }
}

impl<'de, F: JsonField> Deserialize<'de> for OptBool<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<bool>::deserialize(deserializer)
            .map(|value| Self(value, PhantomData))
            .map_err(|_| D::Error::custom(named::<F>("must be true or false")))
    }
}

/// An array of strings.
pub(super) struct Names<F: JsonField>(pub Vec<String>, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for Names<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        read_names(&value)
            .map(|names| Self(names, PhantomData))
            .map_err(|reason| D::Error::custom(named::<F>(reason)))
    }
}

/// An array of strings, or an empty list when absent or `null`.
pub(super) struct OptNames<F: JsonField>(pub Vec<String>, PhantomData<F>);

impl<F: JsonField> Default for OptNames<F> {
    fn default() -> Self {
        Self(Vec::new(), PhantomData)
    }
}

impl<'de, F: JsonField> Deserialize<'de> for OptNames<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if value.is_null() {
            return Ok(Self(Vec::new(), PhantomData));
        }
        read_names(&value)
            .map(|names| Self(names, PhantomData))
            .map_err(|reason| D::Error::custom(named::<F>(reason)))
    }
}

fn read_names(value: &Value) -> Result<Vec<String>, String> {
    let array = value
        .as_array()
        .ok_or_else(|| "must be an array".to_string())?;
    let mut out = Vec::with_capacity(array.len());
    for entry in array {
        let entry = entry
            .as_str()
            .ok_or_else(|| "every entry must be a string".to_string())?;
        out.push(entry.to_string());
    }
    Ok(out)
}

/// An object whose every value is an array of strings.
pub(super) struct NameMap<F: JsonField>(pub BTreeMap<String, Vec<String>>, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for NameMap<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom(named::<F>("must be an object")))?;
        let mut out = BTreeMap::new();
        for (key, entry) in object {
            let array = entry
                .as_array()
                .ok_or_else(|| D::Error::custom(named::<F>("every value must be an array")))?;
            let mut names = Vec::with_capacity(array.len());
            for item in array {
                let item = item
                    .as_str()
                    .ok_or_else(|| D::Error::custom(named::<F>("every entry must be a string")))?;
                names.push(item.to_string());
            }
            out.insert(key.clone(), names);
        }
        Ok(Self(out, PhantomData))
    }
}

/// An object of `u32` caps, for `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS`.
pub(super) struct ConnCaps<F: JsonField>(pub BTreeMap<String, u32>, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for ConnCaps<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom(named::<F>("must be an object")))?;
        let mut out = BTreeMap::new();
        for (name, cap) in object {
            let cap = cap
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| D::Error::custom(named::<F>("every value must be a u32")))?;
            out.insert(name.clone(), cap);
        }
        Ok(Self(out, PhantomData))
    }
}

/// An array of strings, each one byte per character (`latin1`).
pub(super) struct Bytes<F: JsonField>(pub Vec<Vec<u8>>, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for Bytes<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let array = value
            .as_array()
            .ok_or_else(|| D::Error::custom(named::<F>("must be an array")))?;
        let mut out = Vec::with_capacity(array.len());
        for entry in array {
            let text = entry
                .as_str()
                .ok_or_else(|| D::Error::custom(named::<F>("every entry must be a string")))?;
            out.push(unlatin1::<F>(text).map_err(|reason| D::Error::custom(named::<F>(reason)))?);
        }
        Ok(Self(out, PhantomData))
    }
}

/// A `latin1` byte string that may be `null`.
pub(super) struct OptBytes<F: JsonField>(pub Option<Vec<u8>>, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for OptBytes<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Option::<String>::deserialize(deserializer) {
            Ok(None) => Ok(Self(None, PhantomData)),
            Ok(Some(text)) => unlatin1::<F>(&text)
                .map(|bytes| Self(Some(bytes), PhantomData))
                .map_err(|reason| D::Error::custom(named::<F>(reason))),
            Err(_) => Err(D::Error::custom(named::<F>("must be a string or null"))),
        }
    }
}

/// The inverse of the document's writer: one byte per `char`.
fn unlatin1<F: JsonField>(value: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(value.len());
    for character in value.chars() {
        let byte = u8::try_from(u32::from(character)).map_err(|_| {
            named::<F>(format!(
                "`{character}` is not a byte (the document carries latin-1)"
            ))
        })?;
        bytes.push(byte);
    }
    Ok(bytes)
}

/// `BLASTIT_LOG_FORMAT`.
pub(super) struct LogFmt<F: JsonField>(pub LogFormat, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for LogFmt<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)
            .map_err(|_| D::Error::custom(named::<F>("must be a string")))?;
        match text.as_str() {
            "json" => Ok(Self(LogFormat::Json, PhantomData)),
            "pretty" => Ok(Self(LogFormat::Pretty, PhantomData)),
            other => Err(D::Error::custom(named::<F>(format!(
                "unknown format `{other}`"
            )))),
        }
    }
}

/// `BLASTIT_VIEW_SPAWN`.
pub(super) struct Spawn<F: JsonField>(pub SpawnMode, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for Spawn<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)
            .map_err(|_| D::Error::custom(named::<F>("must be a string")))?;
        match text.as_str() {
            "lazy" => Ok(Self(SpawnMode::Lazy, PhantomData)),
            "eager" => Ok(Self(SpawnMode::Eager, PhantomData)),
            other => Err(D::Error::custom(named::<F>(format!(
                "unknown mode `{other}`"
            )))),
        }
    }
}

/// `BLASTIT_IMMORTALIZE`.
pub(super) struct Immortal<F: JsonField>(pub Immortalize, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for Immortal<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)
            .map_err(|_| D::Error::custom(named::<F>("must be a string")))?;
        match text.as_str() {
            "auto" => Ok(Self(Immortalize::Auto, PhantomData)),
            "on" => Ok(Self(Immortalize::On, PhantomData)),
            "off" => Ok(Self(Immortalize::Off, PhantomData)),
            other => Err(D::Error::custom(named::<F>(format!(
                "unknown setting `{other}`"
            )))),
        }
    }
}

/// `BLASTIT_LOG_LEVEL`, parsed by the event crate's own `FromStr`.
pub(super) struct LevelText<F: JsonField>(pub Level, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for LevelText<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)
            .map_err(|_| D::Error::custom(named::<F>("must be a string")))?;
        text.parse::<Level>()
            .map(|level| Self(level, PhantomData))
            .map_err(|error| D::Error::custom(named::<F>(error)))
    }
}

/// The configuration warnings, by token.
pub(super) struct Warns<F: JsonField>(pub Vec<Warning>, PhantomData<F>);

impl<'de, F: JsonField> Deserialize<'de> for Warns<F> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let names = read_names(&value).map_err(|reason| D::Error::custom(named::<F>(reason)))?;
        names
            .iter()
            .map(|token| Warning::parse(token).map_err(|error| D::Error::custom(error_text(error))))
            .collect::<Result<Vec<Warning>, D::Error>>()
            .map(|warnings| Self(warnings, PhantomData))
    }
}
