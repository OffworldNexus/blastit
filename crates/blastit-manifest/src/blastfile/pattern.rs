//! Path patterns: the one compiler from a `Blastfile` pattern to the regex the
//! scheduler matches (§8.1), and the one method list lowering normalises before
//! a `Route` exists (§8.4).
//!
//! A pattern is not a regex, and it cannot be escaped as a whole either:
//! escaping `"/products/{id:int}"` would make `{id:int}` literal, and a string
//! replacement of `{…}` would be ambiguous with a `{` in literal text (§8.2).
//! [`compile`] therefore scans the pattern **once**, by `char_indices()`:
//!
//! 1. `{` opens a parameter: the text runs to the next `}`, splits on `:`, and
//!    the name and the optional type are validated before the type's fixed
//!    fragment is pushed.
//! 2. `*` at the last byte, preceded by `/`, pushes `.*`. Anywhere else —
//!    including `"/assets*"` — is a refusal, so a pattern can never match
//!    `/assets-secret` by accident.
//! 3. every other run of literal bytes goes through [`regex::escape`].
//!
//! Because every literal byte is escaped and the only unescaped fragments are
//! the three fixed ones below, a pattern can never emit a character class or a
//! quantifier of its own — the property §17.3 relies on. `regex '…'` is the
//! escape hatch: its text is kept verbatim and compiled as written (§8.1).
//!
//! Both spellings are capped at [`MAX_PATTERN_BYTES`] (§4.5), so a `regex`
//! cannot hide a megabyte pattern the quoted form would refuse, and every
//! fragment is compiled by the existing `routing::compile_pattern` —
//! pre-compiled on its own, so an unbalanced `)` can never be rescued by the
//! wrapper into a catch-all, then wrapped as `^(?:…)$` so it has to match the
//! whole path. The cap is on the pattern's text only: the compiled program is
//! still bounded by `REGEX_SIZE_LIMIT` (`1 << 20`) and `REGEX_NEST_LIMIT`, so a
//! longer source pattern never buys a bigger regex (§17.4).
//!
//! [`normalise_methods`] implements §8.4's table: `HEAD` is implied by `GET`,
//! written order is kept, duplicates are deduplicated, and `CONNECT`/`TRACE`
//! are refused. `HEAD` therefore lands in `Route.methods` before
//! [`RoutingTable`](crate::RoutingTable) sees the route, so request matching
//! itself does not change.

use crate::error::{ManifestError, Position, regex_message};
use crate::routing::compile_pattern;

/// Largest pattern either spelling may hold, in **bytes** (§4.5): 4 KiB.
///
/// It lives here rather than beside the other budgets in `mod.rs` because this
/// is the one place that sees the text of both forms (§8.1), and it is checked
/// before either is scanned or compiled.
///
/// This is a cap on the *source* pattern, not on the compiled regex: the program
/// `regex '…'` builds is still bounded by `REGEX_SIZE_LIMIT` (`routing.rs`,
/// `1 << 20`) and `REGEX_NEST_LIMIT`, which are unchanged. A 4 KiB pattern is
/// permission to write a longer pattern, never to compile a bigger program than
/// 0.2.0 already allowed (§17.4).
pub(crate) const MAX_PATTERN_BYTES: usize = 4_096;

/// The fragment `{name}` emits: one non-empty segment (§8.1).
const SEGMENT: &str = "[^/]+";

/// The fragment `{name:int}` emits: digits only (§8.1).
const INTEGER: &str = "[0-9]+";

/// The fragment `{name:uuid}` emits: the canonical UUID form (§8.1).
const UUID: &str = "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}";

/// The fragment a trailing `*` emits: the prefix and everything after it
/// (§8.1). It crosses `/` by design, which is what makes `/assets/*` answer a
/// nested file.
const TAIL: &str = ".*";

/// One compiled pattern: the text it was compiled from, and the fragment it
/// emits (§8.1).
///
/// The parser stores one per `pattern` node, keyed by the pattern's token, so
/// `lower` reads the fragment without recompiling and without a second escape
/// layer: the value is decoded once, here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Compiled {
    /// The pattern's value, as compiled: decoded for `"…"`, verbatim for `'…'`.
    text: String,
    /// The fragment a `Route.path` entry holds.
    fragment: String,
}

