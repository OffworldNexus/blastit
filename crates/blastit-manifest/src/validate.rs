//! Validation of a parsed manifest: everything that needs the whole document
//! (§6).

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use crate::error::{
    Location, ManifestError, ManifestErrors, ManifestWarning, quoted, regex_message,
};
use crate::model::{Manifest, PgGrant, PhpPolicy, ProxyName, Route, StaticPolicy};
use crate::routing::compile_pattern;
use crate::uid::UidLayout;

use blastit_ingress::BodyCodec;
use blastit_sqlpolicy::{KNOWN_STATIC_SETUP_SQL, has_quoted_literal, is_system_table, normalize};

/// The fixed rules a `[route.static]` value can break (`R2`, `R3`, `R5`).
///
/// The design pins these phrases so a boot message reads the same everywhere
/// and no test depends on the wording of an operating-system error.
const RELATIVE_PATH: &str = "must be a relative path";
const NO_PARENT_SEGMENT: &str = "must not contain ..";
const NO_BACKSLASH: &str = "must not contain \\";
const NO_CONTROL_BYTE: &str = "must not contain a control byte";
const ONE_OR_MORE_SEGMENTS: &str = "must be one or more segments";
const NO_DOTFILE_SEGMENT: &str = "must not contain a dotfile segment";
const ANCHORED_PATTERN: &str = "must start with /";
const AT_MOST_PATTERNS: &str = "at most 32 patterns";
const AT_MOST_PATTERN_BYTES: &str = "at most 256 bytes";

/// The fixed rules a `static "<prefix>"` mount's prefix can break (§9.1).
///
/// The prefix is a URL prefix, not a filesystem path, so it carries the one
/// rule a path does not: it is `/`-anchored and has no trailing `/` (unless it
/// is `"/"`), and a query, a fragment or an escape is refused because a request
/// path never carries one.
const MOUNT_ANCHORED: &str = "must start with /";
const MOUNT_NO_TRAILING_SLASH: &str = "must not end with /";
const MOUNT_NO_EMPTY_SEGMENT: &str = "must not contain an empty segment";
const MOUNT_NO_QUERY: &str = "must not contain ?";
const MOUNT_NO_FRAGMENT: &str = "must not contain #";
const MOUNT_NO_ESCAPE: &str = "must not contain %";

/// The fixed rules a `[route.php]` value can break (`ADR-0010` §2; the shape
/// rules of §4.1 transferred verbatim from the retired key).
const MUST_END_IN_PHP: &str = "must end in .php";
const AT_LEAST_ONE: &str = "must be at least 1";
const AT_MOST_WORKERS: &str = "at most 64";

/// Why a `[route.php]` declaration can never run (`ADR-0010` §2).
///
/// `spa = true` answers every miss itself, and `CONNECT`/`TRACE` are the two
/// methods a front controller cannot serve.
const SPA_NEVER_REACHES_FRONT_CONTROLLER: &str = "spa = true never reaches the front controller";
const CONNECT_CANNOT_REACH_WORKER: &str = "CONNECT cannot reach a PHP worker";
const TRACE_CANNOT_REACH_WORKER: &str = "TRACE cannot reach a PHP worker";

/// The largest `[route.php]` `workers` cap (`ADR-0010` §2).
///
/// A grammar bound, not the deployment's: a value above the route's per-view
/// budget is a separate boot check that has a `Config` (and so is not
/// `validate`'s, which must keep parsing on a machine with no deployment).
const MAX_WORKERS: u32 = 64;

/// How many `immutable` patterns one route may declare (`R5`).
const MAX_PATTERNS: usize = 32;

/// How long one `immutable` pattern may be, in bytes (`R5`).
const MAX_PATTERN_BYTES: usize = 256;

/// Validates a manifest and collects its warnings.
pub(crate) fn validate(
    manifest: &Manifest,
    base: u32,
) -> Result<Vec<ManifestWarning>, ManifestErrors> {
    let mut errors = Vec::new();
    check_app(manifest, &mut errors);
    check_groups(manifest, &mut errors);
    check_grants(manifest, &mut errors);
    check_unrestricted(manifest, &mut errors);
    check_table_grants(manifest, &mut errors);
    check_queries(manifest, &mut errors);
    check_routes(manifest, &mut errors);
    check_static(manifest, &mut errors);
    check_mounts(manifest, &mut errors);
    check_php(manifest, &mut errors);
    check_uids(manifest, base, &mut errors);

    if errors.is_empty() {
        Ok(warnings(manifest))
    } else {
        Err(ManifestErrors::from(errors))
    }
}

/// The top-level `app` has to be a `module:attribute` callable (`R7`).
///
/// A colon is the one rule every caller agrees on — `BLASTIT_APP` is checked
/// the same way — and it is the mistake worth naming: Django's dotted
/// `myshop.wsgi.application` looks right and imports as a module path, which
/// fails much later and much less clearly.
fn check_app(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    let Some(app) = manifest.app() else {
        return;
    };
    if !app.contains(':') {
        errors.push(ManifestError::InvalidApp {
            app: app.to_string(),
        });
    }
}

/// Every referenced group must exist.
fn check_groups(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    for group in manifest.global_groups() {
        if !manifest.groups().contains_key(group) {
            errors.push(ManifestError::UnknownGroup {
                location: Location::Global,
                group: group.clone(),
            });
        }
    }
    for (index, route) in manifest.routes().iter().enumerate() {
        for group in route.groups() {
            if !manifest.groups().contains_key(group) {
                errors.push(ManifestError::UnknownGroup {
                    location: Location::Route {
                        index,
                        name: route.name().to_string(),
                    },
                    group: group.clone(),
                });
            }
        }
    }
}

/// Every grant must name a declared proxy.
///
/// A grant on a proxy whose own declaration failed is not reported again: the
/// declaration error is already in the list.
fn check_grants(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    for unresolved in manifest.unresolved_grants() {
        errors.push(ManifestError::GrantOnUnknownProxy {
            location: unresolved.location().clone(),
            proxy: unresolved.proxy().clone(),
        });
    }
}

