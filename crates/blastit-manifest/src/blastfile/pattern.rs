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
//! whole path.
//!
//! [`normalise_methods`] implements §8.4's table: `HEAD` is implied by `GET`,
//! written order is kept, duplicates are deduplicated, and `CONNECT`/`TRACE`
//! are refused. `HEAD` therefore lands in `Route.methods` before
//! [`RoutingTable`](crate::RoutingTable) sees the route, so request matching
//! itself does not change.

use crate::error::{ManifestError, Position, regex_message};
use crate::routing::compile_pattern;

/// Largest pattern either spelling may hold, in bytes (§4.5).
///
/// It lives here rather than beside the other budgets in `mod.rs` because this
/// is the one place that sees the text of both forms (§8.1), and it is checked
/// before either is scanned or compiled.
pub(crate) const MAX_PATTERN_BYTES: usize = 256;

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
    #[allow(dead_code)] // the printer and the editor read the value (`MB.21.3.4`, `MB.21.3.5`)
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::{MAX_PATTERN_BYTES, compile, compile_regex, normalise_methods, parameter_end};
    use crate::error::{ManifestError, Position};
    use crate::routing::compile_pattern;

    /// The position every table case is written at, so a refusal's `line:col`
    /// is asserted as well as its sentence (§18).
    const AT: Position = Position::at(3, 7);

    /// The fragment `pattern` emits, or a panic naming the refusal.
    fn fragment(pattern: &str) -> String {
        compile(pattern, AT)
            .expect("the pattern compiles")
            .fragment()
            .to_owned()
    }

    /// Whether `pattern` matches the whole of `path`, through the same
    /// anchored compile the scheduler uses.
    fn matched(pattern: &str, path: &str) -> bool {
        compile_pattern(&fragment(pattern))
            .expect("the fragment compiles")
            .is_match(path)
    }

    /// Whether `regex 'pattern'` matches the whole of `path`.
    fn matched_regex(pattern: &str, path: &str) -> bool {
        let compiled = compile_regex(pattern, AT).expect("the regex compiles");
        compile_pattern(compiled.fragment())
            .expect("the regex compiles")
            .is_match(path)
    }

    /// The rendered refusal of a quoted pattern.
    fn refused(pattern: &str) -> String {
        compile(pattern, AT)
            .expect_err("the pattern is refused")
            .to_string()
    }

    /// §8.1's table, both directions: every row matches what it says it
    /// matches, and nothing that only shares its characters.
    #[test]
    fn the_table() {
        // "/products" — exactly that path.
        assert!(matched("/products", "/products"));
        assert!(!matched("/products", "/product"));
        assert!(!matched("/products", "/products/1"));
        assert!(!matched("/products", "/x/products"));

        // "/v1.0/x" — `.` is a literal dot.
        assert!(matched("/v1.0/x", "/v1.0/x"));
        assert!(!matched("/v1.0/x", "/v1x0/x"));
        assert!(!matched("/v1.0/x", "/v1.0/x/y"));

        // "/products/{id}" — one non-empty segment, never two.
        assert!(matched("/products/{id}", "/products/7"));
        assert!(matched("/products/{id}", "/products/a-b.c"));
        assert!(!matched("/products/{id}", "/products/"));
        assert!(!matched("/products/{id}", "/products/7/x"));
        assert!(!matched("/products/{id}", "/products/7/8"));

        // "/products/{id:int}" — digits only, so `12a` is not an id.
        assert!(matched("/products/{id:int}", "/products/42"));
        assert!(!matched("/products/{id:int}", "/products/12a"));
        assert!(!matched("/products/{id:int}", "/products/a12"));
        assert!(!matched("/products/{id:int}", "/products/"));
        assert!(!matched("/products/{id:int}", "/products/4.2"));

        // "/orders/{id:uuid}" — the canonical form, so a near miss is refused.
        let uuid = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        assert!(matched("/orders/{id:uuid}", &format!("/orders/{uuid}")));
        assert!(matched(
            "/orders/{id:uuid}",
            "/orders/6BA7B810-9DAD-11D1-80B4-00C04FD430C8"
        ));
        // The last group is twelve hex digits: eleven is a near miss.
        assert!(!matched(
            "/orders/{id:uuid}",
            "/orders/6ba7b810-9dad-11d1-80b4-00c04fd430c"
        ));
        // So is the same UUID with no dashes, and a 13-digit first group.
        assert!(!matched(
            "/orders/{id:uuid}",
            "/orders/6ba7b8109dad11d180b400c04fd430c8"
        ));
        assert!(!matched(
            "/orders/{id:uuid}",
            "/orders/6ba7b8100-9dad-11d1-80b4-00c04fd430c8"
        ));
        assert!(!matched("/orders/{id:uuid}", "/orders/not-a-uuid"));

        // "/assets/*" — the prefix and everything after it, nested files
        // included, but not the prefix itself (§8.1).
        assert!(matched("/assets/*", "/assets/a/b.css"));
        assert!(matched("/assets/*", "/assets/app.js"));
        assert!(matched("/assets/*", "/assets/"));
        assert!(!matched("/assets/*", "/assets"));
        assert!(!matched("/assets/*", "/assets-secret/key.pem"));
        assert!(!matched("/assets/*", "/other/a.css"));

        // "/*" — every path that starts with `/`.
        assert!(matched("/*", "/"));
        assert!(matched("/*", "/anything/at/all"));

        // A bare "/x" does not match "/xy": the wrapper anchors the whole path.
        assert!(matched("/x", "/x"));
        assert!(!matched("/x", "/xy"));
        assert!(!matched("/x", "/x/y"));
        assert!(!matched("/x", "x"));

        // "/a.b" is not "/axb".
        assert!(matched("/a.b", "/a.b"));
        assert!(!matched("/a.b", "/axb"));
    }

    /// The emitted fragment is §8.1's table, character for character — not
    /// merely something that happens to match the same paths.
    #[test]
    fn the_emitted_fragments() {
        assert_eq!(fragment("/products"), "/products");
        assert_eq!(fragment("/v1.0/x"), "/v1\\.0/x");
        assert_eq!(fragment("/products/{id}"), "/products/[^/]+");
        assert_eq!(fragment("/products/{id:int}"), "/products/[0-9]+");
        assert_eq!(
            fragment("/orders/{id:uuid}"),
            "/orders/[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"
        );
        assert_eq!(fragment("/assets/*"), "/assets/.*");
        assert_eq!(fragment("/*"), "/.*");
        // Two parameters, one literal run between them.
        assert_eq!(fragment("/a/{x}/b/{y:int}"), "/a/[^/]+/b/[0-9]+");
    }

    /// Every literal byte is escaped, so a pattern cannot emit a character
    /// class or a quantifier of its own (§8.2, §17.3).
    #[test]
    fn literal_bytes_are_escaped() {
        for path in [
            "/a.b", "/a+b", "/a(b)", "/a[b]", "/a$b", "/a^b", "/a?b", "/a|b", "/a\\b", "/a}b",
            "/a-b", "/a&b", "/a~b", "/a#b",
        ] {
            assert!(matched(path, path), "{path} must match itself");
        }
        // `*` is the other special byte: only a trailing one after `/` is a
        // tail, so a literal `*` is written with `regex '…'` (§8.2).
        assert_eq!(
            refused("/a*b"),
            "3:7: `*` is only valid at the end of a pattern; write `/x/*`"
        );
        assert!(matched_regex("/a\\*b", "/a*b"));
        // The metacharacters are literals, not wildcards.
        assert!(!matched("/a+b", "/aab"));
        assert!(!matched("/a?b", "/axb"));
        assert!(!matched("/a.b", "/axb"));
        assert!(!matched("/a(b)", "/ab"));
        assert!(!matched("/a[b]", "/ab"));
        assert!(!matched("/a$b", "/ab"));
        assert!(!matched("/a^b", "/ab"));
        assert!(!matched("/a|b", "/a"));
        assert!(!matched("/a|b", "/b"));
    }

    /// A `}` with no `{` is literal text: §8.2 makes only `{` special, and
    /// §8.3 pins no sentence for a stray `}`.
    ///
    /// A literal `{` is the other half of the same rule: `{` always opens a
    /// parameter, so a pattern that needs a literal brace is written with
    /// `regex '…'`, where nothing is scanned (§8.2).
    #[test]
    fn a_stray_close_brace_is_literal() {
        assert_eq!(fragment("/a}b"), "/a\\}b");
        assert!(matched("/a}b", "/a}b"));
        assert!(!matched("/a}b", "/ab"));
        // `{` always opens a parameter, so a lone one is §8.3's unclosed row.
        assert_eq!(
            refused("/a{b"),
            "3:7: the parameter `{b` is not closed; write `{id:int}`"
        );
        // The raw form keeps a literal brace, because nothing scans it.
        assert!(matched_regex("/a\\{b", "/a{b"));
    }

    /// §8.3's refusal matrix: every row, with its sentence **and** its
    /// `line:col`.
    #[test]
    fn the_refusal_matrix() {
        for (pattern, message) in [
            (
                "/x/{id:int",
                "the parameter `{id:int` is not closed; write `{id:int}`",
            ),
            (
                "/x/{}",
                "`{}` has no name; write `{id}`, `{id:int}` or `{id:uuid}`",
            ),
            (
                "/x/{Id}",
                "`{Id}` is not a parameter name; names start with a lower-case letter and are \
                 `[a-z][a-z0-9_]*`",
            ),
            (
                "/x/{id:str}",
                "`{id:str}` is not a pattern type; the types are `int` and `uuid`",
            ),
            (
                "/x/*/y",
                "`*` is only valid at the end of a pattern; write `/x/*`",
            ),
            (
                "/assets*",
                "`*` is only valid at the end of a pattern; write `/x/*`",
            ),
            (
                "/x/{a}/{a:int}",
                "the parameter `{a}` appears twice; a pattern names each parameter once",
            ),
            ("x", "a pattern starts with `/`: write `/x`"),
            ("", "a pattern starts with `/`: write `/x`"),
        ] {
            assert_eq!(refused(pattern), format!("3:7: {message}"), "{pattern:?}");
        }
    }

    /// The two rows §8.3 does not spell out but the same sentences cover: an
    /// empty type and a name that starts with a digit.
    #[test]
    fn a_parameter_is_validated_before_it_is_emitted() {
        assert_eq!(
            refused("/x/{id:}"),
            "3:7: `{id:}` is not a pattern type; the types are `int` and `uuid`"
        );
        assert_eq!(
            refused("/x/{1id}"),
            "3:7: `{1id}` is not a parameter name; names start with a lower-case letter and are \
             `[a-z][a-z0-9_]*`"
        );
        assert_eq!(
            refused("/x/{a-b}"),
            "3:7: `{a-b}` is not a parameter name; names start with a lower-case letter and are \
             `[a-z][a-z0-9_]*`"
        );
        // A parameter with no closing brace at the end of the pattern.
        assert_eq!(
            refused("/x/{"),
            "3:7: the parameter `{` is not closed; write `{id:int}`"
        );
        // The first `}` closes the parameter, so the name is what is between.
        assert_eq!(
            refused("/x/{a{b}"),
            "3:7: `{a{b}` is not a parameter name; names start with a lower-case letter and are \
             `[a-z][a-z0-9_]*`"
        );
    }

    /// §4.5's cap, in bytes, for both spellings — and 256 bytes is still legal.
    #[test]
    fn a_pattern_holds_at_most_256_bytes() {
        let at_cap = format!("/{}", "a".repeat(MAX_PATTERN_BYTES - 1));
        assert_eq!(at_cap.len(), MAX_PATTERN_BYTES);
        assert_eq!(fragment(&at_cap).len(), MAX_PATTERN_BYTES);
        assert_eq!(
            compile_regex(&at_cap, AT).expect("at the cap").fragment(),
            at_cap,
            "the raw form is capped at the same 256 bytes"
        );

        let over = format!("/{}", "a".repeat(MAX_PATTERN_BYTES));
        assert_eq!(over.len(), MAX_PATTERN_BYTES + 1);
        assert_eq!(refused(&over), "3:7: a pattern holds at most 256 bytes");
        assert_eq!(
            compile_regex(&over, AT)
                .expect_err("over the cap")
                .to_string(),
            "3:7: a pattern holds at most 256 bytes"
        );

        // The cap is bytes, not characters: 127 two-byte characters after
        // `/x` are 256 bytes and legal, 128 are not.
        let two_byte = format!("/x{}", "é".repeat(127));
        assert_eq!(two_byte.len(), MAX_PATTERN_BYTES);
        assert!(compile(&two_byte, AT).is_ok());
        let over_two_byte = format!("/x{}", "é".repeat(128));
        assert_eq!(over_two_byte.len(), MAX_PATTERN_BYTES + 2);
        assert_eq!(
            compile(&over_two_byte, AT)
                .expect_err("over the cap")
                .to_string(),
            "3:7: a pattern holds at most 256 bytes"
        );
    }

    /// A `regex` is kept verbatim and compiled as written; one the crate
    /// refuses is a clean refusal naming the crate's own message, never a
    /// panic (§8.3, §17.4).
    #[test]
    fn a_regex_the_crate_refuses_is_a_clean_refusal() {
        assert_eq!(
            compile_regex("/v[0-9]+/items", AT)
                .expect("valid")
                .fragment(),
            "/v[0-9]+/items",
            "the text is kept verbatim"
        );
        assert!(matched_regex("/v[0-9]+/items", "/v12/items"));
        assert!(!matched_regex("/v[0-9]+/items", "/v12/items/3"));
        assert!(!matched_regex("/v[0-9]+/items", "/vx/items"));

        // Look-around is not in the `regex` crate (§17.4).
        let look_around = compile_regex("/a(?=b)", AT)
            .expect_err("look-around is refused")
            .to_string();
        assert!(
            look_around.starts_with("3:7: regex `/a(?=b)` does not compile: "),
            "{look_around}"
        );
        assert!(look_around.contains("look-around"), "{look_around}");

        // An unbalanced `)` is refused before the wrapper: `^(?:x)|(.*)$`
        // would be a catch-all, which is the whole reason for the double
        // compile (§8.1).
        let unbalanced = compile_regex("x)|(.*", AT)
            .expect_err("unbalanced")
            .to_string();
        assert!(
            unbalanced.starts_with("3:7: regex `x)|(.*` does not compile: "),
            "{unbalanced}"
        );
        for pattern in ["a)|(b", ")", "(", "a)b(c"] {
            assert!(
                compile_regex(pattern, AT).is_err(),
                "{pattern:?} must not compile"
            );
        }

        // The raw form is not scanned for parameters or a trailing `*`: the
        // crate reads it, so `{2}` is a quantifier and `*` a repeat.
        assert_eq!(
            compile_regex("/v[0-9]{2}", AT).expect("valid").fragment(),
            "/v[0-9]{2}"
        );
        assert!(matched_regex("/v[0-9]{2}", "/v42"));
        assert!(!matched_regex("/v[0-9]{2}", "/v4"));
    }

    /// §8.4's table, exactly: written order kept, `HEAD` after its `GET`,
    /// duplicates deduplicated.
    #[test]
    fn the_method_table() {
        let normalised = |written: &[&str]| {
            let pairs: Vec<(&str, Position)> = written
                .iter()
                .map(|method| (*method, Position::at(2, 9)))
                .collect();
            normalise_methods(&pairs).expect("the list is legal")
        };
        let list = |methods: &[&str]| Some(methods.iter().map(|one| (*one).to_owned()).collect());

        // No method at all is `None`: any method the front end lets through.
        assert_eq!(normalised(&[]), None);
        // `GET` gains `HEAD`.
        assert_eq!(normalised(&["GET"]), list(&["GET", "HEAD"]));
        // `GET, HEAD` is the same list.
        assert_eq!(normalised(&["GET", "HEAD"]), list(&["GET", "HEAD"]));
        // `POST` keeps exactly what it declared.
        assert_eq!(normalised(&["POST"]), list(&["POST"]));
        // `GET, POST` puts `HEAD` after its `GET`.
        assert_eq!(normalised(&["GET", "POST"]), list(&["GET", "HEAD", "POST"]));
        // Written order, not a canonical order.
        assert_eq!(normalised(&["POST", "GET"]), list(&["POST", "GET", "HEAD"]));
        assert_eq!(
            normalised(&["DELETE", "GET"]),
            list(&["DELETE", "GET", "HEAD"])
        );
        // A duplicate is deduplicated, not an error.
        assert_eq!(normalised(&["GET", "GET"]), list(&["GET", "HEAD"]));
        assert_eq!(
            normalised(&["GET", "HEAD", "GET", "HEAD"]),
            list(&["GET", "HEAD"])
        );
        assert_eq!(normalised(&["PUT", "PUT"]), list(&["PUT"]));
        // `HEAD` on its own is not implied by anything and stays as written.
        assert_eq!(normalised(&["HEAD"]), list(&["HEAD"]));
        // `HEAD` written before its `GET` keeps its place; it is not duplicated.
        assert_eq!(normalised(&["HEAD", "GET"]), list(&["HEAD", "GET"]));
    }

    /// `CONNECT` and `TRACE` cannot reach a view (§8.4), at the method's own
    /// position.
    #[test]
    fn connect_and_trace_cannot_reach_a_view() {
        for method in ["CONNECT", "TRACE"] {
            let written = [(method, Position::at(5, 3))];
            let refusal = normalise_methods(&written)
                .expect_err("the method is refused")
                .to_string();
            assert_eq!(
                refusal,
                format!(
                    "5:3: `{method}` cannot reach a view; Blastit answers `GET`/`HEAD` static \
                     files and forwards the methods you declare"
                )
            );
            // The refusal wins over the rest of the list, whatever it holds.
            let written = [("GET", Position::at(5, 3)), (method, Position::at(5, 12))];
            let refusal = normalise_methods(&written)
                .expect_err("the method is refused")
                .to_string();
            assert!(refusal.starts_with("5:12: "), "{refusal}");
        }
    }

    /// A refusal renders on one line with the position first, and a control
    /// byte in a pattern cannot break it (§18).
    #[test]
    fn a_refusal_is_one_line() {
        let refusal = compile("/x/{\u{1}}", AT).expect_err("refused");
        assert!(
            matches!(refusal, ManifestError::PatternParameterName { at, .. } if at == AT),
            "{refusal}"
        );
        let rendered = refusal.to_string();
        assert!(rendered.starts_with("3:7: "), "{rendered}");
        assert!(!rendered.contains('\u{1}'), "{rendered}");
        assert!(rendered.contains("\\u{1}"), "{rendered}");
        // A long parameter is cut, so a hostile pattern cannot flood a log.
        let long = format!("/x/{{{}}}", "A".repeat(200));
        let refusal = compile(&long, AT).expect_err("refused").to_string();
        assert!(refusal.contains('…'), "{refusal}");
        assert!(refusal.chars().count() < 240, "{refusal}");
        // The cut value is 120 characters, as every other value is (§18).
        assert!(!refusal.contains(&"A".repeat(121)), "{refusal}");
    }

    /// The one helper that is not a table: the `}` that closes a parameter is
    /// the first one after it, and `None` when there is none.
    #[test]
    fn the_parameter_ends_at_the_first_close_brace() {
        // `/x/{id}`: the `{` is at byte 3, the `}` at byte 6.
        assert_eq!(parameter_end("/x/{id}", 3), Some(6));
        assert_eq!(parameter_end("/x/{a{b}", 3), Some(7));
        assert_eq!(parameter_end("/x/{id:int", 3), None);
        assert_eq!(parameter_end("/x/", 3), None);
        assert_eq!(parameter_end("/x/", 99), None);
    }

    /// Every refusal is the `ManifestError` variant of its own row, carrying
    /// the position and the value the sentence quotes — so a caller never has
    /// to parse a message back into an error.
    #[test]
    fn a_refusal_is_the_variant_of_its_row() {
        assert_eq!(
            compile("x", AT).expect_err("refused"),
            ManifestError::PatternNotAnchored { at: AT }
        );
        assert_eq!(
            compile("/x/{Id}", AT).expect_err("refused"),
            ManifestError::PatternParameterName {
                at: AT,
                parameter: "Id".to_owned(),
            }
        );
        assert_eq!(
            compile("/x/{id:str}", AT).expect_err("refused"),
            ManifestError::PatternParameterType {
                at: AT,
                parameter: "id:str".to_owned(),
            }
        );
        assert_eq!(
            compile("/x/{a}/{a:int}", AT).expect_err("refused"),
            ManifestError::PatternParameterDuplicate {
                at: AT,
                name: "a".to_owned(),
            }
        );
        assert_eq!(
            compile("/x/{id:int", AT).expect_err("refused"),
            ManifestError::PatternParameterUnclosed {
                at: AT,
                parameter: "{id:int".to_owned(),
            }
        );
        assert_eq!(
            compile("/x/{}", AT).expect_err("refused"),
            ManifestError::PatternParameterUnnamed { at: AT }
        );
        assert_eq!(
            compile("/x/*/y", AT).expect_err("refused"),
            ManifestError::PatternStarPosition { at: AT }
        );
        let over = format!("/{}", "a".repeat(MAX_PATTERN_BYTES));
        assert_eq!(
            compile(&over, AT).expect_err("refused"),
            ManifestError::PatternTooLong { at: AT }
        );
        assert!(matches!(
            compile_regex("/a(?=b)", AT).expect_err("refused"),
            ManifestError::PatternRegex { at, .. } if at == AT
        ));
        assert_eq!(
            normalise_methods(&[("TRACE", AT)]).expect_err("refused"),
            ManifestError::MethodCannotReachView {
                at: AT,
                method: "TRACE".to_owned(),
            }
        );
    }

    /// §18's position-less rule: a refusal whose position is `NONE` renders the
    /// sentence alone, so a caller that prefixes the path still gets one line.
    #[test]
    fn a_positionless_refusal_renders_bare() {
        for (error, message) in [
            (
                ManifestError::PatternNotAnchored { at: Position::NONE },
                "a pattern starts with `/`: write `/x`",
            ),
            (
                ManifestError::PatternTooLong { at: Position::NONE },
                "a pattern holds at most 256 bytes",
            ),
            (
                ManifestError::PatternStarPosition { at: Position::NONE },
                "`*` is only valid at the end of a pattern; write `/x/*`",
            ),
            (
                ManifestError::PatternParameterUnnamed { at: Position::NONE },
                "`{}` has no name; write `{id}`, `{id:int}` or `{id:uuid}`",
            ),
        ] {
            assert_eq!(error.to_string(), message);
        }
    }

    /// A compiled pattern keeps both halves: the value it was compiled from and
    /// the fragment it emits, so `lower` reads one and never recompiles.
    #[test]
    fn a_compiled_pattern_keeps_its_text_and_its_fragment() {
        let compiled = compile("/products/{id:int}", AT).expect("valid");
        assert_eq!(compiled.text(), "/products/{id:int}");
        assert_eq!(compiled.fragment(), "/products/[0-9]+");
        // The raw form keeps its text byte for byte, backslashes included.
        let raw = compile_regex(r"/v\d+", AT).expect("valid");
        assert_eq!(raw.text(), r"/v\d+");
        assert_eq!(raw.fragment(), r"/v\d+");
    }
}
