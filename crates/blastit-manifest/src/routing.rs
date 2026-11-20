//! The compiled routing table (§2.4, §6).

use regex::{Regex, RegexBuilder};

use crate::error::{ManifestError, ManifestErrors, regex_message};
use crate::model::{Manifest, StaticMount};

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
    ///
    /// [`RoutingTable::for_learn`] shifts every index by one, because its slot
    /// `0` is the reserved fallback: there, this is the manifest index `+ 1`, so
    /// a caller reading a `for_learn` table must not pair it with
    /// [`Manifest::route`] directly.
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

    /// Compiles the manifest's routing table for `blastit dev --learn`, with
    /// slot `0` reserved for the unrouted-request fallback (`R26`, `R27`).
    ///
    /// Slot `0` is the scheduler's fallback view: the slot a request no view
    /// matched is served by. It holds **no** patterns, so
    /// [`RoutingTable::route`] can never select it and
    /// [`RoutingTable::methods_for`] never takes a method from it — the
    /// reservation can only be reached through the scheduler's learn path,
    /// never by a declared request. Every declared route therefore sits at
    /// `1..=N`, in manifest order, with exactly the methods and compiled
    /// patterns [`RoutingTable::new`] builds.
    ///
    /// # Errors
    ///
    /// Every path pattern [`RoutingTable::new`] refuses, with the same
    /// [`ManifestError::InvalidRegex`].
    ///
    /// # Examples
    ///
    /// ```
    /// use blastit_manifest::{Manifest, Match, RoutingTable};
    ///
    /// let manifest = Manifest::from_blastfile_str(
    ///     "view about \"/about/\" {}\n\nview products \"/products/\" {}\n",
    /// )
    /// .unwrap();
    /// let table = RoutingTable::for_learn(&manifest).unwrap();
    ///
    /// // Slot 0 is the fallback, so the declared views are routes 1 and 2.
    /// assert_eq!(table.route("GET", "/about/"), Some(Match { route: 1, rule: 0 }));
    /// assert_eq!(table.route("GET", "/products/"), Some(Match { route: 2, rule: 0 }));
    /// // Nothing routes into slot 0, and it contributes no `405` methods.
    /// assert_eq!(table.route("GET", "/unrouted"), None);
    /// assert_eq!(table.methods_for("/unrouted"), None);
    /// ```
    pub fn for_learn(manifest: &Manifest) -> Result<Self, ManifestErrors> {
        let mut table = Self::new(manifest)?;
        // One reservation in front of the compiled table: every declared route
        // shifts by exactly one, which is the index the scheduler's learn path
        // expects.
        table.routes.insert(
            0,
            CompiledRoute {
                methods: None,
                rules: Vec::new(),
            },
        );
        Ok(table)
    }

    /// Matches a request against the table, in manifest order: the first route
    /// whose methods (when it lists them) and whose first matching pattern
    /// accept it wins (§6).
    ///
    /// A route that lists methods answers exactly those, compared as exact
    /// strings; `HEAD` is not implied by `GET` **here**. The implication
    /// happened at lower time (§8.4): a `Blastfile` view that declares `GET`
    /// arrives with `["GET", "HEAD"]` already in its list, so this table
    /// answers both without a second rule and the matching order does not
    /// change.
    ///
    /// # Examples
    ///
    /// ```
    /// use blastit_manifest::{Manifest, Match, RoutingTable};
    ///
    /// let manifest = Manifest::from_blastfile_str(
    ///     "view about \"/about/\" {}\n\nview fallback \"/*\" {}\n",
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

    /// The methods of every route whose **path** matched, for a `405` (§9.2
    /// step 4).
    ///
    /// [`RoutingTable::route`] answers `None` when no route accepts the method,
    /// which is a `404` for a target no view's pattern matched either. A request
    /// whose path matched a view that declares methods but not this one is not
    /// unrouted: the design answers `405` with the union of those views'
    /// declared methods, in declaration order. `None` means no view's pattern
    /// matched at all.
    pub fn methods_for(&self, path_info: &str) -> Option<Vec<String>> {
        let mut methods: Vec<String> = Vec::new();
        for compiled in &self.routes {
            if !compiled.rules.iter().any(|rule| rule.is_match(path_info)) {
                continue;
            }
            let Some(declared) = &compiled.methods else {
                // A view with no `methods` answers every method, so `route`
                // would have matched it; it contributes nothing here.
                continue;
            };
            for method in declared {
                if !methods.iter().any(|known| known == method) {
                    methods.push(method.clone());
                }
            }
        }
        (!methods.is_empty()).then_some(methods)
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
        Self::from_mounts(manifest.mounts())
    }

    /// Builds the mount table from a mount list, in mount order.
    ///
    /// The scheduler is handed the mounts separately from the routing table (a
    /// mount owns no view), so this is the entry point it uses.
    pub fn from_mounts(mounts: &[StaticMount]) -> Self {
        let mounts = mounts
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
    /// let manifest = Manifest::from_blastfile_str(
    ///     "static \"/\" {\n    root \"public\"\n}\n",
    /// )
    /// .unwrap();
    /// let table = MountTable::new(&manifest);
    ///
    /// // The root mount is a mount at the URL root.
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
