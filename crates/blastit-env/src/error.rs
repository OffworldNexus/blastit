//! Everything that can go wrong while rewriting the environment (§5.1, §5.3).
//!
//! A message here names proxies, slots and *variables* — never a value taken
//! from the platform environment. That is not a convention: the types only
//! ever carry names, and a test drives every error path with a sentinel secret
//! in the environment and asserts it appears in no `Display` and no `Debug`
//! output (§5.1 "secrets can't be templated", §8.1 "never logged").

use std::fmt;
use std::fmt::Write as _;

use blastit_manifest::ProxyName;

use crate::template::TemplateError;

/// Longest name quoted in an error message before it is cut short.
const MAX_QUOTED_CHARS: usize = 120;

/// Why a variable name cannot be used, if it cannot.
///
/// The name becomes a key in the child's environment block, which is a
/// NUL-separated `KEY=VALUE` list, so an empty name, an `=` or a NUL byte would
/// corrupt or truncate it (§5.3).
pub(crate) fn invalid_name_reason(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        Some("it is empty")
    } else if name.contains('=') {
        Some("it contains '='")
    } else if name.contains('\0') {
        Some("it contains a NUL byte")
    } else {
        None
    }
}

/// Renders a name for an error message: quoted, control characters escaped and
/// long values cut short, so one error always stays on one line (§8.1).
pub(crate) fn quoted(value: impl fmt::Display) -> String {
    let text = value.to_string();
    let cut = text.char_indices().nth(MAX_QUOTED_CHARS).map(|(at, _)| at);
    let mut out = String::with_capacity(text.len() + 3);
    out.push('"');
    match cut {
        Some(at) => {
            out.push_str(&escape_control(&text[..at]));
            out.push('…');
        }
        None => out.push_str(&escape_control(&text)),
    }
    out.push('"');
    out
}

/// Escapes control characters, so a name never breaks a line.
fn escape_control(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", character as u32);
            }
            character => out.push(character),
        }
    }
    out
}

