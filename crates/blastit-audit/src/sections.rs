//! The report's aggregate sections, and what the *global* groups grant.
//!
//! A global group is attached to every route at once (`groups = […]` at the top
//! level), so a statement it grants is in every route's effective grant and no
//! route can claim it. Reporting it as unused per route would repeat one dead
//! grant once per route; reporting it once — here — says the same thing
//! honestly. The same module owns the `connection_setup` section: those
//! statements belong to no request, so what matters is not which route ran them
//! but which routes do not allow them.

use std::collections::{BTreeMap, BTreeSet};

use blastit_manifest::{Grant, Manifest, ProxyName};
use blastit_sqlpolicy::{Analysis, TableGrants, normalize};
use serde_json::{Value, json};

use crate::diff::{GrantCheck, grant_of, resolve_proxy};
use crate::error::AuditError;
use crate::ledger::Ledger;
use crate::observe::ObservedSetup;
/// What the *global* groups of a manifest grant, and whether it was needed
/// (§6, D3c).
///
/// A global group is attached to every route at once (`groups = […]` at the top
/// level), so a statement it grants is in every route's effective grant and no
/// route can claim it. Reporting it as unused per route would repeat one dead
/// grant once per route, which is what a manifest with twenty routes and one
/// global group would drown in; reporting it once — here — says the same thing
/// honestly. A statement is unused only when *no* route matched it.
pub(crate) struct GlobalState {
    /// Per proxy, every statement a global group grants, with the groups that
    /// grant it, in manifest order.
    queries: BTreeMap<ProxyName, BTreeMap<String, Vec<String>>>,
    /// Every table grant a global group hands out, and whether any observed
    /// statement needed it.
    tables: Vec<GlobalTables>,
}

/// One global group's table grant on one proxy, and whether it was used.
struct GlobalTables {
    group: String,
    proxy: ProxyName,
    tables: TableGrants,
    used: bool,
}

impl GlobalState {
    /// Collects the global groups' grants, per proxy and per statement.
    pub(crate) fn of(manifest: &Manifest) -> Self {
        let mut queries: BTreeMap<ProxyName, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        let mut tables = Vec::new();
        for name in manifest.global_groups() {
            let Some(group) = manifest.group(name) else {
                continue;
            };
            for (proxy, grant) in group.grants() {
                let Some(pg) = grant.as_postgresql() else {
                    continue;
                };
                for query in pg.queries() {
                    let normalised = normalize(query);
                    if normalised.is_empty() {
                        continue;
                    }
                    queries
                        .entry(proxy.clone())
                        .or_default()
                        .entry(normalised)
                        .or_default()
                        .push(name.clone());
                }
                if !pg.tables().is_empty() {
                    tables.push(GlobalTables {
                        group: name.clone(),
                        proxy: proxy.clone(),
                        tables: pg.tables().clone(),
                        used: false,
                    });
                }
            }
        }
        Self { queries, tables }
    }

    /// Notes that a global group's table grant permitted one observed statement.
    ///
    /// `TableGrants::check` is also happy with a statement that touches nothing
    /// (a `SELECT 1`), so an analysis that needs no grant at all is not evidence
    /// that anything was used.
    pub(crate) fn mark_used(&mut self, proxy: &ProxyName, analysis: &Analysis) {
        let needs_grant =
            !analysis.access_set().is_empty() || analysis.extra_functions().next().is_some();
        if !needs_grant {
            return;
        }
        for tables in &mut self.tables {
            if !tables.used && &tables.proxy == proxy && tables.tables.check(analysis).is_ok() {
                tables.used = true;
            }
        }
    }
}
/// The proxies on which some route was seen *only* in a live server's log
/// (D3c).
///
/// A global group's grant is only called unused when no route matched it — and
/// on such a proxy no route can be *asked*: the log records denials, not the
/// statements that ran, so "nothing matched it" would be a claim about traffic
/// nobody observed.
pub(crate) fn live_only_proxies(manifest: &Manifest, ledger: &Ledger<'_>) -> BTreeSet<ProxyName> {
    ledger
        .routes()
        .filter(|(_, route)| route.requests > 0 && route.suite_requests == 0)
        .flat_map(|(index, _)| manifest.effective_grants(index).into_keys())
        .collect()
}

