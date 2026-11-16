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

/// The mount a request path falls under, and the path below it (§9.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MountMatch<'a> {
    /// Index of the mount in [`Manifest::mounts`].
    pub mount: usize,
    /// The request path with the mount's prefix removed.
    ///
    /// Empty when the request is exactly the prefix, `/`-anchored otherwise, so
    /// `root` + `relative` names the candidate path (§9.2). For the root mount
    /// it is the whole request path, which is what §9.4's `url_path` rebuild
    /// expects.
    pub relative: &'a str,
}

/// One mount's prefix, with the index of the mount it came from.
#[derive(Debug)]
struct CompiledMount {
    prefix: String,
    mount: usize,
}

/// The manifest's static mounts, ready to be matched against a request path
/// (§9.2).
///
/// Mounts are matched on a **segment boundary** — `"/assets"` matches
/// `/assets` and `/assets/x`, never `/assets-secret` — and the longest matching
/// prefix wins, so a nested `static "/assets/v2"` beats `static "/assets"`.
/// A prefix that is not a `/`-anchored path matches nothing, so a malformed
/// prefix can only leave a request to the views; the prefix's shape rules are
/// part of validation (§9.1).
#[derive(Debug)]
pub struct MountTable {
    mounts: Vec<CompiledMount>,
}

impl MountTable {
    /// Builds the mount table from a manifest, in mount order.
    ///
    /// Infallible: a mount prefix is a string, not a pattern, so there is
    /// nothing to compile and nothing to refuse here.
    pub fn new(manifest: &Manifest) -> Self {
        let mounts = manifest
            .mounts()
            .iter()
            .enumerate()
            .map(|(mount, entry)| CompiledMount {
                prefix: entry.prefix.clone(),
                mount,
            })
            .collect();
        Self { mounts }
    }

    /// The mount that answers a request path, longest prefix first (§9.2).
    ///
    /// `None` means no mount claims the path, and the view table is consulted.
    /// Two mounts with the same prefix are a validation error
    /// (`DuplicateMount`); the first in mount order answers here.
    ///
    /// # Examples
    ///
    /// ```
    /// use blastit_manifest::{Manifest, MountTable};
    ///
    /// let manifest = Manifest::from_toml_str(
    ///     "version = 1\n\
    ///      [[route]]\nname = \"site\"\npath = ['.*']\n\
    ///      [route.static]\nroot = \"public\"\n",
    /// )
    /// .unwrap();
    /// let table = MountTable::new(&manifest);
    ///
    /// // The retired reader's static route is a mount at the URL root.
    /// let found = table.lookup("/assets/app.js").unwrap();
    /// assert_eq!((found.mount, found.relative), (0, "/assets/app.js"));
    /// assert!(table.lookup("assets/app.js").is_none());
    /// ```
    pub fn lookup<'a>(&self, path: &'a str) -> Option<MountMatch<'a>> {
        let mut best: Option<(usize, MountMatch<'a>)> = None;
        for mount in &self.mounts {
            let Some(relative) = strip_prefix(&mount.prefix, path) else {
                continue;
            };
            if best.is_none_or(|(longest, _)| mount.prefix.len() > longest) {
                best = Some((
                    mount.prefix.len(),
                    MountMatch {
                        mount: mount.mount,
                        relative,
                    },
                ));
            }
        }
        best.map(|(_, found)| found)
    }
}