impl Compiled {
    /// The pattern's value, as compiled.
    #[allow(dead_code)] // the editor reads the value (`MB.21.3.5`)
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The fragment the pattern emits, which is what a `Route.path` entry
    /// holds; [`RoutingTable`](crate::RoutingTable) anchors it (§6).
    pub(crate) fn fragment(&self) -> &str {
        &self.fragment
    }
}

/// Compiles one quoted pattern into the fragment a `Route.path` entry holds
/// (§8.1).
///
/// The fragment is not the anchored regex: a `Route` keeps the pattern as
/// written and [`RoutingTable`](crate::RoutingTable) wraps it in `^(?:…)$` when
/// it builds the table, so the anchoring lives in one place (§6).
///
/// `at` is the position of the pattern's opening quote, which is where every
/// refusal points.
///
/// # Errors
///
/// §8.3's rows — an unclosed parameter, `{}`, a bad name, an unknown type, a
/// `*` that is not the last byte after a `/`, a duplicate parameter name, a
/// pattern that does not start with `/`, and a pattern over
/// [`MAX_PATTERN_BYTES`] — each at `at`.
pub(crate) fn compile(pattern: &str, at: Position) -> Result<Compiled, ManifestError> {
    check_cap(pattern, at)?;
    if !pattern.starts_with('/') {
        return Err(ManifestError::PatternNotAnchored { at });
    }
    let mut fragment = String::with_capacity(pattern.len());
    // The start of the literal run that has not been escaped yet.
    let mut literal = 0usize;
    // The parameter names seen so far, in written order (§8.3's duplicate row).
    let mut names: Vec<&str> = Vec::new();
    let mut characters = pattern.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        match character {
            '{' => {
                push_literal(&mut fragment, pattern, literal, index);
                let close = parameter_end(pattern, index).ok_or_else(|| {
                    ManifestError::PatternParameterUnclosed {
                        at,
                        parameter: pattern.get(index..).unwrap_or_default().to_owned(),
                    }
                })?;
                let body = pattern
                    .get(index.saturating_add(1)..close)
                    .unwrap_or_default();
                fragment.push_str(parameter(body, at, &mut names)?);
                // The parameter's text is consumed: continue after its `}`.
                while characters.next_if(|(offset, _)| *offset <= close).is_some() {}
                literal = close.saturating_add(1);
            }
            '*' => {
                push_literal(&mut fragment, pattern, literal, index);
                let last = index.saturating_add(1) == pattern.len();
                let after_slash = pattern.get(..index).is_some_and(|head| head.ends_with('/'));
                if !last || !after_slash {
                    return Err(ManifestError::PatternStarPosition { at });
                }
                fragment.push_str(TAIL);
                literal = index.saturating_add(1);
            }
            _ => {}
        }
    }
    push_literal(&mut fragment, pattern, literal, pattern.len());
    // No compile here: the fragment is built only from `regex::escape` output
    // and the three fixed fragments above, so it always compiles, and
    // `RoutingTable` compiles it with `compile_pattern` — pre-compiled on its
    // own, then wrapped `^(?:…)$` — when it builds the table (§8.1, §17.4). A
    // second compile per pattern would double the cost of parsing a generated
    // file for an invariant §8.2's construction already gives.
    Ok(Compiled {
        text: pattern.to_owned(),
        fragment,
    })
}

/// Compiles one `regex '…'` pattern: the text verbatim, wrapped by
/// `routing::compile_pattern` (§8.1).
///
/// The raw string form is the escape hatch, so nothing here rewrites the text:
/// a backslash reaches the compiler exactly as written and no escape layer can
/// disagree with it (§17.4). It is still capped at [`MAX_PATTERN_BYTES`]
/// (§4.5), and a pattern the `regex` crate refuses is a refusal naming the
/// crate's own message — never a panic.
///
/// # Errors
///
/// A pattern over [`MAX_PATTERN_BYTES`], and any pattern
/// `routing::compile_pattern` refuses, at `at`.
pub(crate) fn compile_regex(pattern: &str, at: Position) -> Result<Compiled, ManifestError> {
    check_cap(pattern, at)?;
    match compile_pattern(pattern) {
        Ok(_) => Ok(Compiled {
            text: pattern.to_owned(),
            fragment: pattern.to_owned(),
        }),
        Err(error) => Err(ManifestError::PatternRegex {
            at,
            pattern: pattern.to_owned(),
            message: regex_message(&error),
        }),
    }
}

