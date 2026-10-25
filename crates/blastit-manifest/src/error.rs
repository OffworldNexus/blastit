//! What can be wrong with a manifest: parse errors, validation errors and the
//! non-fatal warnings (§6).

use std::fmt;
use std::fmt::Write as _;

use crate::model::ProxyName;

/// Longest value quoted in an error message before it is cut short.
pub(crate) const MAX_QUOTED_CHARS: usize = 120;

/// Escapes control characters, so a name or a path never breaks a line.
pub(crate) fn escape_control(value: &str) -> String {
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

/// Renders a name, path or pattern for an error message: quoted, with control
/// characters escaped and long values cut to [`MAX_QUOTED_CHARS`] characters.
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

/// A regex compile failure as a single line: the `regex` crate reports syntax
/// errors with a source excerpt, and the last line is the actual problem.
pub(crate) fn regex_message(error: &regex::Error) -> String {
    let text = error.to_string();
    let last = text
        .lines()
        .last()
        .map_or(text.as_str(), |line| line)
        .trim();
    escape_control(last)
}

/// Where in the manifest something sits, so an error can name it precisely.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// The top-level `groups` list, which attaches a group to every route.
    Global,
    /// A `[group.<name>]` section.
    Group(String),
    /// A `[[route]]` entry, by index and by name.
    Route {
        /// Index of the route in the manifest.
        index: usize,
        /// The route's name, empty when the name itself failed to parse.
        name: String,
    },
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => f.write_str("global groups"),
            Self::Group(group) => write!(f, "group {}", quoted(group)),
            Self::Route { index, name } => {
                write!(f, "route {} (index {index})", quoted(name))
            }
        }
    }
}

/// One problem with a manifest.
///
/// Parsing reports the structural ones ([`ManifestError::Syntax`],
/// [`ManifestError::UnknownKey`], [`ManifestError::WrongType`]…); validation
/// reports the ones that need the whole document
/// ([`ManifestError::UnknownGroup`], [`ManifestError::InvalidRegex`]…).
///
/// [`Display`](fmt::Display) always renders one line, whatever the manifest
/// contains.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// The source is not valid TOML.
    #[error("invalid TOML: {message}")]
    Syntax {
        /// The parser's message, with the line and column when known.
        message: String,
    },
    /// `version` is absent; a manifest must declare `version = 1` (§6).
    #[error("version: missing `version = 1`")]
    MissingVersion,
    /// `version` is present but is not `1`.
    #[error("version: expected 1, found {found}")]
    UnsupportedVersion {
        /// The offending value, rendered as TOML.
        found: String,
    },
    /// A key that is not part of the schema. The manifest is strict, so this is
    /// an error rather than a comment.
    #[error("{path}: unknown key")]
    UnknownKey {
        /// Dotted path of the offending key.
        path: String,
    },
    /// A required key is absent.
    #[error("{path}: missing required key")]
    MissingKey {
        /// Dotted path of the missing key.
        path: String,
    },
    /// A value has the wrong TOML type.
    #[error("{path}: expected {expected}, found {found}")]
    WrongType {
        /// Dotted path of the offending value.
        path: String,
        /// What the schema expects there.
        expected: &'static str,
        /// The TOML type that is actually there.
        found: &'static str,
    },
    /// A name that must not be empty is empty.
    #[error("{path}: must not be empty")]
    Empty {
        /// Dotted path of the offending value, or a description of the name.
        path: String,
    },
    /// `protocol` names something this build does not have (§5.1).
    #[error("proxy {}: unknown protocol {} (known: \"postgresql\")", quoted(.proxy), quoted(.protocol))]
    UnknownProtocol {
        /// The proxy being declared.
        proxy: String,
        /// The protocol the manifest asked for.
        protocol: String,
    },
    /// A group is referenced where it was never declared (§6).
    #[error("{location}: unknown group {}", quoted(.group))]
    UnknownGroup {
        /// Where the group is referenced.
        location: Location,
        /// The undeclared group name.
        group: String,
    },
    /// A grant names a proxy that has no `[proxy.<name>]` section (§5.1, §6).
    #[error("{location}: grant on unknown proxy {}", quoted(.proxy))]
    GrantOnUnknownProxy {
        /// Where the grant is written.
        location: Location,
        /// The undeclared proxy name.
        proxy: ProxyName,
    },
    /// Two routes share a name; routes are addressed by name in the logs and by
    /// index in the uid table, so a duplicate is ambiguous.
    #[error(
        "duplicate route name {} (routes {first} and {second})",
        quoted(.name)
    )]
    DuplicateRouteName {
        /// The duplicated name.
        name: String,
        /// Index of the first route with that name.
        first: usize,
        /// Index of the second one.
        second: usize,
    },
    /// A route name does not match `[a-z0-9][a-z0-9_-]*`.
    #[error(
        "route {index}: invalid name {}; must match [a-z0-9][a-z0-9_-]*",
        quoted(.name)
    )]
    InvalidRouteName {
        /// Index of the route in the manifest.
        index: usize,
        /// The offending name.
        name: String,
    },
    /// A route lists no path regexes at all, so nothing can ever reach it.
    #[error("route {}: path must list at least one regex", quoted(.route))]
    EmptyPath {
        /// The route's name.
        route: String,
    },
    /// A route lists an empty path regex, which would only match an empty path.
    #[error(
        "route {} rule {rule}: the path regex must not be empty",
        quoted(.route)
    )]
    EmptyPattern {
        /// The route's name.
        route: String,
        /// Index of the pattern in the route's `path` list.
        rule: usize,
    },
    /// A path regex does not compile (§6).
    #[error(
        "route {} rule {rule}: invalid regex {}: {message}",
        quoted(.route),
        quoted(.pattern)
    )]
    InvalidRegex {
        /// The route's name.
        route: String,
        /// Index of the pattern in the route's `path` list.
        rule: usize,
        /// The pattern as written.
        pattern: String,
        /// The regex engine's message, on one line.
        message: String,
    },
    /// A method is not an uppercase HTTP method token.
    #[error(
        "route {}: invalid method {}; expected an uppercase HTTP method token",
        quoted(.route),
        quoted(.method)
    )]
    InvalidMethod {
        /// The route's name.
        route: String,
        /// The offending method token.
        method: String,
    },
    /// A route lists methods but none of them, so it can never be reached.
    #[error(
        "route {}: methods must list at least one method, or be left out",
        quoted(.route)
    )]
    EmptyMethods {
        /// The route's name.
        route: String,
    },
    /// The route's uid would leave the range blastit stays in (§2).
    #[error(
        "too many routes: route at index {index} would use uid {uid}, \
         but uids must stay below {max} (base {base})"
    )]
    TooManyRoutes {
        /// Index of the last route in the manifest.
        index: usize,
        /// The uid that route would take.
        uid: u32,
        /// The uid base in use.
        base: u32,
        /// Exclusive upper bound of the uid range.
        max: u32,
    },
    /// The uid base itself leaves no room for blastit's own processes.
    #[error("uid base {base}: dbproxy would use uid {uid}, but uids must stay below {max}")]
    UidBaseTooHigh {
        /// The uid base in use.
        base: u32,
        /// The uid dbproxy would take.
        uid: u32,
        /// Exclusive upper bound of the uid range.
        max: u32,
    },
}

