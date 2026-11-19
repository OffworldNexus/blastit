//! The grant diff itself: what each view asked for, and what it was granted.
//!
//! Both presenters feed their observations into one [`Ledger`], per route:
//!
//! * a request is counted ([`Ledger::request`]) so a route nothing reached is
//!   reported as unexercised rather than as a wall of dead grants;
//! * every statement goes through [`Ledger::record`], which normalises it,
//!   asks the pair's [`GrantCheck`] and files it under `matched` or `missing`.
//!   That pair of sets *is* the answer to "executed but not granted / dead
//!   grant", and it is computed once, here, for `audit` and `check` alike;
//! * egress and the table grant a pair's statements would need are recorded
//!   beside it, because they are per `(route, proxy)` facts too — `check` simply
//!   does not read them.
//!
//! A dead grant is then read off the same record: a granted statement that is
//! not in `matched` was never used. How the two reports *group* that set is
//! their own business (the legacy report keeps a named group's grant under each
//! route, `check` reports it once); what is dead is decided here.

use std::collections::{BTreeMap, BTreeSet};

use blastit_manifest::{Manifest, ProxyName};
use blastit_sqlpolicy::TableGrants;
use serde_json::{Value, json};

use crate::diff::{GrantCheck, TcProbe, grant_locator, grant_of};
use crate::normalize;
use crate::{EXAMPLES, NO_TEST};

/// A set that is always empty, to borrow instead of allocating for an
/// observation that never happened.
pub(crate) static EMPTY_MATCHED: BTreeSet<String> = BTreeSet::new();

/// What one `(route, proxy)` pair did.
#[derive(Default)]
pub struct ProxyLedger {
    /// Statements observed on the pair, granted or not.
    pub executions: usize,
    /// Statements that were executed and not granted, by normalised text.
    pub missing: BTreeMap<String, Occurrence>,
    /// Normalised statements the pair's grant allowed.
    ///
    /// "Allowed", not "listed": an `unrestricted` grant lists nothing, and a
    /// statement a table grant covers is allowed without appearing in
    /// `queries`. Both are statements the route ran and was permitted to run.
    pub matched: BTreeSet<String>,
    /// Whether a transaction-control statement, or a whole request that needs
    /// one, was observed.
    pub transaction_control: bool,
    /// The smallest table grant that covers every statement this pair ran
    /// (§6, D7): what the route would have to write to drop its exact queries.
    pub table_grants: TableGrants,
    /// The statements a table grant cannot describe at all, by normalised text,
    /// with the reason.
    pub unanalysable: BTreeMap<String, String>,
    /// The tables whose row lock this pair's statements could not name a column
    /// for, by the statement that locked them (D3c). A statement here is only
    /// reported when the *merged* grant cannot name one either — another
    /// statement of the route may read a column the lock can use.
    pub nameless_locks: BTreeMap<String, BTreeSet<String>>,
}

/// What one route did in the observed requests.
#[derive(Default)]
pub struct RouteLedger {
    pub requests: usize,
    /// How many of those requests were recorded by this process — the suite
    /// itself — rather than read out of a live server's log (D3c).
    pub suite_requests: usize,
    proxies: BTreeMap<ProxyName, ProxyLedger>,
    egress: BTreeMap<(String, u16), EgressTrace>,
}

impl RouteLedger {
    /// One `(proxy, ledger)` of the route, in name order.
    pub fn proxies(&self) -> impl Iterator<Item = (&ProxyName, &ProxyLedger)> {
        self.proxies.iter()
    }

    /// The record of one proxy, if the route ran anything on it.
    pub fn proxy(&self, proxy: &ProxyName) -> Option<&ProxyLedger> {
        self.proxies.get(proxy)
    }

    /// The network attempts the route made, keyed by `(host, port)` (§7.3).
    pub fn egress(&self) -> &BTreeMap<(String, u16), EgressTrace> {
        &self.egress
    }

    /// Counts one outbound connection attempt and keeps an example of who made
    /// it.
    pub(crate) fn note_egress(
        &mut self,
        attempt: &crate::observe::ObservedEgress,
        nodeid: Option<&str>,
    ) {
        self.egress
            .entry(attempt.key())
            .or_default()
            .record(attempt, nodeid);
    }
}

