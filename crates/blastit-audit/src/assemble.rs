//! The legacy pytest report document, assembled from the grant diff.
//!
//! Every route the manifest declares gets a block: its requests, what it tried
//! to reach on the network, and one entry per `(view, proxy)` pair with the
//! statements it ran but was not granted, the grants nothing matched, whether it
//! needed transaction control, and the table grant that would cover what it ran.
//! The document is `fold`, the pytest plugin and `--blastit-strict`'s contract,
//! so this module renders the [`Ledger`] without changing any answer it holds.

use std::collections::{BTreeMap, BTreeSet};

use blastit_manifest::{Manifest, ProxyName};
use blastit_sqlpolicy::TableGrants;
use serde_json::{Map, Value, json};

use crate::VERSION;
use crate::analyze::LOCK_NEEDS_A_COLUMN;
use crate::diff::{GrantCheck, grant_of, proxy_names};
use crate::error::AuditError;
use crate::ledger::{
    EMPTY_MATCHED, EgressTrace, Ledger, Occurrence, UnroutedTrace, table_grants_json,
};
use crate::observe::Observations;
use crate::pytest::Summary;
use crate::sections::{
    GlobalState, group_queries, live_only_proxies, own_queries, setup_section,
    unused_global_section, used_statements,
};

/// Everything a report is assembled from that is not an observation or a total.
///
/// One value instead of three loose arguments: the manifest the report
/// describes, the diff of the run, and what its *global* groups grant.
#[derive(Clone, Copy)]
pub(crate) struct Run<'a> {
    pub manifest: &'a Manifest,
    pub ledger: &'a Ledger<'a>,
    pub global: &'a GlobalState,
}