/// The two rules an `unrestricted = true` grant has to obey (§6): it is only
/// allowed inline on a route, and it takes no queries.
///
/// A group can be attached to every route at once, so an unrestricted grant
/// there could turn the whole manifest into one sandbox — the opposite of what
/// the file is for. Inline, it reaches exactly one route and one proxy, and a
/// query list next to it could only ever suggest a restriction that is not
/// there.
fn check_unrestricted(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    for (name, group) in manifest.groups() {
        for (proxy, grant) in group.grants() {
            let Some(grant) = grant.as_postgresql() else {
                continue;
            };
            if !grant.is_unrestricted() {
                continue;
            }
            let location = Location::Group(name.clone());
            errors.push(ManifestError::UnrestrictedGroup {
                location: location.clone(),
                proxy: proxy.clone(),
            });
            // A grant that does both is wrong twice; both are reported.
            if !grant.queries().is_empty() {
                errors.push(ManifestError::UnrestrictedWithQueries {
                    location,
                    proxy: proxy.clone(),
                });
            }
        }
    }
    for (index, route) in manifest.routes().iter().enumerate() {
        for (proxy, grant) in route.grants() {
            let Some(grant_pg) = grant.as_postgresql() else {
                continue;
            };
            let location = Location::Route {
                index,
                name: route.name().to_string(),
            };
            if !grant_pg.is_unrestricted() {
                continue;
            }
            if !grant_pg.queries().is_empty() {
                errors.push(ManifestError::UnrestrictedWithQueries {
                    location: location.clone(),
                    proxy: proxy.clone(),
                });
            }
            if !grant_pg.tables().tables().is_empty() {
                errors.push(ManifestError::UnrestrictedWithContent {
                    location: location.clone(),
                    proxy: proxy.clone(),
                    what: "tables",
                });
            }
            if grant_pg.tables().functions().next().is_some() {
                errors.push(ManifestError::UnrestrictedWithContent {
                    location,
                    proxy: proxy.clone(),
                    what: "functions",
                });
            }
        }
    }
}

/// A table grant may name any table except a system catalogue (§6, D7).
///
/// The analysis refuses a qualified `pg_catalog`/`information_schema` name and
/// any **unqualified** name that resolves into one — `pg_class` (because
/// `pg_catalog` is first on the search path) and the `information_schema` views
/// — so an entry naming one could only read as a capability it is not.
fn check_table_grants(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    let mut check = |location: &Location, proxy: &ProxyName, grant: &PgGrant| {
        for table in grant.tables().tables().keys() {
            let qualified = table.contains('.');
            let system = table.starts_with("pg_catalog.")
                || table.starts_with("information_schema.")
                || table == "pg_catalog"
                || table == "information_schema"
                // An unqualified key is the one the analysis would refuse: the
                // name a statement names cannot keep a schema.
                || (!qualified && is_system_table(table));
            if system {
                errors.push(ManifestError::UnusableTable {
                    location: location.clone(),
                    proxy: proxy.clone(),
                    table: table.clone(),
                });
            }
        }
    };
    for (name, group) in manifest.groups() {
        for (proxy, grant) in group.grants() {
            if let Some(grant) = grant.as_postgresql() {
                check(&Location::Group(name.clone()), proxy, grant);
            }
        }
    }
    for (index, route) in manifest.routes().iter().enumerate() {
        let location = Location::Route {
            index,
            name: route.name().to_string(),
        };
        for (proxy, grant) in route.grants() {
            if let Some(grant) = grant.as_postgresql() {
                check(&location, proxy, grant);
            }
        }
    }
}

/// A `queries` entry may never hold a redacted `…` placeholder or an inline
/// quoted literal (`R24`, `ADR-0003`).
///
/// An exact grant is compared and run as written, so a statement carrying a
/// value would both commit that value to the repository and match only that one
/// value. `--learn` refuses such statements instead of writing them, and
/// `KNOWN_STATIC_SETUP_SQL` is the one fixed pair of protocol statements that
/// may keep its literal. Neither message prints the entry: printing it is what
/// the rule forbids.
fn check_queries(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    let mut check = |location: &Location, proxy: &ProxyName, grant: &PgGrant| {
        for (rule, query) in grant.queries().iter().enumerate() {
            // The two fixed setup statements are exempt in their normalised
            // form: `SET TIME ZONE 'UTC'` and its `$1` spelling are the same
            // statement on the wire and in the manifest.
            let query = normalize(query);
            if query.contains('…') {
                errors.push(ManifestError::RedactedQueryPlaceholder {
                    location: location.clone(),
                    proxy: proxy.clone(),
                    rule,
                });
            } else if has_quoted_literal(&query)
                && !KNOWN_STATIC_SETUP_SQL.contains(&query.as_str())
            {
                errors.push(ManifestError::LiteralQuery {
                    location: location.clone(),
                    proxy: proxy.clone(),
                    rule,
                });
            }
        }
    };
    for (name, group) in manifest.groups() {
        for (proxy, grant) in group.grants() {
            if let Some(grant) = grant.as_postgresql() {
                check(&Location::Group(name.clone()), proxy, grant);
            }
        }
    }
    for (index, route) in manifest.routes().iter().enumerate() {
        let location = Location::Route {
            index,
            name: route.name().to_string(),
        };
        for (proxy, grant) in route.grants() {
            if let Some(grant) = grant.as_postgresql() {
                check(&location, proxy, grant);
            }
        }
    }
}