/// The request path below `prefix`, or `None` when the path is not under it.
///
/// The match is on a segment boundary: after the prefix the path has to end or
/// continue with `/`, so `"/assets"` never matches `/assets-secret`. A request
/// that is exactly the prefix has an empty remainder. The root mount keeps the
/// whole path, so `relative` is always `/`-anchored for it (§9.4). A prefix that
/// is not a `/`-anchored path matches nothing.
fn strip_prefix<'a>(prefix: &str, path: &'a str) -> Option<&'a str> {
    if !prefix.starts_with('/') {
        return None;
    }
    if prefix == "/" {
        return path.starts_with('/').then_some(path);
    }
    if path == prefix {
        return Some("");
    }
    path.strip_prefix(prefix)
        .filter(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::collections::BTreeMap;

    use super::{MountMatch, MountTable, compile_pattern};
    use crate::model::{Manifest, StaticMount, StaticPolicy};

    /// A manifest with the given mount prefixes, in order, and no routes: a
    /// static-only file (§9.5).
    fn mounted(prefixes: &[&str]) -> Manifest {
        let mounts = prefixes
            .iter()
            .enumerate()
            .map(|(index, prefix)| StaticMount {
                prefix: (*prefix).to_string(),
                policy: StaticPolicy {
                    root: format!("public{index}"),
                    index: "index.html".to_string(),
                    spa: false,
                    immutable: Vec::new(),
                },
            })
            .collect();
        Manifest::assemble(
            1,
            Vec::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
        )
        .with_mounts(mounts)
    }

    /// A mount answers its prefix and the paths below it, and nothing that only
    /// shares the prefix's characters: `/assets` is not a prefix of
    /// `/assets-secret` (§9.2).
    #[test]
    fn a_mount_matches_on_a_segment_boundary_only() {
        let table = MountTable::new(&mounted(&["/assets"]));
        assert_eq!(
            table.lookup("/assets"),
            Some(MountMatch {
                mount: 0,
                relative: ""
            })
        );
        assert_eq!(
            table.lookup("/assets/app.js"),
            Some(MountMatch {
                mount: 0,
                relative: "/app.js"
            })
        );
        for path in [
            "/assets-secret",
            "/assets-secret/key.pem",
            "/other",
            "assets",
        ] {
            assert_eq!(table.lookup(path), None, "{path} must not match /assets");
        }

        // The other direction: the longer word's mount does not answer the
        // shorter path either.
        let table = MountTable::new(&mounted(&["/assets-secret"]));
        assert_eq!(table.lookup("/assets"), None);
        assert_eq!(table.lookup("/assets/secret"), None);
        assert_eq!(
            table.lookup("/assets-secret"),
            Some(MountMatch {
                mount: 0,
                relative: ""
            })
        );
    }

    /// A nested mount beats its parent, and the manifest's order does not
    /// decide: the longest prefix does (§9.2).
    #[test]
    fn the_longest_prefix_wins() {
        let table = MountTable::new(&mounted(&["/assets", "/assets/v2", "/assets-secret"]));
        assert_eq!(
            table.lookup("/assets/v2/app.js").map(|found| found.mount),
            Some(1)
        );
        assert_eq!(
            table
                .lookup("/assets/v2")
                .map(|found| (found.mount, found.relative)),
            Some((1, ""))
        );
        assert_eq!(
            table.lookup("/assets/v3/app.js").map(|found| found.mount),
            Some(0)
        );
        assert_eq!(
            table.lookup("/assets-secret/key").map(|found| found.mount),
            Some(2)
        );
        assert_eq!(table.lookup("/assets").map(|found| found.mount), Some(0));

        // The same table written the other way round answers the same mounts.
        let reversed = MountTable::new(&mounted(&["/assets/v2", "/assets"]));
        assert_eq!(
            reversed
                .lookup("/assets/v2/app.js")
                .map(|found| found.mount),
            Some(0)
        );
        assert_eq!(
            reversed.lookup("/assets/app.js").map(|found| found.mount),
            Some(1)
        );
    }

    /// The root mount keeps the whole path, so §9.4's `url_path` rebuild is the
    /// identity for it; a nested mount still beats it (§9.4, §9.2).
    #[test]
    fn the_root_mount_keeps_the_whole_path() {
        let table = MountTable::new(&mounted(&["/"]));
        assert_eq!(table.lookup("/").map(|found| found.relative), Some("/"));
        assert_eq!(
            table.lookup("/a/b").map(|found| found.relative),
            Some("/a/b")
        );
        assert_eq!(table.lookup(""), None);

        let nested = MountTable::new(&mounted(&["/", "/assets"]));
        assert_eq!(nested.lookup("/assets/x").map(|found| found.mount), Some(1));
        assert_eq!(nested.lookup("/about").map(|found| found.mount), Some(0));
    }

    /// A prefix that is not a `/`-anchored path matches nothing, so a mount the
    /// validator would refuse can only leave the request to the views.
    #[test]
    fn a_malformed_prefix_matches_nothing() {
        for prefix in ["assets", "", "/assets/"] {
            let table = MountTable::new(&mounted(&[prefix]));
            assert_eq!(table.lookup("/assets"), None, "{prefix:?}");
            assert_eq!(table.lookup("/assets/app.js"), None, "{prefix:?}");
        }
    }

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
