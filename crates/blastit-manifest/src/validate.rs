//! Validation of a parsed manifest: everything that needs the whole document
//! (§6).

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use crate::error::{Location, ManifestError, ManifestErrors, ManifestWarning, regex_message};
use crate::model::{Manifest, PgGrant, ProxyName, Route, StaticPolicy};
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

#[cfg(test)]
mod tests {
    use super::{valid_method, valid_route_name};

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