/// One problem found while rewriting the environment (§5.1, §5.3).
///
/// [`fmt::Display`] renders one line. No variant carries a value from the
/// environment; they carry the names of the variables involved.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EnvError {
    /// `input` names a slot the proxy's protocol does not have.
    #[error(
        "proxy {}: unknown input slot {}; postgresql has {}",
        quoted(.proxy),
        quoted(.slot),
        crate::postgresql::INPUT_SLOTS.join(", ")
    )]
    UnknownSlot {
        /// The proxy being declared.
        proxy: ProxyName,
        /// The unknown slot name.
        slot: String,
    },
    /// `input` sets `url` together with one of the split slots.
    #[error(
        "proxy {}: input sets {} and also the split form; use one or the other",
        quoted(.proxy),
        quoted("url")
    )]
    UrlAndSplit {
        /// The proxy being declared.
        proxy: ProxyName,
    },
    /// `input` sets neither `url` nor `host`.
    #[error("proxy {}: input names neither {} nor {}", quoted(.proxy), quoted("url"), quoted("host"))]
    MissingUrlOrHost {
        /// The proxy being declared.
        proxy: ProxyName,
    },
    /// An input variable is not in the platform environment.
    #[error(
        "proxy {} input {}: environment variable {} is not set",
        quoted(.proxy),
        quoted(.slot),
        quoted(.variable)
    )]
    MissingVariable {
        /// The proxy being declared.
        proxy: ProxyName,
        /// The slot that wanted the variable.
        slot: String,
        /// The missing variable's name.
        variable: String,
    },
    /// An input variable's value is not valid UTF-8.
    #[error("environment variable {} is not valid UTF-8", quoted(.variable))]
    NonUtf8Variable {
        /// The variable's name.
        variable: String,
    },
    /// The split form's `port` input is not a port number.
    #[error(
        "proxy {} input {}: environment variable {} is not a port number",
        quoted(.proxy),
        quoted(.slot),
        quoted(.variable)
    )]
    InvalidPort {
        /// The proxy being declared.
        proxy: ProxyName,
        /// The slot that wanted the variable.
        slot: String,
        /// The variable's name.
        variable: String,
    },
    /// An input variable is not a usable libpq URI (§5.2).
    #[error(
        "proxy {} input {}: environment variable {} is not a valid postgresql URL",
        quoted(.proxy),
        quoted(.slot),
        quoted(.variable)
    )]
    InvalidUrl {
        /// The proxy being declared.
        proxy: ProxyName,
        /// The slot that wanted the variable.
        slot: String,
        /// The variable's name.
        variable: String,
    },
    /// A URL query parameter that blastit does not carry through (§5.2).
    ///
    /// Only the value of an allowlisted parameter is ever kept; the names that
    /// would move the connection somewhere else (or supply another credential)
    /// are rejected instead of being passed to dbproxy.
    #[error(
        "proxy {} input {}: variable {} names the query parameter {}, which is not allowed",
        quoted(.proxy),
        quoted(.slot),
        quoted(.variable),
        quoted(.parameter)
    )]
    DisallowedParameter {
        /// The proxy being declared.
        proxy: ProxyName,
        /// The slot that wanted the variable.
        slot: String,
        /// The variable's name.
        variable: String,
        /// The offending parameter's name (never its value).
        parameter: String,
    },
    /// A host list (`h1,h2`), which blastit does not support anywhere (§5.2).
    #[error(
        "proxy {} input {}: variable {} names more than one host; multi-host input is not supported",
        quoted(.proxy),
        quoted(.slot),
        quoted(.variable)
    )]
    MultipleHosts {
        /// The proxy being declared.
        proxy: ProxyName,
        /// The slot that wanted the variable.
        slot: String,
        /// The variable's name.
        variable: String,
    },
    /// A URL whose path names no database (`postgres://host`, `postgres://host/`)
    /// or a `database` input whose value is empty (§5.2).
    #[error(
        "proxy {} input {}: variable {} names no database",
        quoted(.proxy),
        quoted(.slot),
        quoted(.variable)
    )]
    MissingDatabase {
        /// The proxy being declared.
        proxy: ProxyName,
        /// The slot that wanted the variable.
        slot: String,
        /// The variable's name.
        variable: String,
    },
    /// The split form maps no `database` slot at all (§5.2).
    #[error(
        "proxy {} input: the split form needs a {} slot",
        quoted(.proxy),
        quoted("database")
    )]
    NoDatabaseSlot {
        /// The proxy being declared.
        proxy: ProxyName,
    },
    /// An input or output variable name cannot be a key in the child's
    /// environment block (§5.3).
    #[error(
        "proxy {}: variable {} is not a usable name: {}",
        quoted(.proxy),
        quoted(.variable),
        .reason
    )]
    InvalidVariableName {
        /// The proxy being declared.
        proxy: ProxyName,
        /// The offending name.
        variable: String,
        /// Why it cannot be used.
        reason: &'static str,
    },
    /// An output would name a `BLASTIT_*` variable, which blastit keeps for
    /// itself (§5.3).
    #[error(
        "proxy {} output {}: an output must not name a BLASTIT_ variable",
        quoted(.proxy),
        quoted(.variable)
    )]
    ReservedOutput {
        /// The proxy producing the output.
        proxy: ProxyName,
        /// The offending variable name.
        variable: String,
    },
    /// An output would overwrite a variable that is not one of the proxy's own
    /// inputs (§5.1).
    #[error(
        "proxy {} output {}: the variable is set in the environment but is not one of this proxy's inputs",
        quoted(.proxy),
        quoted(.variable)
    )]
    OutputOverwritesForeign {
        /// The proxy producing the output.
        proxy: ProxyName,
        /// The offending variable name.
        variable: String,
    },
    /// Two proxies produce the same variable (§5.1).
    #[error(
        "output {}: produced by proxy {} and proxy {}",
        quoted(.variable),
        quoted(.first),
        quoted(.second)
    )]
    DuplicateOutput {
        /// The variable both proxies produce.
        variable: String,
        /// The first proxy to produce it.
        first: ProxyName,
        /// The second proxy to produce it.
        second: ProxyName,
    },
    /// An output template does not parse, or names a field the protocol does
    /// not offer (§5.1).
    #[error(
        "proxy {} output {}: {error}",
        quoted(.proxy),
        quoted(.variable)
    )]
    Template {
        /// The proxy producing the output.
        proxy: ProxyName,
        /// The variable the template fills.
        variable: String,
        /// What is wrong with the template.
        error: TemplateError,
    },
    /// There are more `postgresql` proxies than the port range above
    /// [`crate::postgresql::PORT_BASE`] can hold (§5.2).
    #[error(
        "proxy {}: too many postgresql proxies; there is no local port left above {}",
        quoted(.proxy),
        crate::postgresql::PORT_BASE
    )]
    TooManyProxies {
        /// The first proxy that has no port.
        proxy: ProxyName,
    },
}