/// Normalises a view header's method list (§8.4).
///
/// `HEAD` is implied by `GET`: a view that declares `GET` answers `HEAD` too,
/// and the implication happens **here**, before a `Route` exists, so
/// [`RoutingTable`](crate::RoutingTable) keeps comparing exact strings.
///
/// The table, exactly (§8.4):
///
/// | Written | Result |
/// |---|---|
/// | nothing | `None` — any method the front end lets through |
/// | `GET` | `["GET", "HEAD"]` |
/// | `GET, HEAD` | `["GET", "HEAD"]` |
/// | `POST` | `["POST"]` |
/// | `GET, POST` | `["GET", "HEAD", "POST"]` |
/// | a duplicate | deduplicated, not an error |
///
/// Written order is kept and `HEAD` follows the `GET` that implied it.
///
/// # Errors
///
/// `CONNECT` and `TRACE` cannot reach a view (§8.4), at the method's own
/// position.
pub(crate) fn normalise_methods(
    written: &[(&str, Position)],
) -> Result<Option<Vec<String>>, ManifestError> {
    if written.is_empty() {
        return Ok(None);
    }
    let mut methods: Vec<String> = Vec::with_capacity(written.len().saturating_add(1));
    for (method, at) in written {
        if matches!(*method, "CONNECT" | "TRACE") {
            return Err(ManifestError::MethodCannotReachView {
                at: *at,
                method: (*method).to_owned(),
            });
        }
        if methods.iter().any(|listed| listed == method) {
            continue;
        }
        methods.push((*method).to_owned());
        if *method == "GET" && !methods.iter().any(|listed| listed == "HEAD") {
            methods.push("HEAD".to_owned());
        }
    }
    Ok(Some(methods))
}

/// The byte offset of the `}` that closes the parameter opened at `open`.
///
/// `None` when the pattern holds no `}` after it, which is §8.3's unclosed
/// parameter.
fn parameter_end(pattern: &str, open: usize) -> Option<usize> {
    pattern
        .get(open..)
        .and_then(|tail| tail.find('}'))
        .map(|offset| open.saturating_add(offset))
}

/// The fragment one parameter body (`id`, `id:int`, …) emits (§8.1).
///
/// `names` carries the parameters already seen in this pattern, so a second
/// occurrence of one name is §8.3's duplicate row.
fn parameter<'a>(
    body: &'a str,
    at: Position,
    names: &mut Vec<&'a str>,
) -> Result<&'static str, ManifestError> {
    let (name, kind) = match body.split_once(':') {
        Some((name, kind)) => (name, Some(kind)),
        None => (body, None),
    };
    if name.is_empty() {
        return Err(ManifestError::PatternParameterUnnamed { at });
    }
    if !is_parameter_name(name) {
        return Err(ManifestError::PatternParameterName {
            at,
            parameter: body.to_owned(),
        });
    }
    let fragment = match kind {
        None => SEGMENT,
        Some("int") => INTEGER,
        Some("uuid") => UUID,
        Some(_) => {
            return Err(ManifestError::PatternParameterType {
                at,
                parameter: body.to_owned(),
            });
        }
    };
    if names.contains(&name) {
        return Err(ManifestError::PatternParameterDuplicate {
            at,
            name: name.to_owned(),
        });
    }
    names.push(name);
    Ok(fragment)
}

/// True for a parameter name: `[a-z][a-z0-9_]*` (§8.3).
///
/// A parameter name is narrower than a view name: no `-`, and it may not start
/// with a digit.
fn is_parameter_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Appends the literal run `pattern[from..to]`, escaped (§8.2 step 3).
///
/// An empty run is nothing: a pattern that is all parameters emits only their
/// fragments.
fn push_literal(fragment: &mut String, pattern: &str, from: usize, to: usize) {
    if let Some(run) = pattern.get(from..to) {
        fragment.push_str(&regex::escape(run));
    }
}

/// Refuses a pattern over [`MAX_PATTERN_BYTES`] (§4.5), both spellings.
fn check_cap(pattern: &str, at: Position) -> Result<(), ManifestError> {
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(ManifestError::PatternTooLong { at });
    }
    Ok(())
}