/// Builds the report document out of the traces of the request loop.
pub(crate) fn assemble(
    run: &Run<'_>,
    warnings: &[blastit_manifest::ManifestWarning],
    observations: &Observations,
    unrouted: &BTreeMap<(String, String), UnroutedTrace>,
    summary: Summary,
) -> Result<Value, AuditError> {
    let Run {
        manifest,
        ledger,
        global,
    } = *run;
    let mut routes = Vec::with_capacity(manifest.route_count());
    let mut unexercised = Vec::new();
    let mut needs_transaction_control = Vec::new();
    let mut needs_egress = Vec::new();
    let mut exercised = 0usize;
    let mut missing = 0usize;
    let mut missing_distinct = 0usize;
    let mut unused = 0usize;
    // The statements psycopg runs as a connection opens (§6): they belong to no
    // request, so they are not a route's to call dead — a trait attached to
    // every view grants them, and `connection_setup` is where the report shows
    // they are used. Built before the route loop because `unused_here` reads it.
    let setup = setup_section(manifest, &observations.connection_setup)?;
    // The `(proxy, statement)` pairs some route matched: a global group's
    // statement is used as soon as *one* route ran it.
    let mut global_matched: BTreeSet<(ProxyName, String)> = BTreeSet::new();

    for index in 0..manifest.route_count() {
        let (Some(route), trace) = (manifest.route(index), ledger.route(index)) else {
            continue;
        };
        let requests = trace.map_or(0, |trace| trace.requests);
        if requests == 0 {
            // Nothing ran here: naming the route is the signal, and listing its
            // grants as unused would say the same thing five times over.
            unexercised.push(json!(route.name()));
            routes.push(json!({
                "name": route.name(),
                "path": route.path(),
                "requests": 0,
                "needs_transaction_control": false,
                "egress": {"observed": [], "needed": false},
                "proxies": [],
            }));
            continue;
        }
        exercised += 1;

        // A route that only a live server's log saw: the requests are real, but
        // the statements that *ran* are not in it — only the denied ones are.
        // So nothing can be said about the route's grants having matched (D3c).
        let live_only = trace.is_some_and(|trace| trace.suite_requests == 0);

        let mut egress: Vec<Value> = Vec::new();
        let mut attempts: Vec<(&(String, u16), &EgressTrace)> = trace
            .map(|trace| trace.egress().iter().collect())
            .unwrap_or_default();
        attempts.sort_by(|(left, left_trace), (right, right_trace)| {
            right_trace
                .count
                .cmp(&left_trace.count)
                .then_with(|| left.cmp(right))
        });
        for ((host, port), attempt) in attempts {
            egress.push(json!({
                "target": format!("{host}:{port}"),
                "host": host,
                "port": port,
                "family": attempt.family,
                "count": attempt.count,
                "tests": attempt.tests,
            }));
        }
        let egress_needed = !egress.is_empty() && !route.egress();
        if egress_needed {
            needs_egress.push(json!(route.name()));
        }

        let mut proxies = Vec::new();
        let mut route_needs_transaction_control = false;
        for proxy in proxy_names(manifest, index, trace) {
            let grant = grant_of(manifest, index, proxy.as_str());
            let policy = GrantCheck::of(grant.as_ref());
            let observed = trace.and_then(|trace| trace.proxy(&proxy));
            let matched = observed.map_or(&EMPTY_MATCHED, |observed| &observed.matched);
            for sql in matched {
                global_matched.insert((proxy.clone(), sql.clone()));
            }

            let mut entries: Vec<(&String, &Occurrence)> = observed
                .map(|observed| observed.missing.iter().collect())
                .unwrap_or_default();
            entries.sort_by(|(left, left_trace), (right, right_trace)| {
                right_trace
                    .count
                    .cmp(&left_trace.count)
                    .then_with(|| left.cmp(right))
            });
            let entries: Vec<Value> = entries
                .into_iter()
                .map(|(sql, trace)| {
                    missing += trace.count;
                    missing_distinct += 1;
                    json!({
                        "sql": sql,
                        "count": trace.count,
                        "wire_sql": trace.wire_sql,
                        "tests": trace.tests,
                        "paths": trace.paths,
                    })
                })
                .collect();

            // A statement this route only gets through a *global* group is not
            // this route's unused line: that group is attached to every route,
            // so the same grant would be reported dead once per route. What a
            // route *does* own — its inline grant and the groups it names — is
            // reported here; the global groups' statements are reported once,
            // below, when no route matched them (D3c).
            let own = own_queries(manifest, index, &proxy);
            let from_group = group_queries(manifest, index, &proxy);
            let unused_here: Vec<&String> = if live_only {
                // Nothing observed this route but its own log: no grant of it
                // can be called dead.
                Vec::new()
            } else {
                policy
                    .granted()
                    .iter()
                    .filter(|query| {
                        !matched.contains(query.as_str())
                            && !setup
                                .accounted
                                .contains(&(proxy.clone(), (*query).clone()))
                            && own.contains(query.as_str())
                            // A route's own `queries` entry a table grant
                            // already covers is not dead (`R34`): the shared
                            // helper is the one `blastit check` reads. A
                            // group's statement keeps this report's older
                            // per-route accounting, which `unused_global` and
                            // the route's group own.
                            && (from_group.contains(query.as_str())
                                || !manifest.is_query_covered_by_tables_or_groups(
                                    index, &proxy, query,
                                ))
                    })
                    .collect()
            };
            unused += unused_here.len();

            let observed_control = observed.is_some_and(|observed| observed.transaction_control);
            let granted_control = policy.transaction_control();
            let needed = observed_control && !granted_control;
            route_needs_transaction_control |= needed;

            let (table_grants, unanalysable) = observed.map_or_else(
                || (TableGrants::default(), Vec::new()),
                |observed| {
                    let mut tables = observed.table_grants.clone();
                    // A lock one statement could not name a column for may yet be
                    // named from what another statement of the pair reads, which
                    // is what makes the merged grant better than any one of them
                    // (§6, D3c).
                    tables.pick_locking_columns();
                    let nameless: BTreeSet<&str> = tables.locks_without_a_column().collect();
                    let mut unanalysable: Vec<Value> = observed
                        .unanalysable
                        .iter()
                        .map(|(sql, reason)| json!({"sql": sql, "reason": reason}))
                        .collect();
                    for (sql, locked) in &observed.nameless_locks {
                        if locked.iter().any(|table| nameless.contains(table.as_str())) {
                            unanalysable.push(json!({"sql": sql, "reason": LOCK_NEEDS_A_COLUMN}));
                        }
                    }
                    (tables, unanalysable)
                },
            );

            proxies.push(json!({
                "name": proxy.as_str(),
                "observed": observed.map_or(0, |observed| observed.executions),
                "missing": entries,
                "unused": unused_here,
                "transaction_control": {
                    "observed": observed_control,
                    "granted": granted_control,
                    "needed": needed,
                },
                "table_grants": table_grants_json(
                    &table_grants,
                    route.name(),
                    proxy.as_str(),
                    unanalysable,
                ),
            }));
        }
        if route_needs_transaction_control {
            needs_transaction_control.push(json!(route.name()));
        }

        routes.push(json!({
            "name": route.name(),
            "path": route.path(),
            "requests": requests,
            "needs_transaction_control": route_needs_transaction_control,
            "egress": {"observed": egress, "needed": egress_needed},
            "proxies": proxies,
        }));
    }

    let mut unrouted_entries: Vec<&UnroutedTrace> = unrouted.values().collect();
    unrouted_entries.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.method.cmp(&right.method))
            .then_with(|| left.path.cmp(&right.path))
    });
    let unrouted_count: usize = unrouted_entries.iter().map(|entry| entry.count).sum();
    let unrouted_entries: Vec<Value> = unrouted_entries
        .into_iter()
        .map(|entry| {
            json!({
                "method": entry.method,
                "path_info": entry.path,
                "count": entry.count,
                "statements": entry.statements,
                "tests": entry.tests,
            })
        })
        .collect();

    let proxies: Map<String, Value> = manifest
        .proxies()
        .iter()
        .map(|(name, proxy)| (name.as_str().to_string(), json!(proxy.protocol().as_str())))
        .collect();
    let warnings: Vec<Value> = warnings
        .iter()
        .map(|warning| json!(warning.to_string()))
        .collect();
    let unrecognised_total: u64 = observations
        .unrecognised
        .iter()
        .map(|entry| entry.count)
        .sum();
    let unrecognised: Vec<Value> = observations
        .unrecognised
        .iter()
        .map(|entry| {
            json!({
                "query": entry.query,
                "alias": entry.alias,
                "count": entry.count,
            })
        })
        .collect();
    let unused_global = unused_global_section(
        global,
        &used_statements(&global_matched, &setup),
        &live_only_proxies(manifest, ledger),
    );
    let clean = missing == 0
        && unrouted_count == 0
        && needs_transaction_control.is_empty()
        && setup.ungranted == 0
        && needs_egress.is_empty()
        && unrecognised_total == 0
        && observations.warnings.is_empty();

    Ok(json!({
        "version": VERSION,
        "manifest": {
            "routes": manifest.route_count(),
            "proxies": proxies,
            "warnings": warnings,
        },
        "routes": routes,
        "unrouted": unrouted_entries,
        "unexercised_routes": unexercised,
        "unused_global": unused_global,
        "connection_setup": setup.value,
        "unrecognised": {
            "statements": unrecognised,
            "count": unrecognised_total,
        },
        "plugin_warnings": observations.warnings,
        "totals": {
            "requests": summary.requests,
            "routed": summary.routed,
            "unrouted": unrouted_count,
            "routes": manifest.route_count(),
            "routes_exercised": exercised,
            "statements": summary.statements,
            "missing": missing,
            "missing_distinct": missing_distinct,
            "unused": unused,
            "unused_global": unused_global["count"],
            "needs_transaction_control": needs_transaction_control,
            "connection_setup": setup.statements,
            "connection_setup_ungranted": setup.ungranted,
            "egress": needs_egress.len(),
            "unrecognised": unrecognised_total,
            "plugin_warnings": observations.warnings.len(),
            "clean": clean,
        },
    }))
}
