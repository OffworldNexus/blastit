//! Validation of a parsed manifest: everything that needs the whole document
//! (§6).

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use crate::error::{Location, ManifestError, ManifestErrors, ManifestWarning, regex_message};
use crate::model::{Manifest, ProxyName};
use crate::routing::compile_pattern;
use crate::uid::UidLayout;

/// Validates a manifest and collects its warnings.
pub(crate) fn validate(
    manifest: &Manifest,
    base: u32,
) -> Result<Vec<ManifestWarning>, ManifestErrors> {
    let mut errors = Vec::new();
    check_groups(manifest, &mut errors);
    check_grants(manifest, &mut errors);
    check_routes(manifest, &mut errors);
    check_uids(manifest, base, &mut errors);

    if errors.is_empty() {
        Ok(warnings(manifest))
    } else {
        Err(ManifestErrors::from(errors))
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

/// The non-fatal findings: a group attached nowhere and a proxy nobody grants.
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
