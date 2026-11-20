//! `dead_grant` and `unexercised_route` (`R34`, `R35`).
//!
//! A grant is dead when no observed statement of an exercised route matched it.
//! The *set* of matches is the shared diff's ([`super::ledger::Ledger`]); this module
//! applies `check`'s grouping: a route's own `queries` entry is reported on the
//! route, a group's once per group, and an unexercised route is named instead of
//! having all its grants called dead.

use std::collections::{BTreeMap, BTreeSet};

use blastit_manifest::Manifest;
use blastit_sqlpolicy::normalize;

use super::diff::{grant_locator, trait_locator};
use super::findings::{Finding, Fix, Severity};
use super::ledger::Ledger;
use super::obs::CheckObsV1;
/// `dead_grant` for the routes' own `queries` entries (`R34`).
///
/// `requests` and the matched set both come from the [`Ledger`], so this reads
/// the one grant diff the plugin report reads.
pub(crate) fn dead_route_grants(
    manifest: &Manifest,
    ledger: &Ledger<'_>,
    findings: &mut Vec<Finding>,
) {
    let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
    for index in 0..manifest.route_count() {
        if ledger.requests(index) == 0 {
            // An unexercised route says so once; its grants are not dead.
            continue;
        }
        let Some(route) = manifest.route(index) else {
            continue;
        };
        for (proxy, grant) in manifest.effective_grants(index) {
            let Some(pg) = grant.as_postgresql() else {
                continue;
            };
            for query in pg.queries() {
                let normalized = normalize(query);
                if normalized.is_empty() || ledger.matched_contains(index, &normalized) {
                    continue;
                }
                // A group's query is reported once, by `dead_group_grants`.
                if manifest.is_query_covered_by_tables_or_groups(index, &proxy, &normalized) {
                    continue;
                }
                let key = (
                    route.name().to_string(),
                    proxy.as_str().to_string(),
                    normalized.clone(),
                );
                if !seen.insert(key) {
                    continue;
                }
                findings.push(Finding::DeadGrant {
                    severity: Severity::Error,
                    route: route.name().to_string(),
                    group: None,
                    proxy: proxy.as_str().to_string(),
                    sql: normalized,
                    fix: Fix::remove_query(grant_locator(route.name(), proxy.as_str())),
                });
            }
        }
    }
}

/// `dead_grant` for attached groups, once per group/proxy/statement (`R34`).
pub(crate) fn dead_group_grants(
    manifest: &Manifest,
    ledger: &Ledger<'_>,
    findings: &mut Vec<Finding>,
) {
    // Which routes attach each group; a global group attaches to all of them.
    let mut attaching: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for name in manifest.global_groups() {
        attaching
            .entry(name.clone())
            .or_default()
            .extend(0..manifest.route_count());
    }
    for (index, route) in manifest.routes().iter().enumerate() {
        for name in route.groups() {
            attaching.entry(name.clone()).or_default().insert(index);
        }
    }

    for (name, indices) in attaching {
        let Some(group) = manifest.group(&name) else {
            continue;
        };
        let exercised: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|index| ledger.requests(*index) > 0)
            .collect();
        let Some(first) = exercised.first().copied() else {
            // The group is attached only to routes nothing reached: those
            // routes are `unexercised`, and their grants are not dead yet.
            continue;
        };
        for (proxy, grant) in group.grants() {
            let Some(pg) = grant.as_postgresql() else {
                continue;
            };
            for query in pg.queries() {
                let normalized = normalize(query);
                if normalized.is_empty() {
                    continue;
                }
                let used = exercised
                    .iter()
                    .any(|index| ledger.matched_contains(*index, &normalized));
                if used {
                    continue;
                }
                let route_name = manifest
                    .route(first)
                    .map(|route| route.name().to_string())
                    .unwrap_or_default();
                findings.push(Finding::DeadGrant {
                    severity: Severity::Error,
                    route: route_name,
                    group: Some(name.clone()),
                    proxy: proxy.as_str().to_string(),
                    sql: normalized,
                    fix: Fix::remove_query(trait_locator(&name)),
                });
            }
        }
    }
}

/// `unexercised_route`, suppressed when the suite observed no request at all
/// (`R35`: `no_observations` says that instead).
pub(crate) fn unexercised_routes(
    manifest: &Manifest,
    ledger: &Ledger<'_>,
    observations: &CheckObsV1,
    findings: &mut Vec<Finding>,
) {
    if observations.http().is_empty() {
        return;
    }
    for index in 0..manifest.route_count() {
        if ledger.requests(index) > 0 {
            continue;
        }
        let Some(route) = manifest.route(index) else {
            continue;
        };
        // `R14`: a static route answers from the filesystem before anything
        // reaches the app, so a suite cannot exercise it in-process. Its
        // completeness warning is `unrequested_static_roots`, on stderr — never
        // a finding, never `ok`, never the exit code.
        if route.static_policy().is_some() {
            continue;
        }
        findings.push(Finding::UnexercisedRoute {
            severity: Severity::Error,
            route: route.name().to_string(),
            fix: Fix::add_test(),
        });
    }
}