/// Route names, path regexes and method tokens.
///
/// Names are compared **by sandbox**, not by position (§10.3): one `view` with
/// several endpoints lowers to several routes that share one name and one
/// sandbox, and those are not duplicates. A name that reappears in a different
/// sandbox is, and the message names the two routes so an operator can find
/// them. A `blastit.toml` route is always its own sandbox, so the retired
/// format keeps today's answer for every document it can express.
fn check_routes(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    let mut seen: BTreeMap<&str, (u32, usize)> = BTreeMap::new();
    for (index, route) in manifest.routes().iter().enumerate() {
        let name = route.name();
        if !valid_route_name(name) {
            errors.push(ManifestError::InvalidRouteName {
                index,
                name: name.to_string(),
            });
        }
        match seen.entry(name) {
            Entry::Vacant(slot) => {
                slot.insert((route.sandbox(), index));
            }
            Entry::Occupied(first) => {
                let (sandbox, first_index) = *first.get();
                if sandbox != route.sandbox() {
                    errors.push(ManifestError::DuplicateRouteName {
                        name: name.to_string(),
                        first: first_index,
                        second: index,
                    });
                }
            }
        }

        if route.path().is_empty() {
            errors.push(ManifestError::EmptyPath {
                route: name.to_string(),
            });
        }
        for (rule, pattern) in route.path().iter().enumerate() {
            if pattern.is_empty() {
                errors.push(ManifestError::EmptyPattern {
                    route: name.to_string(),
                    rule,
                });
                continue;
            }
            if let Err(error) = compile_pattern(pattern) {
                errors.push(ManifestError::InvalidRegex {
                    route: name.to_string(),
                    rule,
                    pattern: pattern.clone(),
                    message: regex_message(&error),
                });
            }
        }

        match route.methods() {
            Some([]) => errors.push(ManifestError::EmptyMethods {
                route: name.to_string(),
            }),
            Some(methods) => {
                for method in methods {
                    if !valid_method(method) {
                        errors.push(ManifestError::InvalidMethod {
                            route: name.to_string(),
                            method: method.clone(),
                        });
                    }
                }
            }
            None => {}
        }
    }
}

/// The `[route.static]` blocks: the four keys' rules and the declarations a
/// route that can never reach the app may not make (`R1`–`R5`).
///
/// Existence is not checked here: a documented manifest has to validate on a
/// machine that has not produced `public/` yet. [`Manifest::check_static_roots`]
/// is the explicit filesystem probe.
fn check_static(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    for (index, route) in manifest.routes().iter().enumerate() {
        let Some(policy) = route.static_policy() else {
            continue;
        };
        let location = Location::Route {
            index,
            name: route.name().to_string(),
        };
        check_static_root(&location, policy, errors);
        check_static_index(&location, policy, errors);
        check_static_immutable(&location, policy, errors);
        check_static_declarations(manifest, route, policy, location, errors);
    }
}

/// `root` is a relative path that stays in the manifest's directory (`R2`).
///
/// An empty string is not a relative path either, so it shares the rule: the
/// two are the same mistake — a root that names no directory beneath
/// `blastit.toml`.
fn check_static_root(location: &Location, policy: &StaticPolicy, errors: &mut Vec<ManifestError>) {
    let root = policy.root.as_str();
    let rule = if root.is_empty() || root.starts_with('/') {
        Some(RELATIVE_PATH)
    } else if root.contains('\\') {
        Some(NO_BACKSLASH)
    } else if has_parent_segment(root) {
        Some(NO_PARENT_SEGMENT)
    } else if has_control_byte(root) {
        Some(NO_CONTROL_BYTE)
    } else {
        None
    };
    if let Some(rule) = rule {
        errors.push(ManifestError::StaticRootInvalid {
            location: location.clone(),
            root: policy.root.clone(),
            rule,
        });
    }
}

/// `index` is a non-empty relative path beneath `root`, and no part of it is a
/// dotfile (`R3`).
///
/// A dotfile segment is refused because the index answers every miss: an
/// `index = ".env"` would publish the manifest directory's dotfiles to the
/// world.
fn check_static_index(location: &Location, policy: &StaticPolicy, errors: &mut Vec<ManifestError>) {
    let index = policy.index.as_str();
    let rule = if index.starts_with('/') {
        Some(RELATIVE_PATH)
    } else if index.is_empty() || index.split('/').any(|segment| segment.is_empty()) {
        Some(ONE_OR_MORE_SEGMENTS)
    } else if has_parent_segment(index) {
        Some(NO_PARENT_SEGMENT)
    } else if index.split('/').any(is_dotfile_segment) {
        Some(NO_DOTFILE_SEGMENT)
    } else if index.contains('\\') {
        Some(NO_BACKSLASH)
    } else if has_control_byte(index) {
        Some(NO_CONTROL_BYTE)
    } else {
        None
    };
    if let Some(rule) = rule {
        errors.push(ManifestError::StaticIndexInvalid {
            location: location.clone(),
            index: policy.index.clone(),
            rule,
        });
    }
}

/// Every `immutable` pattern is a `/`-anchored glob matched whole against the
/// answering file's path (`R5`).
///
/// The limit is on the list as well as on each pattern: an unbounded list is an
/// unbounded amount of work per answer, and a pattern longer than a path
/// component cannot name a file's path on any filesystem blastit serves.
fn check_static_immutable(
    location: &Location,
    policy: &StaticPolicy,
    errors: &mut Vec<ManifestError>,
) {
    if policy.immutable.len() > MAX_PATTERNS {
        // The first pattern past the limit is the one to delete.
        let pattern = policy
            .immutable
            .get(MAX_PATTERNS)
            .cloned()
            .unwrap_or_default();
        errors.push(ManifestError::StaticImmutablePattern {
            location: location.clone(),
            pattern,
            rule: AT_MOST_PATTERNS,
        });
    }
    for pattern in &policy.immutable {
        let rule = if pattern.len() > MAX_PATTERN_BYTES {
            Some(AT_MOST_PATTERN_BYTES)
        } else if !pattern.starts_with('/') {
            Some(ANCHORED_PATTERN)
        } else if has_parent_segment(pattern) {
            Some(NO_PARENT_SEGMENT)
        } else if has_control_byte(pattern) {
            Some(NO_CONTROL_BYTE)
        } else {
            None
        };
        if let Some(rule) = rule {
            errors.push(ManifestError::StaticImmutablePattern {
                location: location.clone(),
                pattern: pattern.clone(),
                rule,
            });
        }
    }
}