/// Every `(proxy, statement)` pair the suite has used (§6, D3c).
///
/// Two things make a grant used: a route that matched its statement, and
/// [`setup_section`] — the statements that run while a connection is being
/// established. The second is what makes a global group's
/// `SELECT set_config('TimeZone', …)` used: it only runs when a connection
/// opens, so no *request* can match it, and the connection-setup section is
/// precisely where the report already says the grant covers it. Reading the two
/// together is what keeps a correct manifest from being reported as one grant
/// too many.
pub(crate) fn used_statements(
    matched: &BTreeSet<(ProxyName, String)>,
    setup: &SetupSection,
) -> BTreeSet<(ProxyName, String)> {
    let mut used = matched.clone();
    used.extend(setup.accounted.iter().cloned());
    used
}

/// The `unused_global` section: what the global groups grant that nothing used
/// (§6, D3c).
///
/// A global group is attached to every route, so its statements are in every
/// route's effective grant. Reporting each one as unused once per route would
/// be noise a reader cannot act on; reporting them once, here, is the same
/// information: *no* request in the suite touched it, so the grant — or the
/// requests — can go.
///
/// `matched` is every `(proxy, statement)` the suite used, per
/// [`used_statements`]: a statement a route ran, **or** one
/// [`setup_section`] accounts for. The second is what a connection-setup group
/// needs: `SELECT set_config('TimeZone', …)` never runs inside a request, so no
/// route can match it, and a report that called the group that grants it dead
/// would be telling an operator to delete a grant the same report shows
/// `connection_setup` using.
///
/// * `statements` is every statement a global group grants that nothing used,
///   with the groups that grant it;
/// * `tables` is every `tables`/`functions` grant a global group hands out that
///   no observed statement needed: the group's table grant allowed nothing the
///   suite ran. A statement that only *half* used a group's tables does not
///   make it used in this sense — the whole grant is reported, because that is
///   the unit a manifest author edits.
///
/// A proxy on which a route was only seen in a *live server's* log is skipped
/// whole (D3c): that log records what was denied, never what ran, so no global
/// grant of it can be shown to be dead.
///
/// A statement a route also grants itself stays that route's business: it is
/// reported in the route's own `unused`, and here as well while no route
/// matched it, because the global group is what put it in every route's grant.
pub(crate) fn unused_global_section(
    global: &GlobalState,
    matched: &BTreeSet<(ProxyName, String)>,
    unverifiable: &BTreeSet<ProxyName>,
) -> Value {
    let mut statements: Vec<Value> = Vec::new();
    for (proxy, granted) in &global.queries {
        if unverifiable.contains(proxy) {
            continue;
        }
        for (sql, groups) in granted {
            if matched.contains(&(proxy.clone(), sql.clone())) {
                continue;
            }
            statements.push(json!({
                "proxy": proxy.as_str(),
                "sql": sql,
                "groups": groups,
            }));
        }
    }
    let mut tables: Vec<Value> = Vec::new();
    for grant in &global.tables {
        if grant.used || unverifiable.contains(&grant.proxy) {
            continue;
        }
        tables.push(json!({
            "proxy": grant.proxy.as_str(),
            "group": grant.group,
            "tables": grant.tables.tables(),
            "functions": grant.tables.functions().collect::<Vec<_>>(),
        }));
    }
    let count = statements.len() + tables.len();
    json!({
        "statements": statements,
        "tables": tables,
        "count": count,
    })
}
/// The statements a route is granted *without* the global groups: its own
/// inline grant and the grants of the groups it names.
///
/// This is the set whose unmatched entries are the route's own `unused` lines
/// (§6, D3c): a statement only a global group grants is not this route's to
/// call dead, because that group grants it to every route at once.
pub(crate) fn own_queries(
    manifest: &Manifest,
    index: usize,
    proxy: &ProxyName,
) -> BTreeSet<String> {
    let mut queries = BTreeSet::new();
    let Some(route) = manifest.route(index) else {
        return queries;
    };
    collect_queries(route.grant(proxy), &mut queries);
    for name in route.groups() {
        collect_queries(
            manifest.group(name).and_then(|group| group.grant(proxy)),
            &mut queries,
        );
    }
    queries
}

/// The statements a *group* grants this route: the global groups and the ones
/// the route names.
///
/// The legacy report keeps a group's unmatched statement in the route's own
/// `unused` list, and reports a global group's statement once in
/// `unused_global`; the shared `R34` coverage helper is only consulted for the
/// route's own inline entries, so a group statement's accounting does not
/// change.
pub(crate) fn group_queries(
    manifest: &Manifest,
    index: usize,
    proxy: &ProxyName,
) -> BTreeSet<String> {
    let mut queries = BTreeSet::new();
    let Some(route) = manifest.route(index) else {
        return queries;
    };
    for name in manifest.global_groups().iter().chain(route.groups()) {
        collect_queries(
            manifest.group(name).and_then(|group| group.grant(proxy)),
            &mut queries,
        );
    }
    queries
}