/// Every problem found while rewriting the environment, so a boot reports them
/// all at once (§5.1).
///
/// [`fmt::Display`] lists them, one per line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvErrors(Vec<EnvError>);

impl EnvErrors {
    /// Collects problems, in the order they were found.
    pub fn new(errors: Vec<EnvError>) -> Self {
        Self(errors)
    }

    /// The problems themselves.
    pub fn errors(&self) -> &[EnvError] {
        &self.0
    }

    /// How many problems were found.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when there is nothing to report. A value handed back as an error is
    /// never empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Consumes the list and returns the problems.
    pub fn into_errors(self) -> Vec<EnvError> {
        self.0
    }
}

impl fmt::Display for EnvErrors {
    /// Lists the problems, one per line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "environment rewrite has {} error(s)", self.0.len())?;
        for error in &self.0 {
            write!(f, "\n  - {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for EnvErrors {}

impl From<EnvError> for EnvErrors {
    fn from(error: EnvError) -> Self {
        Self(vec![error])
    }
}

impl From<Vec<EnvError>> for EnvErrors {
    fn from(errors: Vec<EnvError>) -> Self {
        Self::new(errors)
    }
}

impl IntoIterator for EnvErrors {
    type Item = EnvError;
    type IntoIter = std::vec::IntoIter<EnvError>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a EnvErrors {
    type Item = &'a EnvError;
    type IntoIter = std::slice::Iter<'a, EnvError>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use blastit_manifest::ProxyName;

    use super::{EnvError, EnvErrors, invalid_name_reason, quoted};
    use crate::template::TemplateError;

    #[test]
    fn unusable_variable_names_are_named_with_a_reason() {
        assert_eq!(invalid_name_reason("DATABASE_URL"), None);
        assert_eq!(invalid_name_reason(""), Some("it is empty"));
        assert_eq!(invalid_name_reason("A=B"), Some("it contains '='"));
        assert_eq!(invalid_name_reason("A\0B"), Some("it contains a NUL byte"));
    }

    #[test]
    fn invalid_name_errors_escape_control_characters() {
        let error = EnvError::InvalidVariableName {
            proxy: ProxyName::from("db"),
            variable: "A\0B".to_string(),
            reason: "it contains a NUL byte",
        };
        let text = error.to_string();
        assert!(!text.contains('\0'));
        assert!(text.contains(r"A\u{0}B"), "{text}");
    }

    #[test]
    fn quoted_escapes_control_characters_and_cuts_long_values() {
        assert_eq!(quoted("a\nb"), "\"a\\nb\"");
        assert!(!quoted("x".repeat(500)).contains(&"x".repeat(121)));
        assert!(quoted("x".repeat(500)).ends_with("…\""));
    }

    #[test]
    fn errors_render_on_one_line() {
        let error = EnvError::Template {
            proxy: ProxyName::from("db"),
            variable: "X".to_string(),
            error: TemplateError::UnknownField {
                name: "password".to_string(),
            },
        };
        let text = error.to_string();
        assert!(!text.contains('\n'));
        assert_eq!(
            text,
            "proxy \"db\" output \"X\": unknown field \"password\""
        );
    }

    #[test]
    fn errors_are_collected_and_rendered_together() {
        let errors = EnvErrors::new(vec![
            EnvError::MissingUrlOrHost {
                proxy: ProxyName::from("db"),
            },
            EnvError::UrlAndSplit {
                proxy: ProxyName::from("analytics"),
            },
        ]);
        assert_eq!(errors.len(), 2);
        assert!(!errors.is_empty());
        let text = errors.to_string();
        assert!(text.starts_with("environment rewrite has 2 error(s)\n"));
        assert_eq!(text.lines().count(), 3);
        assert_eq!(errors.errors().len(), 2);
        assert_eq!((&errors).into_iter().count(), 2);
        assert_eq!(errors.clone().into_errors().len(), 2);
        assert_eq!(EnvErrors::from(errors.clone().into_errors()).len(), 2);
    }
}