/// A static route that can never reach the app may not declare anything only
/// the app could run (`R4`).
///
/// `spa = true` answers every miss itself, and a manifest without an `app` has
/// nothing to forward to. An ingress codec, a prefork, an inline grant or an
/// inline `groups` list on such a route is dead weight that reads like a
/// capability, so it is refused with the reason and the fix. Groups inherited
/// from the manifest-wide `groups` list are not re-checked: they are attached
/// to the route, not declared by it.
fn check_static_declarations(
    manifest: &Manifest,
    route: &Route,
    policy: &StaticPolicy,
    location: Location,
    errors: &mut Vec<ManifestError>,
) {
    // A route with a front controller does reach an executor, so nothing on it
    // is dead; `spa = true` beside `[route.php]` is refused by `check_php`
    // instead (the retired key kept both arms the same way).
    if route.php().is_some() {
        return;
    }
    let why = if policy.fallback.is_some() {
        "spa = true never reaches the app"
    } else if manifest.app().is_none() {
        "the manifest declares no app"
    } else {
        return;
    };

    let mut dead = |key: String| {
        errors.push(ManifestError::StaticDeadDeclaration {
            location: location.clone(),
            key,
            why,
        });
    };
    if route.ingress().is_some() {
        dead("[route.ingress]".to_string());
    }
    if route.prefork() {
        dead("prefork = true".to_string());
    }
    for proxy in route.grants().keys() {
        dead(format!("[route.{proxy}]"));
    }
    if !route.groups().is_empty() {
        dead("groups".to_string());
    }
}

/// The top-level `static "<prefix>"` mounts (§9.1): the four keys' rules, the
/// prefix's shape and its uniqueness (§9.1, §9.4).
///
/// `root`, `index` and `immutable` are checked **per mount**, because that is
/// the declaration they belong to; the sentence names the mount. A mount the
/// retired reader lowered from a `[route.static]` block carries the identical
/// policy that block's route already had checked, so it is skipped here and one
/// mistake is still reported once. Nothing touches the filesystem: a mount has
/// to validate on a machine that has not run the build yet.
fn check_mounts(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    // The retired reader lowers each `[route.static]` block to a route *and* a
    // mount at the URL root, so the two carry one identical policy. The route
    // walk above already checked that one; checking it again here would report
    // one mistake twice under two subjects. A `Blastfile` has no route-level
    // policy at all, so every mount it declares is this function's (§9.1,
    // §9.4).
    let bridged = |policy: &StaticPolicy| {
        manifest
            .routes()
            .iter()
            .any(|route| route.static_policy() == Some(policy))
    };
    for (index, mount) in manifest.mounts().iter().enumerate() {
        let location = Location::Mount {
            index,
            prefix: mount.prefix.clone(),
        };
        if !bridged(&mount.policy) {
            check_static_root(&location, &mount.policy, errors);
            check_static_index(&location, &mount.policy, errors);
            check_static_immutable(&location, &mount.policy, errors);
        }
        if let Some(rule) = mount_prefix_rule(&mount.prefix) {
            errors.push(ManifestError::StaticMountPrefixInvalid {
                location: location.clone(),
                rule,
            });
        }
        // A `fallback` that is the `index` is the retired reader's lowering of
        // its flag, and that name is already checked on the route's own block:
        // one mistake is reported once. Every other name is this check's.
        if let Some(fallback) = mount.policy.fallback.as_deref()
            && fallback != mount.policy.index
            && let Some(rule) = fallback_rule(fallback)
        {
            errors.push(ManifestError::StaticFallbackInvalid {
                location: location.clone(),
                fallback: fallback.to_string(),
                rule,
            });
        }
        match seen.entry(mount.prefix.as_str()) {
            Entry::Vacant(slot) => {
                slot.insert(index);
            }
            Entry::Occupied(first) => errors.push(ManifestError::DuplicateMount {
                prefix: mount.prefix.clone(),
                first: *first.get(),
                second: index,
            }),
        }
    }
}

/// The first rule a mount prefix breaks, or `None` (§9.1).
///
/// A prefix is a URL prefix, not a filesystem path: `/`-anchored, no trailing
/// `/` unless it is `"/"`, no `..` and no empty segment, and no byte a request
/// path cannot carry. A dotfile segment is **allowed** — `/.well-known` is a
/// real prefix — because dotfiles stay refused by the resolver, on the request
/// path (§9.2).
fn mount_prefix_rule(prefix: &str) -> Option<&'static str> {
    if !prefix.starts_with('/') {
        return Some(MOUNT_ANCHORED);
    }
    if prefix != "/" && prefix.ends_with('/') {
        return Some(MOUNT_NO_TRAILING_SLASH);
    }
    let rest = prefix.strip_prefix('/').unwrap_or(prefix);
    if rest.is_empty() {
        return None;
    }
    if rest.split('/').any(|segment| segment.is_empty()) {
        Some(MOUNT_NO_EMPTY_SEGMENT)
    } else if has_parent_segment(prefix) {
        Some(NO_PARENT_SEGMENT)
    } else if prefix.contains('\\') {
        Some(NO_BACKSLASH)
    } else if has_control_byte(prefix) {
        Some(NO_CONTROL_BYTE)
    } else if prefix.contains('?') {
        Some(MOUNT_NO_QUERY)
    } else if prefix.contains('#') {
        Some(MOUNT_NO_FRAGMENT)
    } else if prefix.contains('%') {
        Some(MOUNT_NO_ESCAPE)
    } else {
        None
    }
}

/// The first rule a mount's `fallback` breaks, or `None` (§9.1, §9.3).
///
/// The fallback is a file name below `root` that answers a request the
/// request-path check never sees, so a dotfile segment is refused here: with
/// `root "."`, `fallback ".env"` would answer every `GET` miss with the
/// environment file. `root` itself may be `.` or a dot-directory — the author
/// chose that directory, and the resolver still checks the request path.
fn fallback_rule(fallback: &str) -> Option<&'static str> {
    if fallback.starts_with('/') {
        Some(RELATIVE_PATH)
    } else if fallback.is_empty() || fallback.split('/').any(|segment| segment.is_empty()) {
        Some(ONE_OR_MORE_SEGMENTS)
    } else if has_parent_segment(fallback) {
        Some(NO_PARENT_SEGMENT)
    } else if fallback.split('/').any(is_dotfile_segment) {
        Some(NO_DOTFILE_SEGMENT)
    } else if fallback.contains('\\') {
        Some(NO_BACKSLASH)
    } else if has_control_byte(fallback) {
        Some(NO_CONTROL_BYTE)
    } else {
        None
    }
}