/// Adds a grant's queries, normalised, to a set.
fn collect_queries(grant: Option<&Grant>, into: &mut BTreeSet<String>) {
    let Some(pg) = grant.and_then(Grant::as_postgresql) else {
        return;
    };
    for query in pg.queries() {
        let normalised = normalize(query);
        if !normalised.is_empty() {
            into.insert(normalised);
        }
    }
}

/// The `connection_setup` section of the report: the statements psycopg runs as
/// a connection is established, and which routes do not grant them (§6).
///
/// Every request opens its own connection (§6, "What the app must do"), so in
/// production these statements run on every route that has a listener for that
/// proxy, once per request — while in tests the connection is often reused and
/// they run outside any request at all. They belong in a group attached to
/// every route (`groups = ["connection"]`), so the report names the routes that
/// are missing them rather than diffing them against one route.
///
/// A route with no grant on that proxy gets no listener and never runs them, so
/// it is not counted either way.
pub(crate) struct SetupSection {
    /// The section itself.
    pub(crate) value: Value,
    /// How many distinct statements it holds.
    pub(crate) statements: usize,
    /// How many of them at least one route does not grant.
    pub(crate) ungranted: usize,
    /// The `(proxy, statement)` pairs it accounts for, normalised.
    ///
    /// A statement that only runs while a connection opens is *used*: it is not
    /// a request's statement, so no route matches it, and a global group that
    /// grants it is doing exactly its job (`groups = ["connection"]`). The
    /// report says so by keeping it out of `unused_global` (D3c, §6).
    pub(crate) accounted: BTreeSet<(ProxyName, String)>,
}

/// Builds the `connection_setup` section.
pub(crate) fn setup_section(
    manifest: &Manifest,
    observed: &[ObservedSetup],
) -> Result<SetupSection, AuditError> {
    let mut traces: BTreeMap<(ProxyName, String), SetupTrace> = BTreeMap::new();
    for statement in observed {
        let proxy = resolve_proxy(manifest, statement.proxy.as_deref())?;
        let normalised = normalize(&statement.sql);
        if normalised.is_empty() {
            continue;
        }
        let trace = traces
            .entry((proxy, normalised))
            .or_insert_with(|| SetupTrace {
                wire_sql: statement.sql.clone(),
                ..SetupTrace::default()
            });
        trace.count += statement.count;
    }

    let mut ungranted_total = 0usize;
    let mut statements = Vec::with_capacity(traces.len());
    let mut accounted: BTreeSet<(ProxyName, String)> = BTreeSet::new();
    for ((proxy, sql), mut trace) in traces {
        accounted.insert((proxy.clone(), sql.clone()));
        for index in 0..manifest.route_count() {
            let (Some(route), true) = (
                manifest.route(index),
                manifest.effective_grants(index).contains_key(&proxy),
            ) else {
                continue;
            };
            let grant = grant_of(manifest, index, proxy.as_str());
            // The wire text, not the normalised one: a table grant has to parse
            // it (D7).
            if GrantCheck::of(grant.as_ref()).allows(&trace.wire_sql) {
                trace.granted_in += 1;
            } else {
                trace.ungranted_in.push(route.name().to_string());
            }
        }
        if !trace.ungranted_in.is_empty() {
            ungranted_total += 1;
        }
        statements.push(json!({
            "sql": sql,
            "proxy": proxy.as_str(),
            "count": trace.count,
            "wire_sql": trace.wire_sql,
            "ungranted_in": trace.ungranted_in,
            "granted_in": trace.granted_in,
        }));
    }
    statements.sort_by(|left, right| {
        right["count"]
            .as_u64()
            .cmp(&left["count"].as_u64())
            .then_with(|| left["sql"].as_str().cmp(&right["sql"].as_str()))
    });

    Ok(SetupSection {
        statements: statements.len(),
        value: json!({
            "statements": statements,
            "ungranted": ungranted_total,
        }),
        ungranted: ungranted_total,
        accounted,
    })
}

/// What one connection-setup statement did, and what the manifest says about it.
#[derive(Default)]
struct SetupTrace {
    /// How many times it ran over the session.
    count: u64,
    /// One raw occurrence, for a human who wants to see the placeholders.
    wire_sql: String,
    /// The routes that have a listener for that proxy and do not grant it.
    ungranted_in: Vec<String>,
    /// How many routes that have a listener for that proxy do grant it.
    granted_in: usize,
}