/// The grant diff of one run, indexed by route.
///
/// Holds the manifest it diffs against, so a statement's pair is checked
/// against the same allowlist dbproxy enforces without the caller passing it in
/// again, and the transaction-control probe both presenters ask the same way.
pub struct Ledger<'a> {
    manifest: &'a Manifest,
    probe: TcProbe,
    routes: BTreeMap<usize, RouteLedger>,
}

/// What one statement was, once the pair's grant had seen it.
pub struct Decision {
    /// The statement, normalised (`$?` placeholders, comments collapsed).
    pub normalized: String,
    /// Whether the pair's grant allows it: `false` is "executed but not
    /// granted".
    pub granted: bool,
    /// Whether it is a single transaction-control statement (§5.2).
    pub transaction_control: bool,
}

impl<'a> Ledger<'a> {
    pub fn new(manifest: &'a Manifest) -> Self {
        Self {
            manifest,
            probe: TcProbe::new(),
            routes: BTreeMap::new(),
        }
    }

    /// Counts one routed request; `live` when a live server's log, rather than
    /// this process, is what saw it (D3c).
    pub fn request(&mut self, route: usize, live: bool) -> &mut RouteLedger {
        let entry = self.routes.entry(route).or_default();
        entry.requests += 1;
        if !live {
            entry.suite_requests += 1;
        }
        entry
    }

    /// Files one observed statement against the pair's grant.
    pub fn record(
        &mut self,
        route: usize,
        proxy: &ProxyName,
        wire_sql: &str,
        nodeid: Option<&str>,
        path: Option<&str>,
    ) -> Decision {
        let check = GrantCheck::of(grant_of(self.manifest, route, proxy.as_str()).as_ref());
        let normalized = normalize(wire_sql);
        let granted = check.allows(wire_sql);
        let transaction_control = self.probe.is_transaction_control(&normalized);
        let entry = self
            .routes
            .entry(route)
            .or_default()
            .proxies
            .entry(proxy.clone())
            .or_default();
        entry.executions += 1;
        if granted {
            entry.matched.insert(normalized.clone());
        } else {
            entry
                .missing
                .entry(normalized.clone())
                .or_default()
                .record(wire_sql, nodeid, path);
        }
        Decision {
            normalized,
            granted,
            transaction_control,
        }
    }

    /// The record of one route, if a request reached it.
    pub fn route(&self, route: usize) -> Option<&RouteLedger> {
        self.routes.get(&route)
    }

    /// How many requests reached `route`; `0` for a route nothing reached.
    pub fn requests(&self, route: usize) -> usize {
        self.route(route).map_or(0, |route| route.requests)
    }

    /// Every route that was reached, in index order.
    pub fn routes(&self) -> impl Iterator<Item = (usize, &RouteLedger)> {
        self.routes.iter().map(|(index, route)| (*index, route))
    }

    /// Whether any proxy of `route` was allowed one normalised statement.
    ///
    /// This is `check`'s dead-grant question, and the only form the answer is
    /// needed in: a grant of that route that no observed statement matched was
    /// never used, whichever store the observation named (`R34`). It walks the
    /// per-proxy sets instead of flattening them, so a route with many grants
    /// costs no allocation per query.
    pub fn matched_contains(&self, route: usize, sql: &str) -> bool {
        self.routes.get(&route).is_some_and(|route| {
            route
                .proxies
                .values()
                .any(|proxy| proxy.matched.contains(sql))
        })
    }

    /// Records that a request on this pair needed transaction control.
    pub fn mark_transaction_control(&mut self, route: usize, proxy: &ProxyName) {
        self.pair_mut(route, proxy).transaction_control = true;
    }

    /// The record of one pair, created on first use.
    pub fn pair_mut(&mut self, route: usize, proxy: &ProxyName) -> &mut ProxyLedger {
        self.routes
            .entry(route)
            .or_default()
            .proxies
            .entry(proxy.clone())
            .or_default()
    }

    /// Counts one outbound connection attempt of a route (§7.3).
    pub(crate) fn note_egress(
        &mut self,
        route: usize,
        attempt: &crate::observe::ObservedEgress,
        nodeid: Option<&str>,
    ) {
        self.routes
            .entry(route)
            .or_default()
            .note_egress(attempt, nodeid);
    }
}