/// A `/`-separated segment that starts with `.` and is not exactly `.` (§9.1).
fn is_dotfile_segment(segment: &str) -> bool {
    segment.starts_with('.') && segment != "."
}

/// The `[route.php]` blocks: the block's rules and the declarations that can
/// never reach the front controller (`ADR-0010` §2; the shape rules of the
/// retired key's §4.1, transferred verbatim).
///
/// Existence, symlinks and readability are not checked here: an explicit probe
/// owns those (`MB.3.5.2`), so a manifest still validates on a machine that has
/// not produced `public/` yet. There is no `listen` key to check — the
/// transport is a `socketpair` blastit owns.
fn check_php(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    for (index, route) in manifest.routes().iter().enumerate() {
        let Some(policy) = route.php() else {
            continue;
        };
        let location = Location::Route {
            index,
            name: route.name().to_string(),
        };
        let root = route
            .static_policy()
            .map(|static_policy| static_policy.root.as_str());
        check_php_entry(&location, policy, root, errors);
        check_php_workers(&location, policy, errors);
        check_php_declarations(route, &location, errors);
    }
}

/// `entry` is a relative `.php` path that stays in the manifest's directory, and
/// beneath `[route.static] root` when the route declares both (`ADR-0010` §2).
///
/// Whether it exists, is a regular file and is readable by the route uid is the
/// probe's: its `OutsideRoot`/`Missing`/… map onto this variant with the same
/// rule phrases, so one message reads the same at parse time and at boot. The
/// root comparison is by whole path segment, not by string prefix: `publicity/`
/// is not beneath `public/`.
fn check_php_entry(
    location: &Location,
    policy: &PhpPolicy,
    root: Option<&str>,
    errors: &mut Vec<ManifestError>,
) {
    let entry = policy.entry.as_str();
    let rule = if entry.is_empty() || entry.starts_with('/') {
        Some(RELATIVE_PATH.to_string())
    } else if entry.contains('\\') {
        Some(NO_BACKSLASH.to_string())
    } else if has_parent_segment(entry) {
        Some(NO_PARENT_SEGMENT.to_string())
    } else if has_control_byte(entry) {
        Some(NO_CONTROL_BYTE.to_string())
    } else if !entry.to_ascii_lowercase().ends_with(".php") {
        Some(MUST_END_IN_PHP.to_string())
    } else if let Some(root) = root
        && !entry_is_under_root(entry, root)
    {
        Some(format!("must be under static root {}", quoted(root)))
    } else {
        None
    };
    if let Some(rule) = rule {
        errors.push(ManifestError::PhpEntryInvalid {
            location: location.clone(),
            entry: policy.entry.clone(),
            rule,
        });
    }
}

/// True when `entry` names a file beneath `root`, compared by whole segments.
///
/// Both are already known to be relative paths without `..`; `root` may be `.`
/// (the manifest's own directory), and empty or `.` segments add nothing.
fn entry_is_under_root(entry: &str, root: &str) -> bool {
    let root = path_segments(root);
    let entry = path_segments(entry);
    root.len() < entry.len() && entry[..root.len()] == root[..]
}

/// The segments of a `/`-separated relative path, without empty or `.` ones.
fn path_segments(value: &str) -> Vec<&str> {
    value
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect()
}

/// An explicit `workers` cap is the route's ceiling, from `1` to `64`
/// (`ADR-0010` §2).
///
/// Absent is not a rule violation: blastit derives the value from its
/// concurrency budget, and `workers` may only lower it.
fn check_php_workers(location: &Location, policy: &PhpPolicy, errors: &mut Vec<ManifestError>) {
    let Some(workers) = policy.workers else {
        return;
    };
    let why = if workers == 0 {
        Some(AT_LEAST_ONE)
    } else if workers > MAX_WORKERS {
        Some(AT_MOST_WORKERS)
    } else {
        None
    };
    if let Some(why) = why {
        errors.push(ManifestError::PhpWorkersInvalid {
            location: location.clone(),
            workers,
            why,
        });
    }
}

/// A PHP route may not declare a form that never reaches a worker
/// (`ADR-0010` §2): `spa = true` answers every miss itself, and
/// `CONNECT`/`TRACE` are the two methods a front controller cannot serve.
fn check_php_declarations(route: &Route, location: &Location, errors: &mut Vec<ManifestError>) {
    let mut dead = |key: &str, why: &'static str| {
        errors.push(ManifestError::PhpDeadDeclaration {
            location: location.clone(),
            key: key.to_string(),
            why,
        });
    };
    if route
        .static_policy()
        .is_some_and(|policy| policy.fallback.is_some())
    {
        dead("spa = true", SPA_NEVER_REACHES_FRONT_CONTROLLER);
    }
    if let Some(methods) = route.methods() {
        if methods.iter().any(|method| method == "CONNECT") {
            dead("CONNECT", CONNECT_CANNOT_REACH_WORKER);
        }
        if methods.iter().any(|method| method == "TRACE") {
            dead("TRACE", TRACE_CANNOT_REACH_WORKER);
        }
    }
}

/// A value holding a C0 control byte, NUL included (`R2`, `R3`, `R5`).
fn has_control_byte(value: &str) -> bool {
    value.bytes().any(|byte| byte < 0x20)
}
/// A `/`-separated value with a `..` segment of its own.
fn has_parent_segment(value: &str) -> bool {
    value.split('/').any(|segment| segment == "..")
}

