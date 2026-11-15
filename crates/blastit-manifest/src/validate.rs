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
fn check_routes(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
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
                slot.insert(index);
            }
            Entry::Occupied(first) => errors.push(ManifestError::DuplicateRouteName {
                name: name.to_string(),
                first: *first.get(),
                second: index,
            }),
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
    } else if index.split('/').any(|segment| segment.starts_with('.')) {
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
    let why = if policy.spa {
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
    if route.static_policy().is_some_and(|policy| policy.spa) {
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
fn check_uids(manifest: &Manifest, base: u32, errors: &mut Vec<ManifestError>) {
    let layout = UidLayout::new(base);
    if layout.dbproxy() >= UidLayout::MAX_UID_EXCLUSIVE {
        errors.push(ManifestError::UidBaseTooHigh {
            base,
            uid: layout.dbproxy(),
            max: UidLayout::MAX_UID_EXCLUSIVE,
        });
    }
    if let Some(last) = manifest.route_count().checked_sub(1) {
        let uid = layout.view(last);
        if uid >= UidLayout::MAX_UID_EXCLUSIVE {
            errors.push(ManifestError::TooManyRoutes {
                index: last,
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

/// The non-fatal findings: a group attached nowhere, a proxy nobody grants, and
/// an unrestricted route.
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

    warnings
}

impl Manifest {
    /// The production path's refusal sentences (`ADR-0011`), one per route that
    /// declares `body = "raw"` without `allow_raw = true`, in route order.
    ///
    /// `raw` is the one ingress declaration the runtime does not gate: the bytes
    /// reach the application's own parser unchanged (`blastit_ingress::
    /// plan_body`'s `StreamPassthrough`). The acknowledgement in the manifest is
    /// therefore the whole of the decision, and on the production path an
    /// unacknowledged one is a **boot refusal** rather than the warning it used
    /// to be — the boot names the route and the key before a listener exists.
    /// `blastit dev` keeps the warning (`ManifestWarning::RawIngress`, emitted
    /// from `warnings`) because it is the local simulation, and
    /// `blastit check` keeps the `raw_ingress` finding so CI reports the same
    /// rule before anyone deploys.
    ///
    /// Acknowledged routes contribute nothing, so an empty result is the gate
    /// being open — the caller refuses on `!is_empty()`, never on the text.
    pub fn raw_ingress_refusals(&self) -> Vec<String> {
        self.routes()
            .iter()
            .filter_map(|route| {
                let policy = route.ingress()?;
                (policy.body == BodyCodec::Raw && !policy.allow_raw).then(|| {
                    format!(
                        "route \"{}\": body = \"raw\" hands the request body to the application \
                         unvalidated, and the production path refuses it without the \
                         acknowledgement; add `allow_raw = true` to that [route.ingress] table, or \
                         declare the codec the route takes (json, form, none) (ADR-0011)",
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

    use super::{valid_method, valid_route_name};
    use crate::model::Manifest;

    /// A manifest with one route per `[route.ingress]` spelling this gate cares
    /// about (`MB.3.53`, `ADR-0011`).
    const INGRESS_MATRIX: &str = r#"
version = 1

[[route]]
name = "upload"
path = ['/upload']
[route.ingress]
body = "raw"

[[route]]
name = "assets"
path = ['/assets']
[route.ingress]
body = "raw"
allow_raw = true

[[route]]
name = "checkout"
path = ['/checkout']
[route.ingress]
body = "json"

[[route]]
name = "plain"
path = ['/plain']
"#;

    fn manifest(source: &str) -> Manifest {
        Manifest::from_toml_str(source).unwrap()
    }

    /// The gate has two answers and both are measured: an unacknowledged `raw`
    /// route is refused naming the route **and** the key, and every other shape
    /// — acknowledged `raw`, a structured codec, no `[route.ingress]` at all —
    /// is silent (`ADR-0011`).
    #[test]
    fn raw_ingress_without_allow_raw_is_the_only_refused_shape() {
        let refusals = manifest(INGRESS_MATRIX).raw_ingress_refusals();
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        let sentence = &refusals[0];
        assert!(sentence.contains("route \"upload\""), "{sentence}");
        assert!(sentence.contains("allow_raw = true"), "{sentence}");
        assert!(sentence.contains("body = \"raw\""), "{sentence}");
        assert!(
            !sentence.contains("route \"assets\""),
            "an acknowledged raw route is not refused: {sentence}"
        );
    }

    /// `allow_raw = false` is not an acknowledgement: the key is present and
    /// says no, which is the same boot as omitting it.
    #[test]
    fn allow_raw_false_is_refused() {
        let source = r#"
version = 1
[[route]]
name = "upload"
path = ['/upload']
[route.ingress]
body = "raw"
allow_raw = false
"#;
        let refusals = manifest(source).raw_ingress_refusals();
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        assert!(refusals[0].contains("route \"upload\""), "{refusals:?}");
    }

    /// Every offender is named, in route order: a manifest edited by hand or by
    /// an agent often has more than one, and a boot that named only the first
    /// would send the reader round the loop this task exists to close.
    #[test]
    fn every_unacknowledged_route_is_named_in_route_order() {
        let source = r#"
version = 1
[[route]]
name = "first"
path = ['/first']
[route.ingress]
body = "raw"

[[route]]
name = "second"
path = ['/second']
[route.ingress]
body = "raw"
"#;
        let refusals = manifest(source).raw_ingress_refusals();
        assert_eq!(refusals.len(), 2, "{refusals:?}");
        assert!(refusals[0].contains("route \"first\""), "{refusals:?}");
        assert!(refusals[1].contains("route \"second\""), "{refusals:?}");
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