/// Something suspicious that does not stop the server from starting (§6).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ManifestWarning {
    /// A `[group.<name>]` that no route and no global `groups` entry
    /// references: it grants nothing.
    #[error(
        "group {} is attached to no route and no global groups: it grants nothing",
        quoted(.group)
    )]
    UnattachedGroup {
        /// The group's name.
        group: String,
    },
    /// A proxy that no route and no group grants: it is declared, but nothing
    /// may use it, so its variables never reach the app.
    #[error("proxy {} has no grant in any route or group: it is unusable", quoted(.proxy))]
    UngrantedProxy {
        /// The proxy's name.
        proxy: ProxyName,
    },
}

/// Every problem found in one manifest, so a boot can report them all at once.
///
/// [`Display`](fmt::Display) lists them, one per line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestErrors(Vec<ManifestError>);

impl ManifestErrors {
    /// Collects problems, in the order they were found.
    pub fn new(errors: Vec<ManifestError>) -> Self {
        Self(errors)
    }

    /// The problems themselves.
    pub fn errors(&self) -> &[ManifestError] {
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
    pub fn into_errors(self) -> Vec<ManifestError> {
        self.0
    }
}

impl fmt::Display for ManifestErrors {
    /// Lists the problems, one per line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "manifest has {} error(s)", self.0.len())?;
        for error in &self.0 {
            write!(f, "\n  - {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ManifestErrors {}

impl From<ManifestError> for ManifestErrors {
    fn from(error: ManifestError) -> Self {
        Self(vec![error])
    }
}

impl From<Vec<ManifestError>> for ManifestErrors {
    fn from(errors: Vec<ManifestError>) -> Self {
        Self::new(errors)
    }
}

impl IntoIterator for ManifestErrors {
    type Item = ManifestError;
    type IntoIter = std::vec::IntoIter<ManifestError>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a ManifestErrors {
    type Item = &'a ManifestError;
    type IntoIter = std::slice::Iter<'a, ManifestError>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