/// The uid table must stay below [`UidLayout::MAX_UID_EXCLUSIVE`] (§2).
///
/// Routes are numbered by **sandbox**, not by position: several routes of one
/// `view` share one uid, so the table's reach is the highest sandbox id and the
/// error names the route that carries it (§7.3).
fn check_uids(manifest: &Manifest, base: u32, errors: &mut Vec<ManifestError>) {
    let layout = UidLayout::new(base);
    if layout.dbproxy() >= UidLayout::MAX_UID_EXCLUSIVE {
        errors.push(ManifestError::UidBaseTooHigh {
            base,
            uid: layout.dbproxy(),
            max: UidLayout::MAX_UID_EXCLUSIVE,
        });
    }
    let highest = manifest
        .routes()
        .iter()
        .enumerate()
        .max_by_key(|(_, route)| route.sandbox());
    if let Some((index, route)) = highest {
        let uid = layout.sandbox(route.sandbox());
        if uid >= UidLayout::MAX_UID_EXCLUSIVE {
            errors.push(ManifestError::TooManyRoutes {
                index,
                uid,
                base,
                max: UidLayout::MAX_UID_EXCLUSIVE,
            });
        }
    }
}

/// A route name is `[a-z0-9][a-z0-9_-]*`.
fn valid_route_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        None => false,
        Some(first) => {
            (first.is_ascii_lowercase() || first.is_ascii_digit())
                && bytes.all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'
                        || byte == b'-'
                })
        }
    }
}

/// A method is an uppercase HTTP token. `HEAD` is never implied by `GET`, so a
/// lowercase or malformed token is a mistake rather than a shorthand (§6).
fn valid_method(method: &str) -> bool {
    !method.is_empty()
        && method
            .bytes()
            .all(|byte| token_char(byte) && !byte.is_ascii_lowercase())
}

/// RFC 9110 `tchar`.
fn token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// The non-fatal findings: a trait attached nowhere, a store nobody grants, an
/// unrestricted view, a `raw` body, `egress any`, and a front controller no view
/// forwards to.
fn warnings(manifest: &Manifest) -> Vec<ManifestWarning> {
    let mut warnings = Vec::new();

    let mut attached: BTreeSet<&str> = manifest
        .global_groups()
        .iter()
        .map(String::as_str)
        .collect();
    for route in manifest.routes() {
        attached.extend(route.groups().iter().map(String::as_str));
    }
    for group in manifest.groups().keys() {
        if !attached.contains(group.as_str()) {
            warnings.push(ManifestWarning::UnattachedGroup {
                group: group.clone(),
            });
        }
    }

    let mut granted: BTreeSet<&ProxyName> = BTreeSet::new();
    for route in manifest.routes() {
        granted.extend(route.grants().keys());
    }
    for group in manifest.groups().values() {
        granted.extend(group.grants().keys());
    }
    for proxy in manifest.proxies().keys() {
        if !granted.contains(proxy) {
            warnings.push(ManifestWarning::UngrantedProxy {
                proxy: proxy.clone(),
            });
        }
    }

    // One warning per (route, proxy) that may run anything at all, in route
    // order and then proxy order, so a boot reports the same list every time.
    for route in manifest.routes() {
        for (proxy, grant) in route.grants() {
            if grant
                .as_postgresql()
                .is_some_and(|grant| grant.is_unrestricted())
            {
                warnings.push(ManifestWarning::UnrestrictedRoute {
                    route: route.name().to_string(),
                    proxy: proxy.clone(),
                });
            }
        }
    }

    // The two route-level opt-outs, each in route order: a `raw` body reaches
    // the app unvalidated, and `egress = true` opens any address and port.
    for route in manifest.routes() {
        if route
            .ingress()
            .is_some_and(|ingress| ingress.body == BodyCodec::Raw)
        {
            warnings.push(ManifestWarning::RawIngress {
                route: route.name().to_string(),
            });
        }
    }
    for route in manifest.routes() {
        if route.egress() {
            warnings.push(ManifestWarning::EgressRoute {
                route: route.name().to_string(),
            });
        }
    }

    // §12.2/§9.5: a front controller no view forwards to serves nothing. A
    // warning, not an error, so a static mount and a `libphp` entry can ship
    // together before the catch-all view is written; the reminder is the point.
    if let Some(php) = manifest.php()
        && manifest.routes().is_empty()
    {
        warnings.push(ManifestWarning::EntrypointNotReached {
            entry: php.entry.clone(),
        });
    }

    warnings
}

