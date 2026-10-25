//! The compiled routing table (§2.4, §6).

use regex::{Regex, RegexBuilder};

use crate::error::{ManifestError, ManifestErrors, regex_message};
use crate::model::Manifest;

/// Largest compiled size of one path regex.
///
/// Path patterns come from a file in the image, not from a request, but a
/// bounded compile keeps a pathological pattern from ballooning the scheduler.
pub const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// Deepest nesting of groups in one path regex.
///
/// A path pattern has no business being deeply nested, and bounding it keeps a
/// pathological one from making the parser work hard.
pub const REGEX_NEST_LIMIT: u32 = 64;

/// A regex builder with the limits blastit applies to manifest regexes.
fn builder(pattern: &str) -> RegexBuilder {
    let mut builder = RegexBuilder::new(pattern);
    builder.size_limit(REGEX_SIZE_LIMIT);
    builder.dfa_size_limit(REGEX_SIZE_LIMIT);
    builder.nest_limit(REGEX_NEST_LIMIT);
    builder
}

/// Compiles a path pattern anchored to a full match (§6).
///
/// The pattern is compiled twice. First on its own: wrapping a pattern in a
/// group rescues a malformed one — it makes `x)|(.*` a valid, and dangerous,
/// catch-all — so an unbalanced pattern has to be rejected before it is
/// wrapped. Then as `^(?:pattern)$`, so it has to match the whole of
/// `PATH_INFO` rather than any part of it.
pub(crate) fn compile_pattern(pattern: &str) -> Result<Regex, regex::Error> {
    builder(pattern).build()?;
    builder(&format!("^(?:{pattern})$")).build()
}

/// The routing decision: which route, and which of its path rules, matched
/// (§8.1 `request.rule`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Match {
    /// Index of the route in [`Manifest::routes`].
    pub route: usize,
    /// Index of the matching pattern in that route's `path` list.
    pub rule: usize,
}

/// One route's compiled patterns.
#[derive(Debug)]
struct CompiledRoute {
    methods: Option<Vec<String>>,
    rules: Vec<Regex>,
}

/// The manifest's routes, with their path regexes compiled (§2.4).
#[derive(Debug)]
pub struct RoutingTable {
    routes: Vec<CompiledRoute>,
}

impl RoutingTable {
    /// Compiles the manifest's routing table.
    ///
    /// Every pattern has to compile, so a manifest that is not
    /// [`validated`](Manifest::validate) first still fails here rather than
    /// routing requests by accident.
    pub fn new(manifest: &Manifest) -> Result<Self, ManifestErrors> {
        let mut errors = Vec::new();
        let mut routes = Vec::with_capacity(manifest.route_count());
        for route in manifest.routes() {
            let mut rules = Vec::with_capacity(route.path().len());
            for (rule, pattern) in route.path().iter().enumerate() {
                match compile_pattern(pattern) {
                    Ok(regex) => rules.push(regex),
                    Err(error) => errors.push(ManifestError::InvalidRegex {
                        route: route.name().to_string(),
                        rule,
                        pattern: pattern.clone(),
                        message: regex_message(&error),
                    }),
                }
            }
            routes.push(CompiledRoute {
                methods: route.methods().map(<[String]>::to_vec),
                rules,
            });
        }

        if errors.is_empty() {
            Ok(Self { routes })
        } else {
            Err(ManifestErrors::from(errors))
        }
    }

    /// Matches a request against the table, in manifest order: the first route
    /// whose methods (when it lists them) and whose first matching pattern
    /// accept it wins (§6).
    ///
    /// A route that lists methods answers exactly those; `HEAD` is not implied
    /// by `GET`.
    ///
    /// # Examples
    ///
    /// ```
    /// use blastit_manifest::{Manifest, Match, RoutingTable};
    ///
    /// let manifest = Manifest::from_toml_str(
    ///     "version = 1\n\
    ///      [[route]]\nname = \"about\"\npath = ['/about/']\n\
    ///      [[route]]\nname = \"fallback\"\npath = ['.*']\n",
    /// )
    /// .unwrap();
    /// let table = RoutingTable::new(&manifest).unwrap();
    ///
    /// assert_eq!(table.route("GET", "/about/"), Some(Match { route: 0, rule: 0 }));
    /// assert_eq!(table.route("GET", "/about/x"), Some(Match { route: 1, rule: 0 }));
    /// ```
    pub fn route(&self, method: &str, path_info: &str) -> Option<Match> {
        for (route, compiled) in self.routes.iter().enumerate() {
            if let Some(methods) = &compiled.methods
                && !methods.iter().any(|listed| listed == method)
            {
                continue;
            }
            for (rule, pattern) in compiled.rules.iter().enumerate() {
                if pattern.is_match(path_info) {
                    return Some(Match { route, rule });
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::compile_pattern;

    #[test]
    fn patterns_match_whole_paths() {
        let about = compile_pattern("/about/").expect("valid");
        assert!(about.is_match("/about/"));
        assert!(!about.is_match("/about/x"));
        assert!(!about.is_match("/prefix/about/"));

        let catch_all = compile_pattern(".*").expect("valid");
        assert!(catch_all.is_match(""));
        assert!(catch_all.is_match("/anything/at/all"));
    }

    #[test]
    fn alternatives_stay_inside_the_wrapper() {
        // Without the non-capturing group, `a|ab` would match `a` and stop.
        let pattern = compile_pattern("a|ab").expect("valid");
        assert!(pattern.is_match("ab"));
        assert!(pattern.is_match("a"));
        assert!(!pattern.is_match("aba"));

        let grouped = compile_pattern("(a)").expect("valid");
        assert!(grouped.is_match("a"));
        assert!(!grouped.is_match("aa"));
    }

    #[test]
    fn unbalanced_patterns_are_rejected_before_they_are_wrapped() {
        // `x)|(.*` would become `^(?:x)|(.*)$`: a catch-all. `a)|(b` would
        // match a prefix or a suffix. Both are malformed on their own.
        for pattern in ["x)|(.*", "a)|(b", ")", "(", "a)b(c"] {
            assert!(
                compile_pattern(pattern).is_err(),
                "{pattern:?} must not compile"
            );
        }
    }

    #[test]
    fn inline_flags_cannot_widen_the_anchor() {
        // Flag groups are scoped to the group they are written in, so `(?m)`
        // inside the wrapper cannot turn the trailing `$` into a line anchor.
        let pattern = compile_pattern("(?m).*a").expect("valid");
        assert!(pattern.is_match("a"));
        assert!(pattern.is_match("ba"));
        assert!(!pattern.is_match("a\n"));
        assert!(!pattern.is_match("b\na"));
    }

    #[test]
    fn deeply_nested_patterns_are_rejected() {
        let deep = format!("{}a{}", "(?:".repeat(200), ")".repeat(200));
        let error = compile_pattern(&deep).expect_err("nesting is bounded");
        assert!(error.to_string().contains("nested parentheses"), "{error}");
    }
}
