//! Validation of a parsed manifest: everything that needs the whole document
//! (§6).

use std::collections::{BTreeMap, BTreeSet, HashSet, btree_map::Entry};

use crate::error::{
    Location, ManifestError, ManifestErrors, ManifestWarning, Position, quoted, regex_message,
};
use crate::model::{Manifest, PgGrant, PhpPolicy, ProxyName, Route, StaticPolicy};
use crate::routing::compile_pattern;
use crate::uid::UidLayout;

use blastit_ingress::{BodyCodec, IngressPolicy};
use blastit_sqlpolicy::{
    KNOWN_STATIC_SETUP_SQL, MAX_TEMPLATE_PARSES, TemplateError, check_template, has_quoted_literal,
    is_system_table, literal_offset, normalize,
};

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
/// A `fallback "<file>"` answers every miss itself, and `CONNECT`/`TRACE` are
/// the two methods a front controller cannot serve.
const CONNECT_CANNOT_REACH_WORKER: &str = "CONNECT cannot reach a PHP worker";
const TRACE_CANNOT_REACH_WORKER: &str = "TRACE cannot reach a PHP worker";

/// The largest `[route.php]` `workers` cap (`ADR-0010` §2).
///
/// A grammar bound, not the deployment's: a value above the route's per-view
/// budget is a separate boot check that has a `Config` (and so is not
/// `validate`'s, which must keep parsing on a machine with no deployment).
const MAX_WORKERS: u32 = 64;

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
/// quoted literal (`R24`, `ADR-0003`), and it has to parse as one statement
/// once its template is expanded (`R31`–`R35`, §13.4.3).
///
/// An exact grant is compared and run as written, so a statement carrying a
/// value would both commit that value to the repository and match only that one
/// value. `--learn` refuses such statements instead of writing them, and
/// `KNOWN_STATIC_SETUP_SQL` is the one fixed pair of protocol statements that
/// may keep its literal. Neither message prints the entry: printing it is what
/// the rule forbids.
///
/// The checks run in one `if … else if … else` chain — redacted `…`, then
/// inline literal, then the template — because **one entry gets one sentence**
/// (§18.1): a statement holding `…` does not parse, so running the template
/// check after it would print two sentences for one defect. Every site comes
/// from the entry's [`crate::QuerySite`] and the offset the check reported; a
/// grant no document wrote has `Position::NONE` and renders without a prefix.
fn check_queries(manifest: &Manifest, errors: &mut Vec<ManifestError>) {
    // §13.4.2's parse cap is per file, so the set of statements already parsed
    // is shared by every grant the two loops below visit. The keys borrow the
    // document, so the set costs one pointer per distinct entry.
    let mut parsed: HashSet<&str> = HashSet::new();
    for (name, group) in manifest.groups() {
        for (proxy, grant) in group.grants() {
            if let Some(grant) = grant.as_postgresql() {
                check_grant_queries(
                    &Location::Group(name.clone()),
                    proxy,
                    grant,
                    &mut parsed,
                    errors,
                );
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
                check_grant_queries(&location, proxy, grant, &mut parsed, errors);
            }
        }
    }
}

/// The query rules of one grant, in the order §18.1 pins.
fn check_grant_queries<'a>(
    location: &Location,
    proxy: &ProxyName,
    grant: &'a PgGrant,
    parsed: &mut HashSet<&'a str>,
    errors: &mut Vec<ManifestError>,
) {
    for (rule, query) in grant.queries().iter().enumerate() {
        let site = grant.query_sites().get(rule);
        // The raw text the developer wrote, before `normalize`: its offsets are
        // the file's, while the normalised text's are not (§13.4.5).
        let raw = site.map_or(query.as_str(), |site| site.raw.as_str());
        let at = |offset: u32| site.map_or(Position::NONE, |site| site.position_at(offset));
        // The two shape refusals keep reading the normalised statement, which
        // is what they have always compared; only their site comes from `raw`.
        let normalised = normalize(query);
        if normalised.contains('…') {
            errors.push(ManifestError::RedactedQueryPlaceholder {
                location: location.clone(),
                proxy: proxy.clone(),
                rule,
                at: at(redaction_offset(raw)),
            });
        } else if has_quoted_literal(&normalised)
            && !KNOWN_STATIC_SETUP_SQL.contains(&normalised.as_str())
        {
            errors.push(ManifestError::LiteralQuery {
                location: location.clone(),
                proxy: proxy.clone(),
                rule,
                // The first literal's quote, or the statement's own origin
                // when there is none: never `NONE` for a parsed document
                // (§18.1).
                at: at(literal_offset(raw).unwrap_or(0)),
            });
        } else if parsed.insert(raw) {
            // §13.4.2: the 4097th distinct entry in a file is refused at its
            // own origin instead of being parsed, which keeps a huge file from
            // costing one parse per entry.
            if parsed.len() > MAX_TEMPLATE_PARSES {
                let refusal = TemplateError::too_many_queries();
                errors.push(ManifestError::QueryTemplate {
                    at: at(refusal.offset()),
                    kind: refusal.kind(),
                });
            } else if let Err(error) = check_template(raw) {
                errors.push(ManifestError::QueryTemplate {
                    at: at(error.offset()),
                    kind: error.kind(),
                });
            }
        }
    }
}