/// One statement executed but not granted, and the first examples of who ran it.
#[derive(Default)]
pub struct Occurrence {
    pub count: usize,
    pub wire_sql: String,
    pub tests: Vec<String>,
    pub paths: Vec<String>,
    seen_tests: BTreeSet<String>,
    seen_paths: BTreeSet<String>,
}

impl Occurrence {
    /// Counts one execution and keeps the first examples of who ran it.
    pub fn record(&mut self, wire_sql: &str, nodeid: Option<&str>, path: Option<&str>) {
        self.count += 1;
        if self.wire_sql.is_empty() {
            self.wire_sql = wire_sql.to_string();
        }
        let test = nodeid.unwrap_or("").trim();
        let test = if test.is_empty() { NO_TEST } else { test };
        if self.tests.len() < EXAMPLES && self.seen_tests.insert(test.to_string()) {
            self.tests.push(test.to_string());
        }
        if let Some(path) = path
            && self.paths.len() < EXAMPLES
            && self.seen_paths.insert(path.to_string())
        {
            self.paths.push(path.to_string());
        }
    }
}

/// What one route did on the network (§7.3).
#[derive(Default)]
pub struct EgressTrace {
    /// `AF_INET`, `AF_INET6`… of the first attempt seen.
    pub family: String,
    /// How many times the route tried to reach it.
    pub count: usize,
    /// Up to five tests that made the attempt.
    pub tests: Vec<String>,
    seen_tests: BTreeSet<String>,
}

impl EgressTrace {
    /// Counts one attempt and keeps an example of who made it.
    fn record(&mut self, attempt: &crate::observe::ObservedEgress, nodeid: Option<&str>) {
        self.count += 1;
        if self.family.is_empty() {
            self.family = attempt.family.clone();
        }
        let test = nodeid.unwrap_or("").trim();
        let test = if test.is_empty() { NO_TEST } else { test };
        if self.tests.len() < EXAMPLES && self.seen_tests.insert(test.to_string()) {
            self.tests.push(test.to_string());
        }
    }
}

/// One `(method, path)` blastit would not have routed.
#[derive(Default)]
pub struct UnroutedTrace {
    pub method: String,
    pub path: String,
    pub count: usize,
    pub statements: usize,
    pub tests: Vec<String>,
    seen: BTreeSet<String>,
}

impl UnroutedTrace {
    pub fn new(method: &str, path: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            ..Self::default()
        }
    }

    /// Counts one request, its statements and the test that made it.
    pub fn record(&mut self, statements: usize, nodeid: Option<&str>) {
        self.count += 1;
        self.statements += statements;
        let test = nodeid.unwrap_or("").trim();
        let test = if test.is_empty() { NO_TEST } else { test };
        if self.tests.len() < EXAMPLES && self.seen.insert(test.to_string()) {
            self.tests.push(test.to_string());
        }
    }
}

/// The `table_grants` section of one `(route, proxy)` pair (§6, D7).
///
/// It is the smallest table grant that covers every statement the pair was
/// observed to run, plus the directives a `Blastfile` view's `postgres` block
/// takes — the rewrite of a route's exact queries into table grants. It is per
/// pair, not per route, because a grant is: a route with two `postgresql` stores
/// has two grants, and the plugin reports each where it belongs.
///
/// `unanalysable` lists the statements a table grant cannot describe at all
/// (a `SET`, a `COPY`, a statement this build does not understand). They are
/// reported, never dropped: the covering grant is then not a complete
/// replacement for the route's exact queries.
pub(crate) fn table_grants_json(
    tables: &TableGrants,
    view: &str,
    store: &str,
    unanalysable: Vec<Value>,
) -> Value {
    let mut section = serde_json::to_value(tables).unwrap_or_else(|_| json!({}));
    if let Some(map) = section.as_object_mut() {
        map.insert(
            "blastfile".to_string(),
            json!(tables.to_blastfile(&grant_locator(view, store))),
        );
        map.insert("unanalysable".to_string(), json!(unanalysable));
    }
    section
}
