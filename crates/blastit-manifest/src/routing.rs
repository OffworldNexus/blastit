//! The compiled routing table (§2.4, §6).

use regex::{Regex, RegexBuilder};

use crate::error::{ManifestError, ManifestErrors};
use crate::model::Manifest;

/// Largest compiled size of one path regex.
///
/// Path patterns come from a file in the image, not from a request, but a
/// bounded compile keeps a pathological pattern from ballooning the scheduler.
pub const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// Compiles a path pattern anchored to a full match (§6).
///
/// The manifest's regexes carry no anchors of their own: `path = ['.*']` is a
/// catch-all, and `path = ['/about/']` matches `/about/` and nothing else.
pub(crate) fn compile_pattern(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(&format!("^(?:{pattern})$"))
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_SIZE_LIMIT)
        .build()
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
    rules: Vec<Option<Regex>>,
}

/// The manifest's routes, with their path regexes compiled (§2.4).
#[derive(Debug)]
pub struct RoutingTable {
    routes: Vec<CompiledRoute>,
}

impl RoutingTable {
    /// Compiles the manifest's routing table.
    ///
    /// A pattern that does not compile is skipped, so it can never match:
    /// [`Manifest::validate`] rejects such a manifest before the scheduler ever
    /// gets here, and [`RoutingTable::try_new`] reports the failures instead.
    pub fn new(manifest: &Manifest) -> Self {
        Self::compile(manifest, &mut Vec::new())
    }

    /// Compiles the manifest's routing table, reporting patterns that do not
    /// compile.
    pub fn try_new(manifest: &Manifest) -> Result<Self, ManifestErrors> {
        let mut errors = Vec::new();
        let table = Self::compile(manifest, &mut errors);
        if errors.is_empty() {
            Ok(table)
        } else {
            Err(ManifestErrors::from(errors))
        }
    }

    /// The compiled table, collecting compile failures.
    fn compile(manifest: &Manifest, errors: &mut Vec<ManifestError>) -> Self {
        let routes = manifest
            .routes()
            .iter()
            .map(|route| {
                let rules = route
                    .path()
                    .iter()
                    .enumerate()
                    .map(|(rule, pattern)| match compile_pattern(pattern) {
                        Ok(regex) => Some(regex),
                        Err(error) => {
                            errors.push(ManifestError::InvalidRegex {
                                route: route.name().to_string(),
                                rule,
                                pattern: pattern.clone(),
                                message: error.to_string(),
                            });
                            None
                        }
                    })
                    .collect();
                CompiledRoute {
                    methods: route.methods().map(<[String]>::to_vec),
                    rules,
                }
            })
            .collect();
        Self { routes }
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
    /// let table = RoutingTable::try_new(&manifest).unwrap();
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
                if pattern
                    .as_ref()
                    .is_some_and(|pattern| pattern.is_match(path_info))
                {
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
}