impl Manifest {
    /// The production path's refusal sentences (`ADR-0011`), one per view that
    /// declares `body raw` without `unvalidated`, in route order.
    ///
    /// `raw` is the one ingress declaration the runtime does not gate: the bytes
    /// reach the application's own parser unchanged (`blastit_ingress::
    /// plan_body`'s `StreamPassthrough`). The acknowledgement in the view is
    /// therefore the whole of the decision, and on the production path an
    /// unacknowledged one is a **boot refusal** rather than the warning it used
    /// to be — the boot names the view and the node before a listener exists
    /// (§10.2). `blastit dev` keeps the warning
    /// (`ManifestWarning::RawIngress`, emitted from `warnings`) because it is
    /// the local simulation, and `blastit check` keeps the `raw_ingress` finding
    /// so CI reports the same rule before anyone deploys.
    ///
    /// Acknowledged views contribute nothing, so an empty result is the gate
    /// being open — the caller refuses on `!is_empty()`, never on the text.
    pub fn raw_ingress_refusals(&self) -> Vec<String> {
        self.routes()
            .iter()
            .filter_map(|route| {
                let policy = route.ingress()?;
                (policy.body == BodyCodec::Raw && !policy.allow_raw).then(|| {
                    format!(
                        "view \"{}\": body raw hands the request body to the application \
                         unvalidated, and the production path refuses it without the \
                         acknowledgement; write `body raw unvalidated` in that view, or declare \
                         the codec it takes (json, form, none)",
                        route.name()
                    )
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::collections::BTreeMap;

    use super::{
        MOUNT_ANCHORED, MOUNT_NO_EMPTY_SEGMENT, MOUNT_NO_ESCAPE, MOUNT_NO_FRAGMENT, MOUNT_NO_QUERY,
        MOUNT_NO_TRAILING_SLASH, NO_BACKSLASH, NO_CONTROL_BYTE, NO_DOTFILE_SEGMENT,
        NO_PARENT_SEGMENT, fallback_rule, mount_prefix_rule, valid_method, valid_route_name,
    };
    use crate::error::{Location, ManifestError, ManifestErrors};
    use crate::model::{Manifest, Route, StaticMount, StaticPolicy};

    /// A manifest with the given `(prefix, fallback)` mounts and no routes: the
    /// static-only shape of §9.5, which is the only one a mount can be checked
    /// on before the Blastfile lowering exists.
    fn mounted(mounts: &[(&str, Option<&str>)]) -> Manifest {
        let mounts = mounts
            .iter()
            .map(|(prefix, fallback)| StaticMount {
                prefix: (*prefix).to_string(),
                policy: StaticPolicy {
                    root: "public".to_string(),
                    index: "index.html".to_string(),
                    fallback: fallback.map(|file| file.to_string()),
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

    /// A retired-format manifest whose only route is static, with the given
    /// `[route.static]` body.
    fn retired_static(block: &str) -> Manifest {
        Manifest::from_toml_str(&format!(
            "version = 1\n[[route]]\nname = \"site\"\npath = ['.*']\n[route.static]\n{block}"
        ))
        .unwrap()
    }

    /// §9.1: a prefix is a URL prefix, not a filesystem path. Every rule has
    /// its legal twin, and the two prefixes the retired format can produce —
    /// `"/"` and nothing else — are accepted.
    #[test]
    fn a_mount_prefix_is_a_url_prefix() {
        for prefix in ["/", "/assets", "/assets/v2", "/.well-known", "/a-b_c"] {
            assert_eq!(mount_prefix_rule(prefix), None, "{prefix}");
            assert_eq!(
                mounted(&[(prefix, None)]).validate(20_000),
                Ok(Vec::new()),
                "{prefix}"
            );
        }

        let cases = [
            ("assets", MOUNT_ANCHORED),
            ("", MOUNT_ANCHORED),
            ("/assets/", MOUNT_NO_TRAILING_SLASH),
            ("//assets", MOUNT_NO_EMPTY_SEGMENT),
            ("/assets//v2", MOUNT_NO_EMPTY_SEGMENT),
            ("/assets/../etc", NO_PARENT_SEGMENT),
            ("/assets\\v2", NO_BACKSLASH),
            ("/assets\0", NO_CONTROL_BYTE),
            ("/assets?x=1", MOUNT_NO_QUERY),
            ("/assets#top", MOUNT_NO_FRAGMENT),
            ("/assets%2e", MOUNT_NO_ESCAPE),
        ];
        for (prefix, rule) in cases {
            assert_eq!(mount_prefix_rule(prefix), Some(rule), "{prefix:?}");
            assert_eq!(
                mounted(&[(prefix, None)]).validate(20_000),
                Err(ManifestErrors::from(
                    ManifestError::StaticMountPrefixInvalid {
                        location: Location::Mount {
                            index: 0,
                            prefix: prefix.to_string(),
                        },
                        rule,
                    }
                )),
                "{prefix:?}"
            );
        }

        // The line the operator reads: the mount, its prefix and the fix.
        assert_eq!(
            mounted(&[("/assets/", None)])
                .validate(20_000)
                .unwrap_err()
                .to_string(),
            "mount \"/assets/\" (index 0): must not end with /"
        );
    }

    /// §9.1: two mounts with one prefix, where the second could never answer.
    #[test]
    fn two_mounts_with_one_prefix_are_refused() {
        let manifest = mounted(&[
            ("/assets", None),
            ("/", None),
            ("/assets", Some("index.html")),
        ]);
        assert_eq!(
            manifest.validate(20_000),
            Err(ManifestErrors::from(ManifestError::DuplicateMount {
                prefix: "/assets".to_string(),
                first: 0,
                second: 2,
            }))
        );
        assert_eq!(
            manifest.validate(20_000).unwrap_err().to_string(),
            "duplicate static mount \"/assets\" (mounts 0 and 2)"
        );
    }

    /// §9.1: the two names answered *for a request* refuse a dotfile segment;
    /// `root` keeps today's rules, so a dot-directory the author chose is legal
    /// — the resolver still checks the request path.
    #[test]
    fn a_dotfile_fallback_is_refused_while_a_dot_directory_root_is_legal() {
        for fallback in [".env", "shell/.env", ".output/index.html"] {
            assert_eq!(
                fallback_rule(fallback),
                Some(NO_DOTFILE_SEGMENT),
                "{fallback}"
            );
            assert_eq!(
                mounted(&[("/", Some(fallback))]).validate(20_000),
                Err(ManifestErrors::from(ManifestError::StaticFallbackInvalid {
                    location: Location::Mount {
                        index: 0,
                        prefix: "/".to_string(),
                    },
                    fallback: fallback.to_string(),
                    rule: NO_DOTFILE_SEGMENT,
                })),
                "{fallback}"
            );
        }
        for fallback in ["index.html", "app/shell.html", "./index.html"] {
            assert_eq!(fallback_rule(fallback), None, "{fallback}");
            assert_eq!(
                mounted(&[("/", Some(fallback))]).validate(20_000),
                Ok(Vec::new()),
                "{fallback}"
            );
        }

        assert_eq!(
            retired_static("root = '.output'\n").validate(20_000),
            Ok(Vec::new()),
            "a dot-directory root is the author's own choice"
        );
        assert_eq!(
            retired_static("root = '.'\nindex = '.env'\n").validate(20_000),
            Err(ManifestErrors::from(ManifestError::StaticIndexInvalid {
                location: Location::Route {
                    index: 0,
                    name: "site".to_string(),
                },
                index: ".env".to_string(),
                rule: NO_DOTFILE_SEGMENT,
            }))
        );
    }

    /// One mistake, one line: the retired reader lowers its flag to the index,
    /// and a bad index is already refused on the route's own block, so the
    /// mount check does not report the same name twice.
    #[test]
    fn a_bad_index_beside_the_retired_flag_is_reported_once() {
        assert_eq!(
            retired_static("root = '.'\nindex = '.env'\nspa = true\n").validate(20_000),
            Err(ManifestErrors::from(ManifestError::StaticIndexInvalid {
                location: Location::Route {
                    index: 0,
                    name: "site".to_string(),
                },
                index: ".env".to_string(),
                rule: NO_DOTFILE_SEGMENT,
            }))
        );
    }

    /// §10.3: two routes of one sandbox are two endpoints of one view, so one
    /// name is not a duplicate; a name that reappears in another sandbox is two
    /// views with one name.
    #[test]
    fn two_endpoints_of_one_view_are_not_a_duplicate() {
        let route = |name: &str, sandbox: u32| {
            Route::new(
                name.to_string(),
                vec!["/cart".to_string()],
                Some(vec!["GET".to_string()]),
                Vec::new(),
                false,
                false,
                BTreeMap::new(),
            )
            .with_sandbox(sandbox)
        };
        let manifest = |routes: Vec<Route>| {
            Manifest::assemble(
                1,
                Vec::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                routes,
                Vec::new(),
            )
        };

        // One `view cart` with two endpoints: legal, and one uid.
        assert_eq!(
            manifest(vec![route("cart", 0), route("cart", 0)]).validate(20_000),
            Ok(Vec::new())
        );

        // The same name in a second sandbox is two views called `cart`.
        assert_eq!(
            manifest(vec![route("cart", 0), route("cart", 1)]).validate(20_000),
            Err(ManifestErrors::from(ManifestError::DuplicateRouteName {
                name: "cart".to_string(),
                first: 0,
                second: 1,
            }))
        );
    }

    /// A manifest with one view per `body` spelling this gate cares about
    /// (`MB.3.53`, `ADR-0011`, §10.1).
    const INGRESS_MATRIX: &str = r#"
view upload POST "/upload" {
    body raw
}

view assets POST "/assets" {
    body raw unvalidated
}

view checkout POST "/checkout" {
    body json
}

view plain POST "/plain" {}
"#;

    fn manifest(source: &str) -> Manifest {
        Manifest::from_blastfile_str(source).unwrap()
    }

    /// The gate has two answers and both are measured: an unacknowledged `raw`
    /// view is refused with §10.2's sentence — naming the view and the node to
    /// write — and every other shape — acknowledged `raw`, a structured codec,
    /// no `body` at all — is silent (`ADR-0011`).
    #[test]
    fn raw_ingress_without_unvalidated_is_the_only_refused_shape() {
        let refusals = manifest(INGRESS_MATRIX).raw_ingress_refusals();
        assert_eq!(
            refusals,
            [
                "view \"upload\": body raw hands the request body to the application unvalidated, \
                 and the production path refuses it without the acknowledgement; write `body raw \
                 unvalidated` in that view, or declare the codec it takes (json, form, none)",
            ],
            "§10.2, verbatim"
        );
    }

    /// The acknowledgement is the whole decision, and it is the one thing that
    /// opens the gate: the same view with `unvalidated` is silent.
    #[test]
    fn the_acknowledgement_is_what_opens_the_gate() {
        let acknowledged =
            manifest("view upload POST \"/upload\" {\n    body raw unvalidated\n}\n");
        assert_eq!(acknowledged.raw_ingress_refusals(), Vec::<String>::new());
        let unacknowledged = manifest("view upload POST \"/upload\" {\n    body raw\n}\n");
        assert_eq!(unacknowledged.raw_ingress_refusals().len(), 1);
        assert!(
            unacknowledged.raw_ingress_refusals()[0].contains("body raw unvalidated"),
            "the sentence names the node to write"
        );
    }

    /// Every offender is named, in route order: a manifest edited by hand or by
    /// an agent often has more than one, and a boot that named only the first
    /// would send the reader round the loop this task exists to close.
    #[test]
    fn every_unacknowledged_view_is_named_in_route_order() {
        let source = r#"
view first POST "/first" {
    body raw
}

view second POST "/second" {
    body raw
}
"#;
        let refusals = manifest(source).raw_ingress_refusals();
        assert_eq!(refusals.len(), 2, "{refusals:?}");
        assert!(refusals[0].starts_with("view \"first\": "), "{refusals:?}");
        assert!(refusals[1].starts_with("view \"second\": "), "{refusals:?}");
        assert!(
            !refusals[0].contains("\"second\"") && !refusals[1].contains("\"first\""),
            "each sentence names one view: {refusals:?}"
        );
    }

    /// The warning is not the gate and the gate is not the warning: `dev` keeps
    /// the warning for both raw shapes (acknowledged or not), while the
    /// production refusal only ever names the unacknowledged one.
    #[test]
    fn the_boot_warning_still_fires_for_an_acknowledged_route() {
        let parsed = manifest(INGRESS_MATRIX);
        let warnings = super::warnings(&parsed);
        let raw: Vec<&str> = warnings
            .iter()
            .filter_map(|warning| match warning {
                crate::ManifestWarning::RawIngress { route } => Some(route.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(raw, ["upload", "assets"], "{warnings:?}");
        assert_eq!(parsed.raw_ingress_refusals().len(), 1);
    }

    #[test]
    fn route_names() {
        assert!(valid_route_name("a"));
        assert!(valid_route_name("product-detail"));
        assert!(valid_route_name("v2_api"));
        assert!(valid_route_name("9lives"));
        assert!(!valid_route_name(""));
        assert!(!valid_route_name("-leading"));
        assert!(!valid_route_name("_leading"));
        assert!(!valid_route_name("Upper"));
        assert!(!valid_route_name("with space"));
        assert!(!valid_route_name("with/slash"));
    }

    #[test]
    fn methods() {
        assert!(valid_method("GET"));
        assert!(valid_method("HEAD"));
        assert!(valid_method("MKCOL"));
        assert!(valid_method("M-SEARCH"));
        assert!(!valid_method("get"));
        assert!(!valid_method("Get"));
        assert!(!valid_method(""));
        assert!(!valid_method("GE T"));
        assert!(!valid_method("GET/"));
        assert!(!valid_method("GÉT"));
    }
}