/// The character offset of the first `…` in a statement, for the
/// `RedactedQueryPlaceholder` refusal's site (§18.1).
///
/// A `…` is not transitive through any escape, so the first one is the
/// placeholder a copied `sql.denied` line left behind. There is none only for a
/// grant built by [`PgGrant::new`], whose stored text an author never wrote.
fn redaction_offset(raw: &str) -> u32 {
    raw.chars()
        .position(|character| character == '…')
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(0)
}

/// Route names, path regexes and method tokens.
///
/// Names are compared **by sandbox**, not by position (§10.3): one `view` with
/// several endpoints lowers to several routes that share one name and one
/// sandbox, and those are not duplicates. A name that reappears in a different
/// sandbox is, and the message names the two routes so an operator can find
/// them.
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
        check_static_declarations(manifest, route, policy, location, errors);
    }
}

/// `root` is a relative path that stays in the manifest's directory (`R2`).
///
/// An empty string is not a relative path either, so it shares the rule: the
/// two are the same mistake — a root that names no directory beneath the
/// manifest file.
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

/// A static route that can never reach the app may not declare anything only
/// the app could run (`R4`).
///
/// A `fallback "<file>"` answers every miss itself, and a manifest without an
/// `app` has nothing to forward to. A `body` codec, a `prefork`, an inline grant
/// or an inline `use` on such a route is dead weight that reads like a
/// capability, so it is refused with the reason and the fix — naming the
/// directive as a `Blastfile` writes it. Groups inherited from a trait are not
/// re-checked: they are attached to the route, not declared by it.
fn check_static_declarations(
    manifest: &Manifest,
    route: &Route,
    policy: &StaticPolicy,
    location: Location,
    errors: &mut Vec<ManifestError>,
) {
    // A route with a front controller does reach an executor, so nothing on it
    // is dead; a `fallback` beside `entrypoint php` is refused by `check_php`
    // instead (the retired key kept both arms the same way).
    if route.php().is_some() {
        return;
    }
    let why = if let Some(fallback) = &policy.fallback {
        format!("fallback {fallback:?} never reaches the app")
    } else if manifest.app().is_none() {
        "the manifest declares no app".to_string()
    } else {
        return;
    };

    let mut dead = |key: String| {
        errors.push(ManifestError::StaticDeadDeclaration {
            location: location.clone(),
            key,
            why: why.clone(),
        });
    };
    if let Some(ingress) = route.ingress() {
        dead(body_directive(&ingress));
    }
    if route.prefork() {
        dead("prefork".to_string());
    }
    for store in route.grants().keys() {
        dead(store_block(store.as_str()));
    }
    if !route.groups().is_empty() {
        dead("use".to_string());
    }
}

/// The `body` directive an ingress policy is written as (§10.1), for a refusal
/// that names the declaration the operator has to delete.
fn body_directive(policy: &IngressPolicy) -> String {
    let codec = match policy.body {
        BodyCodec::Json => "json",
        BodyCodec::Form => "form",
        BodyCodec::None => "none",
        BodyCodec::Raw => "raw",
    };
    if policy.body == BodyCodec::Raw && policy.allow_raw {
        format!("body {codec} unvalidated")
    } else {
        format!("body {codec}")
    }
}

/// The `postgres` block a grant on `store` is written in (§13.1): `postgres {
/// … }` for the unnamed store, `postgres <name> { … }` otherwise.
fn store_block(store: &str) -> String {
    if store == crate::blastfile::DEFAULT_STORE {
        "postgres { … }".to_string()
    } else {
        format!("postgres {store} {{ … }}")
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
/// (`ADR-0010` §2): a `fallback "<file>"` answers every miss itself, and
/// `CONNECT`/`TRACE` are the two methods a front controller cannot serve.
///
/// The sentence names the declaration as a `Blastfile` writes it — `fallback
/// "<file>"`, never the retired `spa` key that directive replaced (§9.3).
fn check_php_declarations(route: &Route, location: &Location, errors: &mut Vec<ManifestError>) {
    let mut dead = |key: String, why: String| {
        errors.push(ManifestError::PhpDeadDeclaration {
            location: location.clone(),
            key,
            why,
        });
    };
    if let Some(fallback) = route
        .static_policy()
        .and_then(|policy| policy.fallback.as_deref())
    {
        dead(
            format!("fallback {fallback:?}"),
            format!("fallback {fallback:?} never reaches the front controller"),
        );
    }
    if let Some(methods) = route.methods() {
        if methods.iter().any(|method| method == "CONNECT") {
            dead(
                "CONNECT".to_string(),
                CONNECT_CANNOT_REACH_WORKER.to_string(),
            );
        }
        if methods.iter().any(|method| method == "TRACE") {
            dead("TRACE".to_string(), TRACE_CANNOT_REACH_WORKER.to_string());
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
