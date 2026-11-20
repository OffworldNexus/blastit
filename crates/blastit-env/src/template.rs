//! The template engine (§5.1).
//!
//! Templates are deliberately tiny: literal text with `{{ field }}` and
//! `{{ field | urlencode }}` substitutions. Whitespace inside the braces is
//! optional, so `{{field}}`, `{{ field }}` and `{{  field  |  urlencode  }}`
//! all work. There is no logic, no loops, no access to the environment and no
//! other filter.
//!
//! A `}}` that no `{{` opened is literal text, so an app whose output value
//! happens to contain braces is not an error.
//!
//! ```
//! use blastit_env::{Fields, Template};
//!
//! let template = Template::parse("{{ dir | urlencode }}/.s.PGSQL.{{ port }}").unwrap();
//! let mut fields = Fields::new();
//! fields.insert("dir", "/proc/self/cwd");
//! fields.insert("port", "5432");
//! assert_eq!(template.render(&fields).unwrap(), "%2Fproc%2Fself%2Fcwd/.s.PGSQL.5432");
//! ```

use std::collections::BTreeMap;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// Percent-encodes everything except the RFC 3986 unreserved characters
/// (`A-Za-z0-9-._~`), which is what `urlencode` does and what libpq and
/// `dj-database-url` expect for a socket directory in a host position (§5.2).
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Percent-encodes a value the way `{{ field | urlencode }}` does.
///
/// `/proc/self/cwd` becomes `%2Fproc%2Fself%2Fcwd`.
pub fn urlencode(value: &str) -> String {
    utf8_percent_encode(value, UNRESERVED).to_string()
}

/// A filter a template may apply to a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    /// `{{ field | urlencode }}`: percent-encode the value.
    UrlEncode,
}

impl Filter {
    /// The manifest spelling of the filter.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UrlEncode => "urlencode",
        }
    }

    /// Parses a filter name; `None` for something this engine lacks.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "urlencode" => Some(Self::UrlEncode),
            _ => None,
        }
    }
}

/// One piece of a template.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    /// Text copied through unchanged.
    Literal(String),
    /// A field substitution, with an optional filter.
    Field {
        name: String,
        filter: Option<Filter>,
    },
}

/// A parsed template (§5.1).
///
/// [`Template::parse`] checks the syntax only: an unknown *field* is a
/// property of the protocol the template belongs to, so it is reported by
/// [`Template::render`], before any output value is built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    source: String,
    segments: Vec<Segment>,
}

/// One problem with a template (§5.1).
///
/// [`std::fmt::Display`] renders one line, whatever the template contains.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    /// A `{{` with no matching `}}`.
    #[error("unclosed `{{{{` at byte {at}")]
    Unclosed {
        /// Byte offset of the `{{`.
        at: usize,
    },
    /// `{{ }}`: a substitution with no field name.
    #[error("empty field name at byte {at}")]
    EmptyName {
        /// Byte offset of the `{{`.
        at: usize,
    },
    /// A filter the engine does not have.
    #[error("unknown filter \"{name}\"")]
    UnknownFilter {
        /// The filter as written.
        name: String,
    },
    /// A field the protocol does not offer. Reported by [`Template::render`].
    #[error("unknown field \"{name}\"")]
    UnknownField {
        /// The field as written.
        name: String,
    },
}

/// The field values a template may read.
///
/// A protocol builds one [`Fields`] per proxy; a template can only see what is
/// in it, which is how secrets stay out of templates (§5.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fields(BTreeMap<String, String>);

impl Fields {
    /// An empty field set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a field, replacing any previous value.
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.insert(name.into(), value.into());
    }

    /// Borrows a field, if the protocol offers it.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    /// True when the protocol offers this field.
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// The fields, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

impl Template {
    /// Parses a template (§5.1).
    ///
    /// Only the syntax is checked here: unclosed `{{`, empty names and unknown
    /// filters. Field names are checked by [`Template::render`] against the
    /// fields the protocol offers, so a template that names a field nobody
    /// provides still fails, but as an "unknown field" rather than a syntax
    /// error.
    pub fn parse(source: &str) -> Result<Self, TemplateError> {
        let mut segments = Vec::new();
        let mut offset = 0usize;
        let mut rest = source;

        while let Some(open) = rest.find("{{") {
            let at = offset + open;
            if open > 0 {
                segments.push(Segment::Literal(rest[..open].to_string()));
            }
            let after = &rest[open + 2..];
            let Some(close) = after.find("}}") else {
                return Err(TemplateError::Unclosed { at });
            };
            let inner = after[..close].trim();
            segments.push(parse_field(inner, at)?);
            let consumed = open + 2 + close + 2;
            offset += consumed;
            rest = &rest[consumed..];
        }

        if !rest.is_empty() {
            segments.push(Segment::Literal(rest.to_string()));
        }

        Ok(Self {
            source: source.to_string(),
            segments,
        })
    }

    /// Renders the template against a protocol's fields.
    ///
    /// Everything is checked before a single byte is produced: a template that
    /// names an unknown field fails without yielding a partial string.
    pub fn render(&self, fields: &Fields) -> Result<String, TemplateError> {
        for segment in &self.segments {
            if let Segment::Field { name, .. } = segment
                && !fields.contains(name)
            {
                return Err(TemplateError::UnknownField { name: name.clone() });
            }
        }

        let mut out = String::with_capacity(self.source.len());
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => out.push_str(text),
                Segment::Field { name, filter } => {
                    let value = fields.get(name).unwrap_or_default();
                    match filter {
                        None => out.push_str(value),
                        Some(Filter::UrlEncode) => out.push_str(&urlencode(value)),
                    }
                }
            }
        }
        Ok(out)
    }

    /// The template as written.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Every field the template names, in template order.
    pub fn field_names(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().filter_map(|segment| match segment {
            Segment::Field { name, .. } => Some(name.as_str()),
            Segment::Literal(_) => None,
        })
    }

    /// True when the template is only literal text and can never fail.
    pub fn is_literal(&self) -> bool {
        !self
            .segments
            .iter()
            .any(|segment| matches!(segment, Segment::Field { .. }))
    }
}

/// Parses the inside of a `{{ … }}`.
fn parse_field(inner: &str, at: usize) -> Result<Segment, TemplateError> {
    let mut parts = inner.split('|');
    let name = parts.next().unwrap_or("").trim();
    if name.is_empty() {
        return Err(TemplateError::EmptyName { at });
    }

    let mut filter = None;
    for part in parts {
        let part = part.trim();
        match Filter::parse(part) {
            Some(known) => filter = Some(known),
            None => {
                return Err(TemplateError::UnknownFilter {
                    name: part.to_string(),
                });
            }
        }
    }

    Ok(Segment::Field {
        name: name.to_string(),
        filter,
    })
}
