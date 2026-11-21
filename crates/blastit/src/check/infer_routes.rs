//! Route inference for `--learn` (`R28`, `ADR-0003`).
//!
//! A request no `view` matched is what `--learn` exists to explain: the path
//! names the view a human forgot to declare. This module turns those paths into
//! the views the `Blastfile` should hold, using one rule set for both
//! `blastit dev --learn` and `blastit check --learn`:
//!
//! * the **name** is the first non-empty path segment, lowercased, with every
//!   run of characters outside `[a-z0-9_-]` collapsed to a single `-`, and
//!   both leading and trailing characters outside `[a-z0-9]` trimmed (`/café/`
//!   is `caf`, not `caf-`); an empty result is `route`;
//! * `/` itself is `root`;
//! * a purely numeric segment becomes `[0-9]+`, a UUID-shaped one `[^/]+`, and
//!   every other byte is regex-escaped so `.` and `+` in a path stay literal;
//! * two paths that infer the same name share **one** view (the fixture
//!   `/products/` + `/products/42/` is one view with two patterns), while a
//!   name already taken becomes `<name>-2`, `<name>-3`, …;
//! * the insertion point is the first method-unrestricted catch-all
//!   ([`blastit_manifest::Route::is_unrestricted_catch_all`], which matches the lowered `/.*`
//!   and `.*` fragments alike), or the end of the file.
//!
//! [`super::infer_routes::infer`] is pure: it reads a [`blastit_manifest::Manifest`] and the paths, and returns a
//! [`super::infer_routes::RoutePlan`]. Writing the plan into a document belongs to
//! `blastit_manifest::edit` (`R29`), which owns the comment-preserving splice.

use std::collections::{BTreeMap, BTreeSet};

use blastit_manifest::{Manifest, Route};

/// One `view` a run of unrouted requests proposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferredRoute {
    /// The route name, unique in the manifest once the plan is applied.
    pub name: String,
    /// The path regexes, in sorted order.
    pub paths: Vec<String>,
}

/// Everything route inference derived from the unrouted requests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutePlan {
    /// The proposed routes, in name order.
    pub routes: Vec<InferredRoute>,
    /// Which proposed route each observed path belongs to.
    pub by_path: BTreeMap<String, String>,
    /// Where the proposals go: the first unrestricted catch-all, or the end.
    pub insert_at: usize,
}

impl RoutePlan {
    /// Whether the plan proposes nothing.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

/// Infers the routes for a set of unrouted request paths (`R28`).
///
/// `paths` may hold duplicates and may be in any order; the result is
/// deterministic. Names already used by `manifest` are suffixed `-2`, `-3`, …
/// and never reused.
pub fn infer<'a>(paths: impl IntoIterator<Item = &'a str>, manifest: &Manifest) -> RoutePlan {
    // Group by the name candidate first, so the two paths of one route are not
    // told apart by the disambiguation that only exists for *different* routes.
    let mut candidates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut originals: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in paths {
        let (candidate, pattern) = describe(path);
        candidates
            .entry(candidate.clone())
            .or_default()
            .insert(pattern);
        originals
            .entry(candidate)
            .or_default()
            .insert(path.to_string());
    }

    let mut used: BTreeSet<String> = manifest
        .routes()
        .iter()
        .map(|route| route.name().to_string())
        .collect();
    let mut plan = RoutePlan {
        insert_at: manifest
            .routes()
            .iter()
            .position(Route::is_unrestricted_catch_all)
            .unwrap_or_else(|| manifest.route_count()),
        ..RoutePlan::default()
    };
    for (candidate, patterns) in candidates {
        let name = disambiguate(&candidate, &mut used);
        if let Some(originals) = originals.get(&candidate) {
            for original in originals {
                plan.by_path.insert(original.clone(), name.clone());
            }
        }
        plan.routes.push(InferredRoute {
            name,
            paths: patterns.into_iter().collect(),
        });
    }
    plan
}

/// Whether a request path may be turned into a view by `--learn` at all.
///
/// `--learn` exists to name the views a developer forgot to declare. A scanner
/// probe is not one of them: a `/../etc/passwd` traversal, `/wp-login.php`, a
/// `/.env` fetch or a `/.git/config` request is a bot trying its luck, and a
/// `Blastfile` view for it would grant an attacker's path a home in production.
/// So a path is learnable only when it is absolute, holds no control character,
/// no backslash and no segment that starts with `.` — which covers `.`, `..`,
/// `.env`, `.git` and any other hidden file — and no `..` inside a segment.
///
/// It is also not learnable when a `static` mount already answers it: the
/// responder serves those files before any view sees them, so a view for the
/// path would be dead weight (`R14`).
pub fn is_learnable_path(path: &str) -> bool {
    if !path.starts_with('/') {
        return false;
    }
    if path
        .chars()
        .any(|character| character.is_control() || character == '\\')
    {
        return false;
    }
    path.split('/')
        .all(|segment| !segment.starts_with('.') && !segment.contains(".."))
}

/// The name and path regex one request path infers, before grouping.
fn describe(path: &str) -> (String, String) {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return ("root".to_string(), "/".to_string());
    }
    let mut pattern = String::from("/");
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            pattern.push('/');
        }
        pattern.push_str(&segment_pattern(segment));
    }
    if path.ends_with('/') {
        pattern.push('/');
    }
    // A numeric or UUID segment is a placeholder, not the route's name: the
    // name comes from the first *static* segment, so `/123/456` is `route`.
    let name = segments
        .iter()
        .find(|segment| !is_dynamic(segment))
        .map_or_else(|| "route".to_string(), |segment| sanitize(segment));
    (name, pattern)
}

/// Whether a segment is a value rather than a name: all digits, or a UUID.
fn is_dynamic(segment: &str) -> bool {
    segment.chars().all(|character| character.is_ascii_digit()) || is_uuid(segment)
}

/// The route name a path segment suggests.
fn sanitize(segment: &str) -> String {
    let mut name = String::with_capacity(segment.len());
    for character in segment.chars() {
        let lower = character.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() || lower == '_' || lower == '-' {
            name.push(lower);
        } else if !name.ends_with('-') {
            name.push('-');
        }
    }
    let trimmed = name.trim_matches(|character: char| !character.is_ascii_alphanumeric());
    if trimmed.is_empty() {
        "route".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Returns `base`, or the first free `base-N` for `N >= 2`.
fn disambiguate(base: &str, used: &mut BTreeSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

/// One segment as a regex.
fn segment_pattern(segment: &str) -> String {
    if !segment.is_empty() && segment.chars().all(|character| character.is_ascii_digit()) {
        return "[0-9]+".to_string();
    }
    if is_uuid(segment) {
        return "[^/]+".to_string();
    }
    let mut out = String::with_capacity(segment.len());
    for character in segment.chars() {
        if "\\^$.|?*+()[]{}".contains(character) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// Whether `segment` is shaped like a UUID (`8-4-4-4-12` hex digits).
fn is_uuid(segment: &str) -> bool {
    segment.len() == 36
        && segment.char_indices().all(|(at, character)| match at {
            8 | 13 | 18 | 23 => character == '-',
            _ => character.is_ascii_hexdigit(),
        })
}
